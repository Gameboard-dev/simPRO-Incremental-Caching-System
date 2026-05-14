//! This module is responsible for getting the corresponding simPRO records for batched IDs
//! and upserting them into the database

use crate::ApiClient;
use crate::AppState;
use crate::api::types as api;
use crate::db;
use crate::db::insertables;
use crate::records::get_records::Records;
use crate::webhook::variants::Resource;
use diesel::ExpressionMethods;
use diesel_async::AsyncConnection;
use diesel_async::AsyncPgConnection;
use diesel_async::RunQueryDsl;
use diesel_async::pooled_connection::deadpool::Object;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::fs::read_link;

/// A macro expression that generates Diesel `ON CONFLICT DO UPDATE` assignments
/// equivalent to SQL: `UPDATE SET column = EXCLUDED.column` which matches one or more
/// comma-separated column identifiers $($col:ident),+ with an optional trailing comma $(,)?
macro_rules! to_update {($($col:ident),+ $(,)?) => {($( $col.eq(diesel::upsert::excluded($col)) ),+)};}

impl Resource {
    /// This function converts hydrated API models into Diesel insertable structs
    /// and performs batched `INSERT ... ON CONFLICT DO UPDATE` operations using [`diesel_async`]
    /// with [`AsyncPgConnection`] so PostgreSQL queries execute without blocking Tokio worker threads.
    ///
    /// All upserts are idempotent through PostgreSQL `ON CONFLICT DO UPDATE`,
    /// allowing repeated synchronization attempts for the same records.
    ///
    /// * [Activity](https://developer.simprogroup.com/apidoc/?page=d78ed35383108fb6c04c16d0a11b20fe#tag/Activities/operation/c88605b27f7e8a3873047d9af3a93574)
    /// * [Site](https://developer.simprogroup.com/apidoc/?page=3faa64303d5f5bcd043bb88f6768e603#tag/Sites/operation/101d05972386dfa7536b58fe655d382e)
    /// * [Job](https://developer.simprogroup.com/apidoc/?page=12ceff2290bb9039beaa8f36d5dec226#tag/Jobs/operation/9ca8d728df9f031d2828e79cbb093702)
    /// * [Employee](https://developer.simprogroup.com/apidoc/?page=eb626c94531ec554f93b2b78a77c8b1b#tag/Employees/operation/ad2cdcfe3653fce4e460e4468acc2867)
    /// * [Schedule](https://developer.simprogroup.com/apidoc/?page=ccdb7bf9d93e5652b57cabcc8c41e061#tag/Schedules/operation/4a005958478750b0f96cb00b3c9da0f6)
    #[allow(unused)]
    #[tracing::instrument(skip(self, records, connection))]
    pub(crate) async fn upsert_records(
        &self,
        records: Records,
        connection: &mut Object<AsyncPgConnection>,
    ) -> anyhow::Result<()> {
        match records {
            Records::Schedule(records) => {
                use crate::db::table::schedules::dsl::*;

                let rows: Vec<insertables::NewSchedule<'_>> = records
                    .iter()
                    .map(insertables::NewSchedule::try_from)
                    .collect::<Result<Vec<_>, _>>()?;

                diesel::insert_into(schedules)
                    .values(rows)
                    .on_conflict(id)
                    .do_update()
                    .set(to_update!(date_modified, staff_id, schedule_type, notes))
                    .execute(connection)
                    .await?;
            }
            Records::Job(records) => {
                let connection: &mut AsyncPgConnection = &mut **connection;
                // ------------------------------------------------------------------------------------------------
                // Lightweight nested metadata in `Job` contains the small subset of fields required 
                // by the local schema, so issuing additional API requests for full Job Status or Customer 
                // resources would add unnecessary network overhead without providing additional persistence value.
                //
                // Job-related tables are then written inside a single atomic (all or nothing) SQL transaction.
                // ------------------------------------------------------------------------------------------------
                connection
                    .transaction::<_, anyhow::Error, _>(async move |conn| {
                        // -----------------------------> JOB STATUSES
                        {
                            use crate::db::table::job_statuses::dsl::*;
                            // ------------------------------------------------------------------------------------
                            let mut rows = records
                                .iter()
                                .map(|job| insertables::NewJobStatuse::try_from(&job.status))
                                .collect::<Result<Vec<_>, _>>()?;
                            // ------------------------------------------------------------------------------------
                            // The simPRO API returns unique records but this will return duplicate rows
                            // for multiple jobs with the same nested reference object (e.g. `Job.Customer`).
                            //
                            // Deduplication is done once sorted with [`Vec::sort_by_key`]
                            // as [`Vec::dedup_by_key`] only removes consecutive duplicates.
                            // ------------------------------------------------------------------------------------
                            rows.sort_by_key(|row| row.id);
                            rows.dedup_by_key(|row| row.id);
                            // ------------------------------------------------------------------------------------
                            diesel::insert_into(job_statuses)
                                .values(rows)
                                .on_conflict(id)
                                .do_update()
                                .set(to_update!(name, color))
                                .execute(&mut *conn)
                                .await?;
                        }
                        // -----------------------------> JOB CUSTOMERS
                        {
                            use crate::db::table::company_customers::dsl::*;
                            // ------------------------------------------------------------------------------------
                            let mut rows = records
                                .iter()
                                .map(insertables::NewCompanyCustomer::try_from)
                                .collect::<Result<Vec<_>, _>>()?;
                            // ------------------------------------------------------------------------------------
                            rows.sort_by_key(|row| row.id);
                            rows.dedup_by_key(|row| row.id);
                            // ------------------------------------------------------------------------------------
                            diesel::insert_into(company_customers)
                                .values(rows)
                                .on_conflict(id)
                                .do_update()
                                .set(company_name.eq(diesel::upsert::excluded(company_name)))
                                .execute(&mut *conn)
                                .await?;
                        }
                        // -----------------------------> JOBS
                        {
                            use crate::db::table::jobs::dsl::*;
                            // ------------------------------------------------------------------------------------
                            let rows = records
                                .iter()
                                .map(insertables::NewJob::try_from)
                                .collect::<Result<Vec<_>, _>>()?;
                            // ------------------------------------------------------------------------------------
                            diesel::insert_into(jobs)
                                .values(rows)
                                .on_conflict(id)
                                .do_update()
                                .set(to_update!(
                                    name,
                                    customer_id,
                                    date_modified,
                                    description,
                                    site_id,
                                    stage,
                                    status_id,
                                    job_type
                                ))
                                .execute(&mut *conn)
                                .await?;
                        }

                        Ok(())
                    })
                    .await?;
            }
            Records::Site(records) => {
                use crate::db::table::sites::dsl::*;

                diesel::insert_into(sites)
                    .values(
                        records
                            .iter()
                            .map(insertables::NewSite::try_from)
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                    .on_conflict(id)
                    .do_update()
                    .set(to_update!(
                        address_address,
                        address_city,
                        address_country,
                        address_postal_code,
                        date_modified
                    ))
                    .execute(connection)
                    .await?;
            }

            Records::Employee(records) => {
                use crate::db::table::employees::dsl::*;

                diesel::insert_into(employees)
                    .values(
                        records
                            .iter()
                            .map(insertables::NewEmployee::try_from)
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                    .on_conflict(id)
                    .do_update()
                    .set(to_update!(id, name))
                    .execute(connection)
                    .await?;
            }

            Records::Activity(records) => {
                use crate::db::table::activities::dsl::*;

                diesel::insert_into(activities)
                    .values(
                        records
                            .iter()
                            .map(insertables::NewActivity::try_from)
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                    .on_conflict(id)
                    .do_update()
                    .set(to_update!(id, name))
                    .execute(connection)
                    .await?;
            }

            Records::CostCenter(records) => {
                use crate::db::table::cost_centers::dsl::*;

                diesel::insert_into(cost_centers)
                    .values(
                        records
                            .iter()
                            .map(insertables::NewCostCenter::try_from)
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                    .on_conflict(id)
                    .do_update()
                    .set(to_update!(id, name))
                    .execute(connection)
                    .await?;
            }

            Records::Quote(records) => {
                use crate::db::table::quotes::dsl::*;

                diesel::insert_into(quotes)
                    .values(
                        records
                            .iter()
                            .map(insertables::NewQuote::try_from)
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                    .on_conflict(id)
                    .do_update()
                    .set(to_update!(name, id))
                    .execute(connection)
                    .await?;
            }

            Records::Lead(records) => {
                use crate::db::table::leads::dsl::*;

                diesel::insert_into(leads)
                    .values(
                        records
                            .iter()
                            .map(insertables::NewLead::try_from)
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                    .on_conflict(id)
                    .do_update()
                    .set(to_update!(name, id))
                    .execute(connection)
                    .await?;
            }
        }

        Ok(())
    }
}
