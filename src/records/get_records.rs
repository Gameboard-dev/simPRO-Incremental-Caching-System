use chrono::{DateTime, Months, SecondsFormat, Utc};

use crate::{
    AppState, api::types as api, parse::reference::IDs, time::TimeRangeExt,
    webhook::variants::Resource,
};
use std::sync::Arc;

/// The maximum number of results to be returned by a request (`integer [1...250]`)
/// Values above 250 result in the error: "API query parameter should be an integer value between 1 - 250"
pub(crate) const PAGE_SIZE: i64 = 250;

/// simPRO list endpoints return at most [`PAGE_SIZE`] records per request,
/// so a single API call may not contain the complete result set.
///
/// This helper keeps requesting pages until the endpoint returns fewer than [`PAGE_SIZE`]
/// records, which indicates that the final page has been reached.
/// 
/// It accepts a closure which determines how pages are fetched
/// via the requisite Progenator builder method.
///
/// Each returned page is appended into a single `Vec<T>`, so callers receive a
/// flattened result.
///
/// This keeps pagination centralized and makes endpoint-specific code only
/// responsible for describing how to fetch one page.
///
/// ### Documentation:
/// https://developer.simprogroup.com/apidoc/?page=ccdb7bf9d93e5652b57cabcc8c41e061#tag/Schedules/operation/c81549288cc61e04c339b32a65425326
pub(crate) async fn paginate<T, Fut, F>(mut fetch_page: F) -> anyhow::Result<Vec<T>>
where
    F: FnMut(i64) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Vec<T>>>,
{
    let mut page = 1;
    let mut all = Vec::new();

    loop {
        let mut records = fetch_page(page).await?;
        let count = records.len();

        all.append(&mut records);

        if count < PAGE_SIZE as usize {
            break;
        }

        page += 1;
    }

    Ok(all)
}

/// Enum of records returned by API endpoints
#[derive(Debug)]
pub(crate) enum Records {
    Schedule(Vec<api::Schedule>),
    CostCenter(Vec<api::CostCenter>),
    Quote(Vec<api::Quote>),
    Lead(Vec<api::Lead>),
    Job(Vec<api::Job>),
    Site(Vec<api::Site>),
    Employee(Vec<api::Employee>),
    Activity(Vec<api::Activity>),
}

impl Records {
    /// Reverse mapping for dependency-ordered upserts
    pub(crate) fn resource(&self) -> Resource {
        match self {
            Records::Schedule(_) => Resource::Schedule,
            Records::CostCenter(_) => Resource::CostCenter,
            Records::Quote(_) => Resource::Quote,
            Records::Lead(_) => Resource::Lead,
            Records::Job(_) => Resource::Job,
            Records::Site(_) => Resource::Site,
            Records::Employee(_) => Resource::Employee,
            Records::Activity(_) => Resource::Activity,
        }
    }
}

