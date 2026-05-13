//! * https://github.com/diesel-rs/diesel
//! * https://github.com/oxidecomputer/progenitor

#![allow(unused)]
#![allow(unused_lifetimes)]
pub(crate) mod api;
pub(crate) mod db;
pub(crate) mod parse;
pub(crate) mod records;
pub(crate) mod webhook;
use crate::webhook::events::{Buffer, EventBuffer};
use crate::webhook::handler::webhook_handler;
use anyhow::Context;
pub use api::Client as ApiClient;
use axum::{Router, routing::post};
use diesel::PgConnection;
use diesel::r2d2::{ConnectionManager, PooledConnection};
use dotenvy::dotenv;
use reqwest::Client as HttpClient;
use reqwest::header::{AUTHORIZATION, HeaderMap};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::RetryTransientMiddleware;
use retry_policies::Jitter;
use retry_policies::policies::ExponentialBackoff;
use std::sync::Arc;
use std::{env, net::SocketAddr, time::Duration};
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;

pub type DbPool = r2d2::Pool<ConnectionManager<PgConnection>>;

/// Reads a required environment variable.
/// * Returns a contextualized error if the variable is not present.
pub(crate) fn require_env(name: &str) -> anyhow::Result<String> {
    env::var(name).with_context(|| format!("{name} is missing from .env or the environment"))
}

/// Builds a new Progenitor API `Client`
fn build_api_client() -> anyhow::Result<ApiClient> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        format!("Bearer {}", require_env("SIMPRO_API_KEY")?).parse()?,
    );
    let http_client = HttpClient::builder()
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
    let base_url = format!(
        "https://{}/api/v1.0/companies/{}/",
        require_env("SIMPRO_DOMAIN")?,
        require_env("SIMPRO_COMPANY_ID")?
    );
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

#[derive(Debug)]
pub struct AppState {
    pub api: ApiClient,
    pub webhook_secret: String,
    pub webhook_events: EventBuffer,
    pub db_connection_pool: DbPool,
    sync_threshold: usize,
}

pub fn build_app_state(api: ApiClient) -> anyhow::Result<AppState> {
    Ok(AppState {
        api: api,
        webhook_secret: require_env("SIMPRO_WEBHOOK_SECRET")?,
        webhook_events: EventBuffer::default(),
        sync_threshold: require_env("SYNC_THRESHOLD")?.parse::<usize>()?,
        db_connection_pool: r2d2::Pool::builder()
            .build(ConnectionManager::<PgConnection>::new(require_env("DATABASE_URL")?))?,
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
            //.route("/events", post(fetch_events))
            .layer(TraceLayer::new_for_http())
            .with_state(state),
    )
    .await?;
    Ok(())
}

/// This executes a single synchronization pipeline.
///
/// Buffered webhook events are drained and grouped by `(Resource, Operation)`.
///
/// The corresponding simPRO records are then:
/// 1. Retrieved from the simPRO API using the resource-specific GET endpoint
/// 2. Hydrated into strongly typed API models
/// 3. Translated into Diesel insertable structs
/// 4. Upserted into the local PostgreSQL database
///
async fn sync_once(app: Arc<AppState>) -> anyhow::Result<()> {
    use crate::records::hydrate::Records;
    // --------------------------------------------------------
    let events: Buffer = app.webhook_events.drain();
    // --------------------------------------------------------
    for (i, record_ids) in events.into_iter().enumerate() {
        if !record_ids.is_empty() {
            let (resource, operation) = EventBuffer::reverse_index(i);
            let records: Records = resource.get_records(&record_ids, app.clone()).await?;
            resource.upsert_records(records, &mut app.db_connection_pool.get()?)?;
        }
    }
    Ok(())
}

/// Begins the background webhook synchronization worker.
///
/// Runs [sync_once] every N `seconds`.
///
/// This runs continuously in the background using `tokio::spawn`.
///
/// Errors during synchronization are logged and do not terminate the worker.
async fn start_sync(app: Arc<AppState>, seconds: Duration) -> anyhow::Result<()> {
    use tokio::time::{Interval, interval};
    let mut timer: Interval = interval(seconds);
    // --------------------------------------------------------
    tokio::spawn(async move {
        loop {
            if let Err(err) = sync_once(app.clone()).await {
                tracing::error!(?err, "Webhook synchronization failed");
            }
            timer.tick().await;
        }
    });
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ------------------------------------------------------
    dotenv().ok();
    // ------------------------------------------------------
    init_tracing(&require_env("RUST_LOG_LEVEL")?);
    // ------------------------------------------------------
    let client: ApiClient = build_api_client()?;
    // ------------------------------------------------------
    let app_state: AppState = build_app_state(client)?;
    let app_state: Arc<AppState> = Arc::new(app_state);
    // ------------------------------------------------------
    serve(app_state.clone()).await?;
    // ------------------------------------------------------
    let seconds: u64 = require_env("DATABASE_SYNC_INTERVAL")?.parse::<u64>()?;
    // ------------------------------------------------------
    start_sync(app_state, Duration::from_secs(seconds)).await;
    // ------------------------------------------------------
    Ok(())
}
