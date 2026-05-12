//! This module is responsible for getting the corresponding simPRO records for batched IDs
//! and upserting them into the database

use super::variants::Resource;
use crate::ApiClient;
use crate::AppState;
use crate::api::Columns;
use crate::api::types::Schedule;
use crate::bin::BinOfIDs;
use crate::db;
use crate::r#macro::update;
use diesel::ExpressionMethods;
use diesel::RunQueryDsl;
use diesel::pg::PgConnection;
use diesel::r2d2::ConnectionManager;
use r2d2::PooledConnection;
use std::collections::HashSet;
use std::sync::Arc;

impl Resource {
    /// * [Activity](https://developer.simprogroup.com/apidoc/?page=d78ed35383108fb6c04c16d0a11b20fe#tag/Activities/operation/c88605b27f7e8a3873047d9af3a93574)
    /// * [Site](https://developer.simprogroup.com/apidoc/?page=3faa64303d5f5bcd043bb88f6768e603#tag/Sites/operation/101d05972386dfa7536b58fe655d382e)
    /// * [Job](https://developer.simprogroup.com/apidoc/?page=12ceff2290bb9039beaa8f36d5dec226#tag/Jobs/operation/9ca8d728df9f031d2828e79cbb093702)
    /// * [Employee](https://developer.simprogroup.com/apidoc/?page=eb626c94531ec554f93b2b78a77c8b1b#tag/Employees/operation/ad2cdcfe3653fce4e460e4468acc2867)
    /// * [Schedule](https://developer.simprogroup.com/apidoc/?page=ccdb7bf9d93e5652b57cabcc8c41e061#tag/Schedules/operation/4a005958478750b0f96cb00b3c9da0f6)
    #[allow(unused)]
    #[tracing::instrument(skip(self, ids))]
    pub async fn getter(&self, ids: &[i64], app: Arc<AppState>) -> anyhow::Result<()> {
        let id_search: String = format!(
            "ID=in({})",
            ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",")
        );

        let mut connection: PooledConnection<ConnectionManager<PgConnection>> = app.db_connection_pool.get()?;

