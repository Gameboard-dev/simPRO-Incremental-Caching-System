use crate::{AppState, api::types as api, parse::reference::IDs, webhook::variants::Resource};
use std::sync::Arc;

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

impl Resource {
    /// * [Activity](https://developer.simprogroup.com/apidoc/?page=d78ed35383108fb6c04c16d0a11b20fe#tag/Activities/operation/c88605b27f7e8a3873047d9af3a93574)
    /// * [Site](https://developer.simprogroup.com/apidoc/?page=3faa64303d5f5bcd043bb88f6768e603#tag/Sites/operation/101d05972386dfa7536b58fe655d382e)
    /// * [Job](https://developer.simprogroup.com/apidoc/?page=12ceff2290bb9039beaa8f36d5dec226#tag/Jobs/operation/9ca8d728df9f031d2828e79cbb093702)
    /// * [Employee](https://developer.simprogroup.com/apidoc/?page=eb626c94531ec554f93b2b78a77c8b1b#tag/Employees/operation/ad2cdcfe3653fce4e460e4468acc2867)
    /// * [Schedule](https://developer.simprogroup.com/apidoc/?page=ccdb7bf9d93e5652b57cabcc8c41e061#tag/Schedules/operation/4a005958478750b0f96cb00b3c9da0f6)
    #[allow(unused)]
    #[tracing::instrument(skip(self, ids, app))]
    pub(crate) async fn get_records(
        &self,
        ids: &[i64],
        app: Arc<AppState>,
    ) -> anyhow::Result<Vec<Records>> {
        use crate::api::Columns;

        let ids: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        let ids = format!("in({})", ids.join(","));

        let records = match self {
            Resource::Schedule => {
                let schedules: Vec<api::Schedule> = app
                    .api
                    .get_schedules()
                    .id(ids)
                    .columns(api::Schedule::COLUMNS.join(","))
                    .company_id(&app.company_id)
                    .send()
                    .await
                    .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Schedule'"))?
                    .into_inner();

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
                    records.extend(Box::pin(resource.get_records(ids, app.clone())).await?);
                }

                // --------------------------------------------------------------------------------------
                // Record arrays are returned in database dependency order
                records.push(Records::Schedule(schedules));
                records
            }

            Resource::Job => {
                let jobs: Vec<api::Job> = app
                    .api
                    .get_jobs()
                    .id(ids)
                    .columns(api::Job::COLUMNS.join(","))
                    .company_id(&app.company_id)
                    .send()
                    .await
                    .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Job'"))?
                    .into_inner();
                // --------------------------------------------------------------------------------------
                let site_ids: Vec<i64> = jobs.iter().map(|job| job.site.id).collect();
                // --------------------------------------------------------------------------------------
                let mut sites: Records =
                    Box::pin(Resource::Site.get_records(&site_ids, app.clone()))
                        .await?
                        .pop()
                        .ok_or_else(|| anyhow::anyhow!("Expected site records"))?;
                // --------------------------------------------------------------------------------------
                // Record arrays are returned in database dependency order
                vec![sites, Records::Job(jobs)]
            }

            Resource::CostCenter => vec![Records::CostCenter(
                app.api
                    .get_cost_centers()
                    .id(ids)
                    .columns(api::CostCenter::COLUMNS.join(","))
                    .company_id(&app.company_id)
                    .send()
                    .await
                    .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Cost Center'"))?
                    .into_inner(),
            )],

            Resource::Quote => vec![Records::Quote(
                app.api
                    .get_quotes()
                    .id(ids)
                    .columns(api::Quote::COLUMNS.join(","))
                    .company_id(&app.company_id)
                    .send()
                    .await
                    .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Quote'"))?
                    .into_inner(),
            )],

            Resource::Lead => vec![Records::Lead(
                app.api
                    .get_leads()
                    .id(ids)
                    .columns(api::Lead::COLUMNS.join(","))
                    .company_id(&app.company_id)
                    .send()
                    .await
                    .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Lead'"))?
                    .into_inner(),
            )],

            Resource::Site => vec![Records::Site(
                app.api
                    .get_sites()
                    .id(ids)
                    .columns(api::Site::COLUMNS.join(","))
                    .company_id(&app.company_id)
                    .send()
                    .await
                    .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Site'"))?
                    .into_inner(),
            )],

            Resource::Employee => vec![Records::Employee(
                app.api
                    .get_employees()
                    .id(ids)
                    .columns(api::Employee::COLUMNS.join(","))
                    .company_id(&app.company_id)
                    .send()
                    .await
                    .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Employee'"))?
                    .into_inner(),
            )],

            Resource::Activity => vec![Records::Activity(
                app.api
                    .get_activities()
                    .id(ids)
                    .columns(api::Activity::COLUMNS.join(","))
                    .company_id(&app.company_id)
                    .send()
                    .await
                    .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Activity'"))?
                    .into_inner(),
            )],
        };

        Ok(records)
    }
}