impl Resource {
    /// # get_records_by_id
    /// 
    /// Fetches records from simPRO by their resource IDs and hydrates any
    /// dependent records required for database upsertion.
    ///
    /// simPRO list endpoints support search operators such as `in(...)`, which is
    /// used here to request multiple IDs in a single API call:
    /// <https://developer.simprogroup.com/apidoc/?page=ff7c0fcd6a31e735a61c001f75426961#tag/Get-resource>
    ///
    /// Resource endpoints:
    /// * [Activity](https://developer.simprogroup.com/apidoc/?page=d78ed35383108fb6c04c16d0a11b20fe#tag/Activities/operation/c88605b27f7e8a3873047d9af3a93574)
    /// * [Site](https://developer.simprogroup.com/apidoc/?page=3faa64303d5f5bcd043bb88f6768e603#tag/Sites/operation/101d05972386dfa7536b58fe655d382e)
    /// * [Job](https://developer.simprogroup.com/apidoc/?page=12ceff2290bb9039beaa8f36d5dec226#tag/Jobs/operation/9ca8d728df9f031d2828e79cbb093702)
    /// * [Employee](https://developer.simprogroup.com/apidoc/?page=eb626c94531ec554f93b2b78a77c8b1b#tag/Employees/operation/ad2cdcfe3653fce4e460e4468acc2867)
    /// * [Schedule](https://developer.simprogroup.com/apidoc/?page=ccdb7bf9d93e5652b57cabcc8c41e061#tag/Schedules/operation/4a005958478750b0f96cb00b3c9da0f6)
    ///
    /// This is the normal webhook-driven hydration path: webhook events provide
    /// record IDs, and those IDs are converted into an `in(...)` filter,
    /// and the resulting records are returned in database dependency order for upsertion.
    ///
    /// The equivalent query can be tested in the Linux shell using:
    /// 
    /// ```bash
    /// curl -sS \
    ///   --request GET \
    ///   --url 'https://grainconnect.simprosuite.com/api/v1.0/companies/0/jobs/?ID=in(156344,156344)&columns=Customer,DateModified,ID,Name,Site,Stage,Status,Type&page=1&pageSize=250' \
    ///   --header "Authorization: Bearer $SIMPRO_API_KEY" \
    ///   | jq
    /// ```
    /// 
    /// Note that deduplication of IDs is not required here because the simPRO API will 
    /// return only unique records even if duplicate records are requested (e.g., with ID=in(157,157))
    #[allow(unused)]
    #[tracing::instrument(skip(self, ids, app))]
    pub(crate) async fn get_records_by_id(
        &self,
        ids: &[i64],
        app: Arc<AppState>,
    ) -> anyhow::Result<Vec<Records>> {
        use crate::api::Columns;

        let ids: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        let ids = format!("in({})", ids.join(","));

        let records = match self {
            Resource::Schedule => {
                let schedules: Vec<api::Schedule> = paginate(|page| {
                    let app = app.clone();
                    let ids = ids.clone();
                    async move {
                        Ok(app
                            .api
                            .get_schedules()
                            .id(ids)
                            .columns(api::Schedule::COLUMNS.join(","))
                            .company_id(&app.company_id)
                            .page(page)
                            .page_size(PAGE_SIZE)
                            .send()
                            .await
                            .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Schedule'"))?
                            .into_inner())
                    }
                })
                .await?;

                let mut bin = IDs::default();

                for schedule in &schedules {
                    schedule.parse_id_reference(&mut bin)?;
                }

                let mut records = vec![];
                for (ids, resource) in bin.resources() {
                    if ids.is_empty() {
                        continue;
                    }
                    // Recursive async futures have an indeterminate size at compile time.
                    // `Box::pin` allocates the future on the heap with a stable memory address,
                    // allowing recursive async calls without creating potentially infinitely-sized futures.
                    records.extend(Box::pin(resource.get_records_by_id(ids, app.clone())).await?);
                }

                // --------------------------------------------------------------------------------------
                // Record arrays are returned in database dependency order
                records.push(Records::Schedule(schedules));
                records
            }

            Resource::Job => {
                let jobs: Vec<api::Job> = paginate(|page| {
                    let app = app.clone();
                    let ids = ids.clone();
                    async move {
                        Ok(app
                            .api
                            .get_jobs()
                            .id(ids)
                            .page(page)
                            .page_size(PAGE_SIZE)
                            .columns(api::Job::COLUMNS.join(","))
                            .company_id(&app.company_id)
                            .send()
                            .await
                            .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Job'"))?
                            .into_inner())
                    }
                })
                .await?;
                // --------------------------------------------------------------------------------------
                let site_ids: Vec<i64> = jobs.iter().map(|job| job.site.id).collect();
                // --------------------------------------------------------------------------------------
                let mut sites: Records =
                    Box::pin(Resource::Site.get_records_by_id(&site_ids, app.clone()))
                        .await?
                        .pop()
                        .ok_or_else(|| anyhow::anyhow!("Expected site records"))?;
                // --------------------------------------------------------------------------------------
                // Record arrays are returned in database dependency order
                vec![sites, Records::Job(jobs)]
            }

            Resource::CostCenter => vec![Records::CostCenter(
                paginate(|page| {
                    let app = app.clone();
                    let ids = ids.clone();
                    async move {
                        Ok(app
                            .api
                            .get_cost_centers()
                            .id(ids)
                            .page(page)
                            .page_size(PAGE_SIZE)
                            .columns(api::CostCenter::COLUMNS.join(","))
                            .company_id(&app.company_id)
                            .send()
                            .await
                            .inspect_err(|err| {
                                tracing::error!(?err, "Failed to fetch 'Cost Center'")
                            })?
                            .into_inner())
                    }
                })
                .await?,
            )],
            Resource::Quote => vec![Records::Quote(
                paginate(|page| {
                    let app = app.clone();
                    let ids = ids.clone();
                    async move {
                        Ok(app
                            .api
                            .get_quotes()
                            .id(ids)
                            .page(page)
                            .page_size(PAGE_SIZE)
                            .columns(api::Quote::COLUMNS.join(","))
                            .company_id(&app.company_id)
                            .send()
                            .await
                            .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Quote'"))?
                            .into_inner())
                    }
                })
                .await?,
            )],

            Resource::Lead => vec![Records::Lead(
                paginate(|page| {
                    let app = app.clone();
                    let ids = ids.clone();
                    async move {
                        Ok(app
                            .api
                            .get_leads()
                            .id(ids)
                            .page(page)
                            .page_size(PAGE_SIZE)
                            .columns(api::Lead::COLUMNS.join(","))
                            .company_id(&app.company_id)
                            .send()
                            .await
                            .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Lead'"))?
                            .into_inner())
                    }
                })
                .await?,
            )],

            Resource::Site => vec![Records::Site(
                paginate(|page| {
                    let app = app.clone();
                    let ids = ids.clone();
                    async move {
                        Ok(app
                            .api
                            .get_sites()
                            .id(ids)
                            .page(page)
                            .page_size(PAGE_SIZE)
                            .columns(api::Site::COLUMNS.join(","))
                            .company_id(&app.company_id)
                            .send()
                            .await
                            .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Site'"))?
                            .into_inner())
                    }
                })
                .await?,
            )],

            Resource::Employee => vec![Records::Employee(
                paginate(|page| {
                    let app = app.clone();
                    let ids = ids.clone();
                    async move {
                        Ok(app
                            .api
                            .get_employees()
                            .id(ids)
                            .page(page)
                            .page_size(PAGE_SIZE)
                            .columns(api::Employee::COLUMNS.join(","))
                            .company_id(&app.company_id)
                            .send()
                            .await
                            .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Employee'"))?
                            .into_inner())
                    }
                })
                .await?,
            )],

            Resource::Activity => vec![Records::Activity(
                paginate(|page| {
                    let app = app.clone();
                    let ids = ids.clone();
                    async move {
                        Ok(app
                            .api
                            .get_activities()
                            .id(ids)
                            .page(page)
                            .page_size(PAGE_SIZE)
                            .columns(api::Activity::COLUMNS.join(","))
                            .company_id(&app.company_id)
                            .send()
                            .await
                            .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Activity'"))?
                            .into_inner())
                    }
                })
                .await?,
            )],
        };

        Ok(records)
    }

