//! * https://github.com/diesel-rs/diesel
//! * https://github.com/oxidecomputer/progenitor

#![allow(unused)]
#![allow(unused_lifetimes)]
pub(crate) mod api;
pub(crate) mod db;
pub(crate) mod parse;
pub(crate) mod records;
pub(crate) mod serve;
pub(crate) mod time;
pub(crate) mod webhook;
use crate::records::get_records::load_initial_records;
use crate::records::remove_records::IDS_DELETED;
use crate::serve::serve::requests_handler;
use crate::webhook::events::{Buffer, EventBuffer};
use crate::webhook::handler::webhook_handler;
use crate::webhook::variants::Operation;
use anyhow::Context;
pub use api::Client as ApiClient;
use axum::{
    Router,
    routing::{get, post},
};
use diesel_async::AsyncPgConnection;
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::pooled_connection::deadpool::Pool;
use dotenvy::dotenv;
use reqwest::Client as HttpClient;
use reqwest::header::{AUTHORIZATION, HeaderMap};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::RetryTransientMiddleware;
use retry_policies::Jitter;
use retry_policies::policies::ExponentialBackoff;
use std::path::PathBuf;
use std::sync::Arc;
use std::{env, net::SocketAddr, time::Duration};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tower_http::trace::TraceLayer;
use tracing::instrument;

/// This uses `diesel-async` `0.9.0` with a `deadpool` connection pool,
/// rather than synchronous Diesel with `r2d2`.
/// With `diesel-async`, query execution uses `AsyncPgConnection` and async Diesel traits
/// such as `diesel_async::RunQueryDsl`.
///
/// The original implementation used Diesel’s synchronous `PgConnection`,
/// which performed blocking database I/O. In an async Tokio application,
/// that meant database writes had to be wrapped in `tokio::task::spawn_blocking(...)`
/// so they ran on Tokio’s blocking thread pool instead of occupying async worker threads.
pub type DbPool = Pool<AsyncPgConnection>;

/// Reads a required environment variable.
/// * This returns a contextualized error if the variable is not present.
pub(crate) fn require_env(name: &str) -> anyhow::Result<String> {
    env::var(name).with_context(|| format!("{name} is missing from .env or the environment"))
}

/// This builds a new Progenitor API `Client`
fn build_api_client() -> anyhow::Result<ApiClient> {
    let mut headers = HeaderMap::new();
    // simPRO offers a fixed `Grant Token` which never needs to be refreshed or revalidated
    // a more complex alternative would be OAuth2 which requires a managed token lifecycle
    headers.insert(
        AUTHORIZATION,
        format!("Bearer {}", require_env("SIMPRO_API_KEY")?).parse()?,
    );
    let http_client = HttpClient::builder()
        // Ignore HTTP(S)_PROXY environment variables inside the container.
        // Invalid proxy schemes can cause reqwest connection failures before the request is sent.
        .no_proxy()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .default_headers(headers)
        .build()?;
    let retry_policy = ExponentialBackoff::builder()
        .retry_bounds(Duration::from_millis(250), Duration::from_secs(10))
        .jitter(Jitter::Bounded)
        .base(2)
        .build_with_max_retries(3);
    let middleware_client: ClientWithMiddleware = ClientBuilder::new(http_client)
        .with(RetryTransientMiddleware::new_with_policy(retry_policy))
        .build();
    let base_url = format!("https://{}", require_env("SIMPRO_DOMAIN")?);
    Ok(ApiClient::new_with_client(&base_url, middleware_client))
}

fn init_tracing(level: &str) {
    use tracing_error::ErrorLayer;
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{EnvFilter, fmt};
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(ErrorLayer::default())
        .with(EnvFilter::new(level))
        .init();
}

pub struct AppState {
    pub api: ApiClient,
    pub company_id: String,
    pub webhook_secret: String,
    pub webhook_events: EventBuffer,
    pub webhook_events_path: PathBuf,
    pub db_connection_pool: DbPool,
    pub sync_threshold: usize,
}

pub async fn build_app_state(api: ApiClient) -> anyhow::Result<AppState> {
    let database_url = require_env("DATABASE_URL")?;
    let manager = AsyncDieselConnectionManager::<AsyncPgConnection>::new(database_url);
    let webhook_events_path = PathBuf::from(
        env::var("WEBHOOK_EVENTS_FILE").unwrap_or_else(|_| "data/webhook-events.json".to_string()),
    );
    let webhook_events = EventBuffer::load_from_file(&webhook_events_path).unwrap_or_else(|err| {
        tracing::warn!(
            ?err,
            "Failed to load persisted webhook events; starting empty"
        );
        EventBuffer::default()
    });
    Ok(AppState {
        api,
        company_id: require_env("SIMPRO_COMPANY_ID")?,
        webhook_secret: require_env("SIMPRO_WEBHOOK_SECRET")?,
        webhook_events,
        webhook_events_path,
        sync_threshold: require_env("SYNC_THRESHOLD")?.parse::<usize>()?,
        db_connection_pool: Pool::builder(manager).build()?,
    })
}

async fn serve(state: Arc<AppState>) -> anyhow::Result<()> {
    // ----------------------------------------------------------------------------
    let address: String = require_env("LISTEN_ADDRESS")?;
    let address: SocketAddr = address.parse()?;
    // ----------------------------------------------------------------------------
    tracing::info!("Listening on http://{address}");
    // ----------------------------------------------------------------------------
    axum::serve(
        TcpListener::bind(address).await?,
        Router::new()
            .route("/webhook/simpro", post(webhook_handler))
            .route("/events", get(requests_handler))
            .layer(TraceLayer::new_for_http())
            .with_state(state),
    )
    .await?;
    Ok(())
}

