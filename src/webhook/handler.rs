use super::variants::{Operation, Resource};
use crate::records::database::removal::ids_deleted_for_resource;
use crate::webhook::events::EventBuffer;
use crate::webhook::payload::WebhookPayload;
use crate::{AppState, records::database::removal::IDS_DELETED};
use anyhow::Context;
use axum::{body::Bytes, extract::State, http::HeaderMap};
use hmac::{Hmac, Mac};
use reqwest::StatusCode;
use sha1::Sha1;
use std::sync::{self, Arc};
use tracing::instrument;

/// ---------------------------------------------------------------
/// Axum route endpoint executed under a `TraceLayer` span
/// for HTTP disambiguation.
///
/// Uses a fallible closure returning `anyhow::Result`
/// to unify signature verification and parsing errors.
///
/// Extracts `(Resource, Operation, id)` on success
/// and buffers the resource ID for retrieval using the simPRO API.
///
/// Errors are logged via tracing and return `400 BAD_REQUEST`.
#[instrument(skip(app, headers, body))]
pub async fn webhook_handler(
    State(app): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    match (|| -> anyhow::Result<_> {
        verify_signature(&app.webhook_secret, &headers, &body)?;
        Ok(parse_webhook(body)?)
    })() {
        Ok((resource, operation, id)) => {

            if matches!(operation, Operation::Created | Operation::Updated)
                && ids_deleted_for_resource(resource).contains(&id)
            {
                
                tracing::error!(
                    ?id, ?resource, ?operation,
                    "Received CREATED or UPDATED webhook when ID marked as DELETED"
                );
                return StatusCode::INTERNAL_SERVER_ERROR;
            }

            let buffer_threshold_exceeded: bool = {
                let mut webhook_events = app.webhook_events.acquire_lock();
                let index: usize = EventBuffer::index(resource, operation);
                // Add the ID to the buffer at the index which
                // corresponds to the Resource, Operation combination
                webhook_events[index].push(id);
                webhook_events[index].len() > app.sync_threshold
            };
            // Persists the enter webhook_events object to a JSON file in case the server crashes
            if let Err(err) = app.webhook_events.persist_to_file(&app.webhook_events_path) {
                tracing::error!(?err, "Failed to persist webhook event buffer");
                return StatusCode::INTERNAL_SERVER_ERROR;
            }
            if buffer_threshold_exceeded {
                let app = app.clone();
                tokio::spawn(async move {
                    if let Err(err) = crate::sync_once(app).await {
                        tracing::error!(?err, "Webhook synchronization failed");
                    }
                });
            }
            StatusCode::OK
        }
        Err(e) => {
            tracing::error!(error = %e);
            StatusCode::BAD_REQUEST
        }
    }
}

/// [SIMPRO API : MESSAGE VERIFICATION](https://developer.simprogroup.com/apidoc/?page=cd8682773ab1b07fdc9661984e281ce3#tag/Message-Verification)
/// --------------------------------------------------------------------------------------------------------------------------------
/// simPRO signs webhook payloads with an HMAC-SHA1 of the raw request body
/// using a shared secret string. The hex-encoded digest is sent in the
/// `X-Response-Signature` header. We recompute with the secret to verify
/// message authenticity.
pub fn verify_signature(secret: &str, headers: &HeaderMap, body: &Bytes) -> anyhow::Result<()> {
    // --------------------------------------------------------
    let signature: &str = headers
        .get("X-Response-Signature")
        .context("Missing X-Response-Signature")?
        .to_str()
        .context("Invalid X-Response-Signature")?;
    // --------------------------------------------------------
    #[cfg(debug_assertions)]
    tracing::debug!("Incoming X-Response-Signature: '{signature}'");
    // --------------------------------------------------------
    Hmac::<Sha1>::new_from_slice(secret.as_bytes())
        .expect("Failed to initialize HMAC-SHA1")
        .chain_update(&body)
        .verify_slice(&hex::decode(signature).context("Failed to decode X-Response-Signature")?)?;
    // --------------------------------------------------------
    Ok(())
}

pub fn parse_webhook(body: Bytes) -> anyhow::Result<(Resource, Operation, i64)> {
    // --------------------------------------------------------
    let payload: WebhookPayload = serde_json::from_slice(&body)?;
    // --------------------------------------------------------
    let resource: Resource = payload.resource().context("Webhook: Missing 'resource'")?;
    // --------------------------------------------------------
    let operation: Operation = payload
        .operation()
        .context("Webhook: Missing 'operation'")?;
    // --------------------------------------------------------
    let resource_id: i64 = payload
        .reference
        .id_for(&resource)
        .context("Webhook: Missing Resource ID")?;
    // --------------------------------------------------------
    #[cfg(debug_assertions)]
    tracing::debug!(
        ?payload,
        ?resource,
        ?operation,
        resource_id,
        "Parsed webhook payload"
    );
    // --------------------------------------------------------
    Ok((resource, operation, resource_id))
}
