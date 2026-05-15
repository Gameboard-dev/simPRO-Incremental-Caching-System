//! This module is responsible for getting the corresponding simPRO records for batched IDs
//! and upserting them into the database

use crate::ApiClient;
use crate::AppState;
use crate::api::types as api;
use crate::db;
use crate::db::insertables;
use crate::parse::into_rows::prepare_schedule_rows;
use crate::parse::schedule::reference::ScheduleReference;
use crate::records::get_records::Records;
use crate::webhook::variants::Resource;
use diesel::ExpressionMethods;
use diesel::sql_query;
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

                let rows = prepare_schedule_rows(&records)?;

                sql_query("BEGIN")
                    .execute(&mut *connection)
                    .await?;

                let result: anyhow::Result<()> = async {
                    insert_schedules(
                        &rows.schedules,
                        connection,
                    )
                    .await?;
                    associate_job_schedules(
                        &rows.job_schedules,
                        connection,
                    )
                    .await?;
                    associate_lead_schedules(
                        &rows.lead_schedules,
                        connection,
                    )
                    .await?;
                    associate_quote_schedules(
                        &rows.quote_schedules,
                        connection,
                    )
                    .await?;
                    associate_activity_schedules(
                        &rows.activity_schedules,
                        connection,
                    )
                    .await?;
                    upsert_schedule_blocks(
                        &rows.schedule_blocks,
                        connection,
                    )
                    .await?;

                    Ok(())
                }
                .await;

                match result {
                    Ok(()) => {
                        sql_query("COMMIT")
                            .execute(&mut *connection)
                            .await?;
                    }
                    Err(err) => {
                        let _ = sql_query("ROLLBACK")
                            .execute(&mut *connection)
                            .await;
                        return Err(err);
                    }
                }
            }

            Records::Job(records) => {
                let rows = prepare_job_rows(&records)?;

                sql_query("BEGIN")
                    .execute(&mut *connection)
                    .await?;

                // ------------------------------------------------------------------------------------------------
                // Lightweight nested metadata in `Job` contains the small subset of fields required
                // by the local schema, so issuing additional API requests for full Job Status or Customer
                // resources would add unnecessary network overhead without providing additional persistence value.
                //
                // Job-related tables are written inside a single atomic (all or nothing) SQL transaction.
                // ------------------------------------------------------------------------------------------------
                let result: anyhow::Result<()> = async {
                    insert_job_statuses(
                        &rows.job_statuses,
                        connection,
                    )
                    .await?;
                    insert_company_customers(
                        &rows.company_customers,
                        connection,
                    )
                    .await?;
                    insert_jobs(&rows.jobs, connection)
                        .await?;

                    Ok(())
                }
                .await;

                match result {
                    Ok(()) => {
                        sql_query("COMMIT")
                            .execute(&mut *connection)
                            .await?;
                    }
                    Err(err) => {
                        let _ = sql_query("ROLLBACK")
                            .execute(&mut *connection)
                            .await;
                        return Err(err);
                    }
                }
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

async fn insert_schedules(
    rows: &[insertables::NewSchedule<'_>],
    connection: &mut Object<AsyncPgConnection>,
) -> anyhow::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    use crate::db::table::schedules::dsl::*;

    diesel::insert_into(schedules)
        .values(rows)
        .on_conflict(id)
        .do_update()
        .set(to_update!(
            date_modified,
            staff_id,
            schedule_type,
            notes
        ))
        .execute(&mut *connection)
        .await?;

    Ok(())
}

async fn associate_job_schedules(
    rows: &[insertables::NewJobSchedule],
    connection: &mut Object<AsyncPgConnection>,
) -> anyhow::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    use crate::db::table::job_schedules::dsl::*;

    diesel::insert_into(job_schedules)
        .values(rows)
        .on_conflict((schedule_id, job_id, cost_center_id))
        .do_nothing()
        .execute(&mut *connection)
        .await?;

    Ok(())
}

async fn associate_lead_schedules(
    rows: &[insertables::NewLeadSchedule],
    connection: &mut Object<AsyncPgConnection>,
) -> anyhow::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    use crate::db::table::lead_schedules::dsl::*;

    diesel::insert_into(lead_schedules)
        .values(rows)
        .on_conflict((schedule_id, lead_id))
        .do_nothing()
        .execute(&mut *connection)
        .await?;

    Ok(())
}