    /// # get_records_by_date
    /// Keeping this as a separate method from [`Resource::get_records_by_id`] avoids
    /// adding an optional date parameter, such as `None`, to every normal ID-based
    /// hydration call.
    ///
    /// The supplied `(start, end)` [`DateTime<Utc>`] tuple is formatted as
    /// second-precision RFC3339 strings using [`SecondsFormat::Secs`] and `Z` for
    /// UTC, then passed to simPRO using its `between(...)` search operator:
    /// <https://developer.simprogroup.com/apidoc/?page=ff7c0fcd6a31e735a61c001f75426961#tag/Get-resource>
    ///
    /// Example generated filter:
    /// `Date=between(2026-04-14T09:32:15Z,2026-05-14T09:32:15Z)`
    ///
    /// Using RFC3339 instead of `yyyy/mm/dd` avoids missing records created
    /// after 12AM on the (yyyy/mm/dd) computer date.
    #[tracing::instrument(skip(self, dates_between, app))]
    pub(crate) async fn get_records_by_date(
        &self,
        dates_between: (DateTime<Utc>, DateTime<Utc>),
        app: Arc<AppState>,
    ) -> anyhow::Result<Vec<Records>> {
        use crate::api::Columns;

        let (start, end) = dates_between.to_rfc3339(SecondsFormat::Secs, true);
        let dates_between = format!("between({},{})", start, end);

        let records = match self {
            Resource::Schedule => {
                let schedules: Vec<api::Schedule> = paginate(|page| {
                    let app = app.clone();
                    let dates_between = dates_between.clone();
                    async move {
                        Ok(app
                            .api
                            .get_schedules()
                            .date(dates_between)
                            .page(page)
                            .page_size(PAGE_SIZE)
                            .columns(api::Schedule::COLUMNS.join(","))
                            .company_id(&app.company_id)
                            .send()
                            .await
                            .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Schedule'"))?
                            .into_inner())
                    }
                })
                .await?;

                let mut bin = IDs::default();

                for schedule in &schedules {
                    schedule.parse_id_reference(&mut bin)?;
                }

                let mut records = vec![];

                for (ids, resource) in bin.resources() {
                    if ids.is_empty() {
                        continue;
                    }

                    records.extend(Box::pin(resource.get_records_by_id(ids, app.clone())).await?);
                }

                records.push(Records::Schedule(schedules));
                records
            }

            resource => {
                anyhow::bail!("get_records_by_date is not implemented for {resource:?}");
            }
        };

        Ok(records)
    }
}

/// Returns a `(start, end)` [`DateTime<Utc>`] tuple as a time range.
pub(crate) fn time_range(
    base: DateTime<Utc>,
    sub_months: u32,
    add_months: u32,
) -> (DateTime<Utc>, DateTime<Utc>) {
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
    use crate::records::get_records::Records;
    use crate::webhook::variants::Resource;

    let (add_months, sub_months) = (3, 3);
    let records_batches: Vec<Records> = Resource::Schedule
        .get_records_by_date(time_range(Utc::now(), add_months, sub_months), app.clone())
        .await?;

    let pool = app.db_connection_pool.clone();
    let mut conn = pool.get().await?;
    for records in records_batches {
        records
            .resource()
            .upsert_records(records, &mut conn)
            .await?;
    }

    Ok(())
}
