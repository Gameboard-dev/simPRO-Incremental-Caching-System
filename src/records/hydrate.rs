use std::sync::Arc;

use crate::{AppState, api::types as api, parse::reference::IDs, webhook::variants::Resource};

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
    pub(crate) async fn get_records(&self, ids: &[u64], app: Arc<AppState>) -> anyhow::Result<Records> {
        use crate::api::Columns;

        let id_search = format!(
            "ID=in({})",
            ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",")
        );

        let records = match self {
            Resource::Schedule => {
                let records: Vec<api::Schedule> = app
                    .api
                    .get_schedules()
                    .search(id_search)
                    .columns(api::Schedule::COLUMNS.join(","))
                    .company_id(&app.company_id)
                    .send()
                    .await
                    .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Schedule'"))?
                    .into_inner();

                let mut bin = IDs::default();
                for record in &records {
                    record.parse_id_reference(&mut bin)?;
                }

                for (ids, resource) in bin.resources() {
                    if ids.is_empty() {
                        continue;
                    }
                    Box::pin(resource.get_records(ids, app.clone())).await?;
                }

                Records::Schedule(records)
            }

            Resource::CostCenter => Records::CostCenter(
                app.api
                    .get_cost_centers()
                    .search(id_search)
                    .columns(api::CostCenter::COLUMNS.join(","))
                    .company_id(&app.company_id)
                    .send()
                    .await
                    .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Cost Center'"))?
                    .into_inner(),
            ),

            Resource::Quote => Records::Quote(
                app.api
                    .get_quotes()
                    .search(id_search)
                    .columns(api::Quote::COLUMNS.join(","))
                    .company_id(&app.company_id)
                    .send()
                    .await
                    .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Quote'"))?
                    .into_inner(),
            ),

            Resource::Lead => Records::Lead(
                app.api
                    .get_leads()
                    .search(id_search)
                    .columns(api::Lead::COLUMNS.join(","))
                    .company_id(&app.company_id)
                    .send()
                    .await
                    .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Lead'"))?
                    .into_inner(),
            ),

            Resource::Job => Records::Job(
                app.api
                    .get_jobs()
                    .search(id_search)
                    .columns(api::Job::COLUMNS.join(","))
                    .company_id(&app.company_id)
                    .send()
                    .await
                    .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Job'"))?
                    .into_inner(),
            ),

            Resource::Site => Records::Site(
                app.api
                    .get_sites()
                    .search(id_search)
                    .columns(api::Site::COLUMNS.join(","))
                    .company_id(&app.company_id)
                    .send()
                    .await
                    .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Site'"))?
                    .into_inner(),
            ),

            Resource::Employee => Records::Employee(
                app.api
                    .get_employees()
                    .search(id_search)
                    .columns(api::Employee::COLUMNS.join(","))
                    .company_id(&app.company_id)
                    .send()
                    .await
                    .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Employee'"))?
                    .into_inner(),
            ),

            Resource::Activity => Records::Activity(
                app.api
                    .get_activities()
                    .search(id_search)
                    .columns(api::Activity::COLUMNS.join(","))
                    .company_id(&app.company_id)
                    .send()
                    .await
                    .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Activity'"))?
                    .into_inner(),
            ),
        };



        return Ok(records);
    }
}