async fn associate_quote_schedules(
    rows: &[insertables::NewQuoteSchedule],
    connection: &mut Object<AsyncPgConnection>,
) -> anyhow::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    use crate::db::table::quote_schedules::dsl::*;

    diesel::insert_into(quote_schedules)
        .values(rows)
        .on_conflict((schedule_id, quote_id))
        .do_update()
        .set(
            cost_center_id.eq(diesel::upsert::excluded(
                cost_center_id,
            )),
        )
        .execute(&mut *connection)
        .await?;

    Ok(())
}

async fn associate_activity_schedules(
    rows: &[insertables::NewActivitySchedule],
    connection: &mut Object<AsyncPgConnection>,
) -> anyhow::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    use crate::db::table::activity_schedules::dsl::*;

    diesel::insert_into(activity_schedules)
        .values(rows)
        .on_conflict((schedule_id, activity_id))
        .do_nothing()
        .execute(&mut *connection)
        .await?;

    Ok(())
}

async fn upsert_schedule_blocks(
    rows: &[insertables::NewScheduleBlock],
    connection: &mut Object<AsyncPgConnection>,
) -> anyhow::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    use crate::db::table::schedule_blocks::dsl::*;

    diesel::insert_into(schedule_blocks)
        .values(rows)
        .on_conflict((
            schedule_id,
            iso8601_start_time,
            iso8601_end_time,
        ))
        .do_update()
        .set(to_update!(schedule_rate))
        .execute(&mut *connection)
        .await?;

    Ok(())
}

struct JobRows<'a> {
    job_statuses: Vec<insertables::NewJobStatuse<'a>>,
    company_customers:
        Vec<insertables::NewCompanyCustomer<'a>>,
    jobs: Vec<insertables::NewJob<'a>>,
}

fn prepare_job_rows(
    records: &[api::Job],
) -> anyhow::Result<JobRows<'_>> {

    let mut statuses = records
        .iter()
        .map(|job| {
            insertables::NewJobStatuse::try_from(
                &job.status,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    // ------------------------------------------------------------------------------------
    // The simPRO API returns unique records but this will return duplicate rows
    // for multiple jobs with the same nested reference object (e.g. `Job.Customer`).
    //
    // Deduplication is achieved once sorted with [`Vec::sort_by_key`]
    // because [`Vec::dedup_by_key`] only removes consecutive duplicates.
    // ------------------------------------------------------------------------------------
    statuses.sort_by_key(|row| row.id);
    statuses.dedup_by_key(|row| row.id);

    let mut customers = records
        .iter()
        .map(insertables::NewCompanyCustomer::try_from)
        .collect::<Result<Vec<_>, _>>()?;

    customers.sort_by_key(|row| row.id);
    customers.dedup_by_key(|row| row.id);

    let jobs = records
        .iter()
        .map(insertables::NewJob::try_from)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(JobRows {
        job_statuses: statuses,
        company_customers: customers,
        jobs,
    })
}

async fn insert_job_statuses(
    rows: &[insertables::NewJobStatuse<'_>],
    connection: &mut Object<AsyncPgConnection>,
) -> anyhow::Result<()> {

    if rows.is_empty() {
        return Ok(());
    }

    use crate::db::table::job_statuses::dsl::*;

    diesel::insert_into(job_statuses)
        .values(rows)
        .on_conflict(id)
        .do_update()
        .set(to_update!(name, color))
        .execute(&mut *connection)
        .await?;

    Ok(())
}

async fn insert_company_customers(
    rows: &[insertables::NewCompanyCustomer<'_>],
    connection: &mut Object<AsyncPgConnection>,
) -> anyhow::Result<()> {

    if rows.is_empty() {
        return Ok(());
    }

    use crate::db::table::company_customers::dsl::*;

    diesel::insert_into(company_customers)
        .values(rows)
        .on_conflict(id)
        .do_update()
        .set(
            company_name
                .eq(diesel::upsert::excluded(company_name)),
        )
        .execute(&mut *connection)
        .await?;

    Ok(())
}

async fn insert_jobs(
    rows: &[insertables::NewJob<'_>],
    connection: &mut Object<AsyncPgConnection>,
) -> anyhow::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    use crate::db::table::jobs::dsl::*;

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
        .execute(&mut *connection)
        .await?;

    Ok(())
}
