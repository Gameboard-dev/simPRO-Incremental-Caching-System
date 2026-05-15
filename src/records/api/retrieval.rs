use crate::records::api::r#macro::{fetch_by_date, fetch_by_id};
use crate::{
    AppState, api::types as api, parse::schedule::reference::ReferenceIDs, utils::time::TimeRangeExt,
    webhook::variants::{Records, Resource},
};
use chrono::{DateTime, SecondsFormat, Utc};
use std::sync::Arc;

/// The maximum number of results to be returned by a request (`integer [1...250]`)
/// Values above 250 result in the error: "API query parameter should be an integer value between 1 - 250"
pub(crate) const PAGE_SIZE: i64 = 250;
pub(crate) const IDS_PER_REQUEST: usize = 100;

/// Accepts a closure which determines how pages are fetched
/// via the requisite Progenator builder method.
///
/// Requests pages until the endpoint returns fewer than [`PAGE_SIZE`]
/// records, which indicates that the final page has been reached.
///
/// Each returned page is appended into a single `Vec<T>`, so callers receive a
/// flattened result.
///
/// [See Also](https://developer.simprogroup.com/apidoc/?page=ccdb7bf9d93e5652b57cabcc8c41e061#tag/Schedules/operation/c81549288cc61e04c339b32a65425326)
pub(crate) async fn paginate<T, Fut, F>(mut fetch_page: F) -> anyhow::Result<Vec<T>>
where
    F: FnMut(i64) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Vec<T>>>,
{
    let mut page = 1;
    let mut all = Vec::new();
    // ----------------------------------------------------------------------------------------------------
    loop {
        let mut records = fetch_page(page).await?;
        let count = records.len();
        // ----------------------------------------------------------------------------------------------------
        all.append(&mut records);
        // ----------------------------------------------------------------------------------------------------
        if count < PAGE_SIZE as usize {
            break;
        }
        // ----------------------------------------------------------------------------------------------------
        page += 1;
    }

    Ok(all)
}

/// Formats IDs into bounded `ID=in(...)` filters for simPRO list endpoints.
/// IDs are deduplicated before chunking so repeated references do not inflate
/// URL length or cause duplicate records to be fetched.
fn id_filters(ids: &[i64]) -> Vec<String> {
    let mut ids: Vec<i64> = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    ids.chunks(IDS_PER_REQUEST)
        .map(|chunk| {
            let ids: Vec<String> = chunk.iter().map(|id| id.to_string()).collect();
            format!("in({})", ids.join(","))
        })
        .collect()
}

/// Some records can require further resource fetches in a recursive call, so this helper
/// centralizes those calls. The future is boxed because directly awaiting
/// an async function from itself would create a recursive future with no
/// finite compile-time size.
async fn fetch_pinned(resource: Resource, ids: &[i64], app: Arc<AppState>) -> anyhow::Result<Vec<Records>> {
    Box::pin(resource.get_records_by_id(ids, app)).await
}

async fn fetch_schedule_subresources(schedules: &[api::Schedule], app: Arc<AppState>) -> anyhow::Result<Vec<Records>> {
    let mut ids = ReferenceIDs::default();
    // ----------------------------------------------------------------------------------------------------
    for schedule in schedules {
        ids.extend(schedule.parse_reference()?);
    }
    // ----------------------------------------------------------------------------------------------------
    let mut subrecords = vec![];
    for (ids, resource) in ids.resources() {
        // ----------------------------------------------------------------------------------------------------
        if ids.is_empty() {
            continue;
        }
        // ----------------------------------------------------------------------------------------------------
        subrecords.extend(fetch_pinned(resource, ids, app.clone()).await?);
    }
    // ----------------------------------------------------------------------------------------------------
    Ok(subrecords)
}