        match self {
            Resource::Schedule => {
                // -----------------------------------------------------
                use crate::api::types::Schedule;
                use crate::db::{insertables, table::schedules::dsl::*};
                // -----------------------------------------------------
                let records: Vec<Schedule> = app
                    .api
                    .get_schedules()
                    .search(id_search)
                    // Request only the statically-generated API columns for `Schedule`.
                    // `COLUMNS` is generated at compile time from serde rename attributes
                    // in the OpenAPI-generated struct definition in build.rs
                    .columns(Schedule::COLUMNS.join(","))
                    .send()
                    .await
                    .map_err(|err| {
                        tracing::error!(?err, "Failed to fetch 'Schedule'");
                        err
                    })?
                    .into_inner();
                // ----------------------------------------------------------------------------------------
                let mut bin = BinOfIDs::default();
                // ----------------------------------------------------------------------------------------
                // Parse referenced foreign record IDs from the retrieved schedules
                // and accumulate them into a shared dependency bin.
                for record in &records {
                    record.parse_reference(&mut bin)?;
                }
                // -----------------------------------------------------
                // Recursively fetch and upsert referenced dependency records
                // before inserting records that contain foreign key references.
                for (ids, resource) in bin.resources() {
                    if ids.is_empty() {
                        continue;
                    }
                    // `resource.getter(...)` dynamically dispatches to the async retrieval
                    // function associated with the current `Resource` variant. The returned
                    // future is pinned because recursive async calls require a stable memory location
                    Box::pin(resource.getter(ids, app.clone())).await?;
                }
                // -----------------------------------------------------
                let rows: Vec<insertables::NewSchedule<'_>> = records
                    .iter()
                    .map(insertables::NewSchedule::try_from)
                    .collect::<Result<Vec<_>, _>>()?;
                // -----------------------------------------------------
                diesel::insert_into(schedules)
                    .values(rows)
                    .on_conflict(id)
                    .do_update()
                    .set(update!(date_modified, staff_id, schedule_type, notes))
                    .execute(&mut connection)?;
            }
            Resource::CostCenter => {
                // NO DEPENDENCIES
                use crate::api::types::CostCenter;
                use crate::db::insertables;
                use crate::db::table::cost_centers::dsl::*;
                let records: Vec<CostCenter> = app
                    .api
                    .get_cost_centers()
                    .search(id_search)
                    .columns(CostCenter::COLUMNS.join(","))
                    .send()
                    .await
                    .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Cost Center'"))?
                    .into_inner();
            }
            Resource::Quote => {
                // NO DEPENDENCIES
                use crate::api::types::Quote;
                use crate::db::insertables;
                use crate::db::table::quotes::dsl::*;
                let records: Vec<Quote> = app
                    .api
                    .get_quotes()
                    .search(id_search)
                    .columns(Quote::COLUMNS.join(","))
                    .send()
                    .await
                    .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Cost Center'"))?
                    .into_inner();
            }
            Resource::Lead => {
                // NO DEPENDENCIES
                use crate::api::types::Lead;
                use crate::db::insertables;
                use crate::db::table::quotes::dsl::*;
                let records: Vec<Lead> = app
                    .api
                    .get_leads()
                    .search(id_search)
                    .columns(Lead::COLUMNS.join(","))
                    .send()
                    .await
                    .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Cost Center'"))?
                    .into_inner();
            }
            Resource::Job => {
                use crate::api::types::Job;
                use crate::db::insertables;
                use crate::db::table::jobs::dsl::*;
                // ----------------------------------------------------------------------
                let records: Vec<Job> = app
                    .api
                    .get_jobs()
                    .search(id_search)
                    .columns(Job::COLUMNS.join(","))
                    .send()
                    .await
                    .inspect_err(|err| tracing::error!(?err, "Failed to fetch 'Schedule'"))?
                    .into_inner();
                // ----------------------------------------------------------------------
                // simPRO's OpenAPI specification defines IDs as numeric values
                let mut customer_ids = Vec::<i64>::new();
                // ----------------------------------------------------------------------
                for record in &records {
                    
                }

                // ----------------------------------------------------------------------
                diesel::insert_into(jobs)
                    .values(
                        records
                            .iter()
                            .map(insertables::NewJob::try_from)
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                    .on_conflict(id)
                    .do_update()
                    .set(update!(
                        name,
                        customer_company_name,
                        date_modified,
                        description,
                        site_id,
                        stage,
                        status_id,
                        job_type
                    ))
                    .execute(&mut connection)?;
            }
            Resource::Site => {
                use crate::api::types::Site;
                use crate::db::insertables;
                use crate::db::table::sites::dsl::*;
                // ----------------------------------------------------------------------
                let records: Vec<Site> = app
                    .api
                    .get_sites()
                    .search(id_search)
                    .columns(Site::COLUMNS.join(","))
                    .send()
                    .await
                    .map_err(|err| {
                        tracing::error!(?err, "Failed to fetch 'Schedule'");
                        err
                    })?
                    .into_inner();
                // ----------------------------------------------------------------------
                diesel::insert_into(sites)
                    .values(
                        records
                            .iter()
                            .map(insertables::NewSite::try_from)
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                    .on_conflict(id)
                    .do_update()
                    .set(update!(
                        address_address,
                        address_city,
                        address_country,
                        address_postal_code,
                        date_modified
                    ))
                    .execute(&mut connection)?;
            }
            Resource::Employee => {
                use crate::api::types::Employee;
                use crate::db::insertables;
                use crate::db::table::employees::dsl::*;
                // ----------------------------------------------------------------------
                let records: Vec<Employee> = app
                    .api
                    .get_employees()
                    .search(id_search)
                    .columns(Employee::COLUMNS.join(","))
                    .send()
                    .await
                    .map_err(|err| {
                        tracing::error!(?err, "Failed to fetch 'Employee'");
                        err
                    })?
                    .into_inner();
                // ----------------------------------------------------------------------
                diesel::insert_into(employees)
                    .values(
                        records
                            .iter()
                            .map(insertables::NewEmployee::try_from)
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                    .on_conflict(id)
                    .do_update()
                    .set(update!(id, name))
                    .execute(&mut connection)?;
            }
            Resource::Activity => {
                use crate::api::types::Activity;
                use crate::db::insertables;
                use crate::db::table::activities::dsl::*;
                // ----------------------------------------------------------------------
                let records: Vec<Activity> = app
                    .api
                    .get_activities()
                    .search(id_search)
                    .columns(Activity::COLUMNS.join(","))
                    .send()
                    .await
                    .map_err(|err| {
                        tracing::error!(?err, "Failed to fetch 'Activity'");
                        err
                    })?
                    .into_inner();
                // ----------------------------------------------------------------------
                diesel::insert_into(activities)
                    .values(
                        records
                            .iter()
                            .map(insertables::NewActivity::try_from)
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                    .on_conflict(id)
                    .do_update()
                    .set(update!(id, name))
                    .execute(&mut connection)?;
            }
        }

        Ok(())
    }
}
