use crate::AppState;
use crate::api::types::Schedule;
use crate::time::TimeRangeExt;
use anyhow::Context;
use chrono::Months;
use chrono::prelude::DateTime;
use chrono::{SecondsFormat, Utc};
use std::sync::Arc;

/// Returns a `(start, end)` [`DateTime<Utc>`] tuple as a time range.
pub(crate) fn time_range(base: DateTime<Utc>, sub_months: u32, add_months: u32) -> (DateTime<Utc>, DateTime<Utc>) {
    let start: DateTime<Utc> = base
        .checked_sub_months(Months::new(sub_months))
        .expect("Invalid start date");

    let end: DateTime<Utc> = base
        .checked_add_months(Months::new(add_months))
        .expect("invalid end date");

    return (start, end);
}

/// Webhook delivery is incremental and only captures events that occur
/// after the service begins listening. On startup, the local cache may
/// be missing historical schedules and their dependent records
/// (Jobs, Sites, Employees, Activities, Quotes, Leads, etc.).
///
/// This bootstrap step seeds the database before the normal webhook-based
/// incremental synchronization loop begins, retrieving all schedules created
/// or modified within the last calendar month up to the current UTC timestamp,
/// and persisting all related records into the database.
///
/// How does simPRO pagination work?
///
/// ### Documentation
/// * [API Operators](https://developer.simprogroup.com/apidoc/?page=ff7c0fcd6a31e735a61c001f75426961#tag/Search-resources)
/// * [GET Schedules API](https://developer.simprogroup.com/apidoc/?page=ccdb7bf9d93e5652b57cabcc8c41e061#tag/Schedules/operation/c81549288cc61e04c339b32a65425326)
#[tracing::instrument(skip(app))]
pub(crate) async fn load_initial_records(app: Arc<AppState>) -> anyhow::Result<()> {
    use crate::records::get_::Records;
    use crate::webhook::variants::Resource;

    let (add_months, sub_months) = (3, 3);
    let records_batches: Vec<Records> = Resource::Schedule
        .get_records_by_date(time_range(Utc::now(), add_months, sub_months), app.clone())
        .await?;

    let pool = app.db_connection_pool.clone();
    let mut conn = pool.get().await?;
    for records in records_batches {
        records.resource().upsert_records(records, &mut conn).await?;
    }

    Ok(())
}
