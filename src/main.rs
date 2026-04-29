#![allow(unused)]
#![allow(unused_lifetimes)]
pub(crate) mod api;
pub(crate) mod db;
pub(crate) mod webhook;
pub(crate) mod r#macro;
pub(crate) mod parse;
use crate::webhook::events::{Buffer, EventBuffer};
use crate::webhook::handler::webhook_handler;
pub use api::Client as ApiClient;
use axum::{Router, routing::post};
use diesel::PgConnection;
use diesel::r2d2::{self, ConnectionManager};
use dotenvy::dotenv;
use reqwest::Client as HttpClient;
use reqwest::header::{AUTHORIZATION, HeaderMap};
use reqwest_middleware::{
    ClientBuilder, ClientWithMiddleware,
};
use reqwest_retry::RetryTransientMiddleware;
use retry_policies::Jitter;
use retry_policies::policies::ExponentialBackoff;
use std::sync::Arc;
use std::{env, net::SocketAddr, time::Duration};
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;

pub type DbPool =
    r2d2::Pool<ConnectionManager<PgConnection>>;

// https://github.com/diesel-rs/diesel
// https://github.com/oxidecomputer/progenitor

/// Builds a new Progenitor API `Client`
fn build_api_client() -> anyhow::Result<ApiClient> {
    let base_url = env::var("API_URL")?;
    let access_token = env::var("API_ACCESS_TOKEN")?;
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        format!("Bearer {access_token}").parse()?,
    );
    let http_client = HttpClient::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .default_headers(headers)
        .build()?;
    let retry_policy = ExponentialBackoff::builder()
        .retry_bounds(
            Duration::from_millis(250),
            Duration::from_secs(10),
        )
        .jitter(Jitter::Bounded)
        .base(2)
        .build_with_max_retries(3);
    let middleware_client: ClientWithMiddleware =
        ClientBuilder::new(http_client)
            .with(
                RetryTransientMiddleware::new_with_policy(
                    retry_policy,
                ),
            )
            .build();
    Ok(ApiClient::new_with_client(
        &base_url,
        middleware_client,
    ))
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
}

pub fn build_app_state(
    api: ApiClient,
) -> anyhow::Result<AppState> {
    Ok(AppState {
        api: api,
        webhook_secret: env::var("WEBHOOK_SECRET")?,
        webhook_events: EventBuffer::default(),
        db_connection_pool: r2d2::Pool::builder().build(
            ConnectionManager::<PgConnection>::new(
                env::var("DATABASE_URL")?,
            ),
        )?,
    })
}

async fn serve(state: Arc<AppState>) -> anyhow::Result<()> {
    // ----------------------------------------------------------------------------
    let address: String = env::var("APP_ADDRESS")?;
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

async fn sync(state: Arc<AppState>, minutes: Duration) {
    use tokio::time::{Interval, interval};
    // ------------------------------------------------------
    let mut timer: Interval = interval(minutes);
    // ------------------------------------------------------
    tokio::spawn(async move {
        loop {
            timer.tick().await;
            // -----------------------------------------------------------------------------
            let events: Buffer =
                state.webhook_events.drain();
            // ---------------------------------------------------------------------------
            for (i, ids) in events.into_iter().enumerate() {
                let (resource, operation) =
                    EventBuffer::reverse_index(i);
            }
        }
    });
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ------------------------------------------------------
    dotenv().ok();
    // ------------------------------------------------------
    init_tracing(&env::var("LOG_LEVEL")?);
    // ------------------------------------------------------
    let client: ApiClient = build_api_client()?;
    // ------------------------------------------------------
    let app_state: AppState = build_app_state(client)?;
    let app_state: Arc<AppState> = Arc::new(app_state);
    // ------------------------------------------------------
    serve(app_state.clone()).await?;
    // ------------------------------------------------------
    let minutes: u64 = env::var("DATABASE_SYNC_INTERVAL")?
        .parse::<u64>()?;
    // ------------------------------------------------------
    sync(app_state, Duration::from_mins(minutes)).await;
    // ------------------------------------------------------
    Ok(())
}
