use crate::AppState;
use crate::records::database::transpose::EngineerEventRow;
use crate::serve::response::{EngineerEvent, ExtendedProps};
use axum::extract::{Json, State};
use axum::http::StatusCode;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Array, BigInt};
use diesel_async::pooled_connection::deadpool::Object;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use std::sync::Arc;
use tracing::instrument;

#[instrument(skip(app, schedule_ids))]
pub(crate) async fn requests_handler(
    State(app): State<Arc<AppState>>,
    Json(schedule_ids): Json<Vec<i64>>,
) -> Result<Json<Vec<EngineerEvent>>, StatusCode> {

    if schedule_ids.is_empty() {
        return Ok(Json(Vec::new()));
    }

    let mut conn: Object<AsyncPgConnection> =
        app.db_connection_pool.get().await.map_err(
            |err| {
                tracing::error!(
                    ?err,
                    "failed to acquire database connection"
                );
                StatusCode::INTERNAL_SERVER_ERROR
            },
        )?;

    let rows: Vec<EngineerEventRow> = sql_query(
        r#"
        SELECT *
        FROM projections.engineer_events
        WHERE schedule_id = ANY($1)
        "#,
    )
    .bind::<Array<BigInt>, _>(schedule_ids)
    .load(&mut conn)
    .await
    .map_err(|err| {
        tracing::error!(
            ?err,
            "Failed to retrieve engineer events"
        );
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let events: Vec<EngineerEvent> = rows
        .into_iter()
        .map(|row| EngineerEvent {
            id: row.schedule_id.to_string(),
            iso8601_start_time: row.iso8601_start_time,
            iso8601_end_time: row.iso8601_end_time,
            resource_ids: vec![row.employee_id],
            extended_props: ExtendedProps {
                job_id: row.job_id.map(|id| id.to_string()),
                activity_id: row.activity_id,
                cost_centre_id: row
                    .job_cost_center_id
                    .or(row.quote_cost_center_id)
                    .map(|id| id.to_string()),
                site_id: row
                    .site_id
                    .map(|id| id.to_string()),
                status: row.status,
                project: row.project,
                schedule_id: row.schedule_id,
                schedule_date: None,
                employee_name: row.employee_name,
                employee_position: row.employee_position,
                is_core_engineer: row.is_core_engineer,
            },
        })
        .collect();

    Ok(Json(events))
}