impl Resource {
    /// Fetches records from simPRO by resource ID and hydrates any
    /// dependent records required for database upsertion
    /// returning them in a dependency-ordered list.
    ///
    /// simPRO list endpoints support search operators such as `in(...)`, which is
    /// used here to request multiple IDs in a single API call:
    /// <https://developer.simprogroup.com/apidoc/?page=ff7c0fcd6a31e735a61c001f75426961#tag/Get-resource>
    ///
    /// The equivalent Linux shell query would be:
    /// ```bash
    /// curl -sS \
    ///   --request GET \
    ///   --url 'HTTP_REQUEST_URL' \
    ///   --header "Authorization: Bearer $SIMPRO_API_KEY" \
    ///   | jq
    /// ```
    /// Use this to test the same query.
    #[allow(unused)]
    #[tracing::instrument(skip(self, ids, app))]
    pub(crate) async fn get_records_by_id(&self, ids: &[i64], app: Arc<AppState>) -> anyhow::Result<Vec<Records>> {
        let id_filters = id_filters(ids);

        let records = match self {
            Resource::Schedule => {
                let schedules: Vec<api::Schedule> =
                    fetch_by_id!(app, id_filters, get_schedules, api::Schedule, "Schedule");
                // ----------------------------------------------------------------------------------------------------
                let dependencies = fetch_schedule_subresources(&schedules, app.clone()).await?;
                // ----------------------------------------------------------------------------------------------------
                dependencies
                    .into_iter()
                    .chain([Records::Schedule(schedules)])
                    .collect()
            },

            Resource::Job => {
                let jobs: Vec<api::Job> = fetch_by_id!(app, id_filters, get_jobs, api::Job, "Job");
                // --------------------------------------------------------------------------------------
                let site_ids: Vec<i64> = jobs.iter().map(|job| job.site.id).collect();
                // --------------------------------------------------------------------------------------
                let site_dependencies: Records = fetch_pinned(Resource::Site, &site_ids, app.clone())
                    .await?
                    .pop()
                    .ok_or_else(|| anyhow::anyhow!("Expected site records"))?;
                // --------------------------------------------------------------------------------------
                vec![site_dependencies, Records::Job(jobs)]
            },

            Resource::CostCenter => {
                vec![Records::CostCenter(fetch_by_id!(
                    app,
                    id_filters,
                    get_cost_centers,
                    api::CostCenter,
                    "CostCenter"
                ))]
            },
            Resource::Quote => {
                vec![Records::Quote(fetch_by_id!(app, id_filters, get_quotes, api::Quote, "Quote"))]
            },
            Resource::Lead => {
                vec![Records::Lead(fetch_by_id!(app, id_filters, get_leads, api::Lead, "Lead"))]
            },
            Resource::Site => {
                vec![Records::Site(fetch_by_id!(app, id_filters, get_sites, api::Site, "Site"))]
            },
            Resource::Employee => {
                vec![Records::Employee(fetch_by_id!(
                    app,
                    id_filters,
                    get_employees,
                    api::Employee,
                    "Employee"
                ))]
            },
            Resource::Activity => {
                vec![Records::Activity(fetch_by_id!(
                    app,
                    id_filters,
                    get_activities,
                    api::Activity,
                    "Activity"
                ))]
            },
        };

        Ok(records)
    }

    /// Fetches records created within a date range, then hydrates any related
    /// records they reference.
    ///
    /// The returned [`Records`] are ordered so dependent records appear
    /// before the records that reference them, matching database upsert order.
    ///
    /// The supplied `(start, end)` [`DateTime<Utc>`] tuple is formatted as
    /// a simPRO `between(...)` filter, then passed to the resource endpoint, see:
    /// <https://developer.simprogroup.com/apidoc/?page=ff7c0fcd6a31e735a61c001f75426961#tag/Get-resource>
    ///
    /// Example:
    /// `Date=between(2026-04-14T09:32:15Z,2026-05-14T09:32:15Z)`
    #[tracing::instrument(skip(self, dates_between, app))]
    pub(crate) async fn get_records_by_date(
        &self,
        dates_between: (DateTime<Utc>, DateTime<Utc>),
        app: Arc<AppState>,
    ) -> anyhow::Result<Vec<Records>> {
        let (start, end) = dates_between.to_format("%Y-%m-%d");
        let dates_between: String = format!("between({},{})", start, end);
        // -------------------------------------------------------------------------------------
        let records: Vec<Records> = match self {
            Resource::Schedule => {
                let schedules: Vec<api::Schedule> =
                    fetch_by_date!(app, dates_between, get_schedules, api::Schedule, "Schedule");
                let dependencies: Vec<Records> = fetch_schedule_subresources(&schedules, app.clone()).await?;
                dependencies
                    .into_iter()
                    .chain([Records::Schedule(schedules)])
                    .collect()
            },
            resource => {
                anyhow::bail!("get_records_by_date is not implemented for {resource:?}");
            },
        };
        Ok(records)
    }
}