/// This executes a single synchronization pipeline:
/// * Buffered webhook events are drained and grouped by `(Resource, Operation)`
/// * The corresponding simPRO records are then:
///     1. Retrieved from the simPRO API using the resource-specific GET endpoint
///     2. Hydrated into strongly typed API models
///     3. Translated into Diesel insertable structs
///     4. Upserted into the local PostgreSQL database
#[instrument(skip(app))]
async fn sync_once(app: Arc<AppState>) -> anyhow::Result<()> {
    use crate::records::get_records::Records;
    // --------------------------------------------------------
    let events: Buffer = app.webhook_events.snapshot();
    // --------------------------------------------------------
    #[cfg(debug_assertions)]
    tracing::debug!(?events, "Synchronizing");
    // --------------------------------------------------------
    for (i, record_ids) in events.iter().enumerate() {
        if !record_ids.is_empty() {
            let (resource, operation) = EventBuffer::reverse_index(i);
            // --------------------------------------------------------------------
            let pool = app.db_connection_pool.clone();
            // --------------------------------------------------------------------
            // Diesel's native PgConnection API is synchronous/blocking.
            // This uses `diesel-async` crate rather than `tokio::task::spawn_blocking`.
            // --------------------------------------------------------------------
            let mut conn = pool.get().await?;
            // --------------------------------------------------------------------
            match operation {
                Operation::Deleted => {
                    // ----------------------------------------------------------------------------------------------------
                    // Remove the corresponding schedules by ID from the database when a deletion webhook is recieved
                    // to avoid returning stale data for clients requesting Engineer bookings.
                    // ----------------------------------------------------------------------------------------------------
                    resource.remove_records_by_id(record_ids, &mut conn).await?;
                    // ----------------------------------------------------------------------------------------------------
                    // Add deleted IDs to a hashmap per resource to deal with the scenario where a 
                    // DELETED webhook is recieved and synchronized before the CREATED webhook is recieved.
                    // `extend` ignores duplicates because `IDS_DELETED` is a `HashSet`.
                    // ----------------------------------------------------------------------------------------------------
                    IDS_DELETED[resource as usize]
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .extend(record_ids);
                    // ----------------------------------------------------------------------------------------------------
                }
                Operation::Created | Operation::Updated => {
                    // --------------------------------------------------------------------------------
                    #[cfg(debug_assertions)]
                    tracing::debug!(pair = ?(resource, operation), "Hydrating and Persisting");
                    // --------------------------------------------------------------------------------

                    // --------------------------------------------------------------------------------
                    let records_variants: Vec<Records> =
                        resource.get_records_by_id(record_ids, app.clone()).await?;
                    // --------------------------------------------------------
                    #[cfg(debug_assertions)]
                    tracing::debug!(?records_variants, "Upserting");
                    // --------------------------------------------------------------------------------------------
                    // Database upsertion (INSERT OR UPDATE) using dependency-ordered array of record arrays
                    // --------------------------------------------------------------------------------------------
                    for records in records_variants {
                        records
                            .resource()
                            .upsert_records(records, &mut conn)
                            .await?;
                    }
                }
            }
        }
    }
    // --------------------------------------------------------------------
    // Mark this snapshot as having been synchronized only if every
    // batch has completed successfully.
    // --------------------------------------------------------------------
    // ON CONFLICT DO UPDATE makes repeated synchronization
    // attempts with the same ID snapshot idempotent.
    // --------------------------------------------------------------------
    app.webhook_events.remove_synced(&events);
    app.webhook_events
        .persist_to_file(&app.webhook_events_path)?;
    Ok(())
}

/// --------------------------------------------------------------------
/// Begin the background webhook synchronization worker.
///
/// Runs [sync_once] every N `seconds`.
///
/// This runs continuously in the background using `tokio::spawn`.
///
/// Errors during synchronization are logged and do not terminate the worker.
/// --------------------------------------------------------------------
async fn sync_worker(app: Arc<AppState>, seconds: Duration) {
    let mut timer = tokio::time::interval(seconds);

    loop {
        timer.tick().await;

        if let Err(err) = sync_once(app.clone()).await {
            tracing::error!(?err, "Webhook synchronization failed");
        }
    }
}

/// ```bash
/// docker compose --profile dev up --build
/// ```
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ------------------------------------------------------
    dotenv().ok();
    // ------------------------------------------------------
    init_tracing(&require_env("RUST_LOG_LEVEL")?);
    // ------------------------------------------------------
    let client: ApiClient = build_api_client()?;
    // ------------------------------------------------------
    let app_state: AppState = build_app_state(client).await?;
    let app_state: Arc<AppState> = Arc::new(app_state);
    // ------------------------------------------------------
    load_initial_records(app_state.clone()).await?;
    // ------------------------------------------------------
    let seconds: u64 = require_env("DATABASE_SYNC_INTERVAL")?.parse::<u64>()?;
    let sync_task: JoinHandle<()> =
        tokio::spawn(sync_worker(app_state.clone(), Duration::from_secs(seconds)));
    // ------------------------------------------------------
    serve(app_state).await?;
    // ------------------------------------------------------
    sync_task.abort();
    Ok(())
}
