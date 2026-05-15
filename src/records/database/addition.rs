//! This module is responsible for getting the corresponding simPRO records for batched IDs
//! and upserting them into the database

use crate::{
    ApiClient, AppState,
    api::types as api,
    db::{self, insertables},
    parse::schedule::reference::ScheduleReference,
    records::{
        api::{retrieval::Records, into_rows::prepare_schedule_rows},
        database::r#macro::{in_transaction, insert_rows, upsert_api_records},
    },
    webhook::variants::Resource,
};
use diesel::ExpressionMethods;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl, pooled_connection::deadpool::Object};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::fs::read_link;

type DbConnection = Object<AsyncPgConnection>;

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
    pub(crate) async fn upsert_records(&self, records: Records, connection: &mut DbConnection) -> anyhow::Result<()> {
        match records {
            Records::Schedule(records) => {
                let rows = prepare_schedule_rows(&records)?;
                in_transaction!(connection, {
                    upsert_schedules(&rows.schedules, connection).await?;
                    associate_job_schedules(&rows.job_schedules, connection).await?;
                    associate_lead_schedules(&rows.lead_schedules, connection).await?;
                    associate_quote_schedules(&rows.quote_schedules, connection).await?;
                    associate_activity_schedules(&rows.activity_schedules, connection).await?;
                    upsert_schedule_blocks(&rows.schedule_blocks, connection).await?;
                    Ok(())
                });
            }

            Records::Job(records) => {
                let rows = prepare_job_rows(&records)?;
                // ----------------------------------------------------------------------------------------------------
                // The subset of fields required for Job Status and Job Customer is included `Job`, so issuing additional API requests
                // for full resources in this case would add unnecessary network overhead without any additional value.
                // ----------------------------------------------------------------------------------------------------
                in_transaction!(connection, {
                    insert_job_statuses(&rows.job_statuses, connection).await?;
                    insert_company_customers(&rows.company_customers, connection).await?;
                    insert_jobs(&rows.jobs, connection).await?;
                    Ok(())
                });
            }
            Records::Site(records) => {
                upsert_api_records!(records, connection, sites::sites, insertables::NewSite, id, [address_address, address_city, address_country, address_postal_code, date_modified]);
            }

            Records::Employee(records) => {
                upsert_api_records!(records, connection, employees::employees, insertables::NewEmployee, id, [id, name]);
            }

            Records::Activity(records) => {
                upsert_api_records!(records, connection, activities::activities, insertables::NewActivity, id, [id, name]);
            }

            Records::CostCenter(records) => {
                upsert_api_records!(records, connection, cost_centers::cost_centers, insertables::NewCostCenter, id, [id, name]);
            }

            Records::Quote(records) => {
                upsert_api_records!(records, connection, quotes::quotes, insertables::NewQuote, id, [name, id]);
            }

            Records::Lead(records) => {
                upsert_api_records!(records, connection, leads::leads, insertables::NewLead, id, [name, id]);
            }
        }

        Ok(())
    }
}

async fn upsert_schedules(rows: &[insertables::NewSchedule<'_>], connection: &mut DbConnection) -> anyhow::Result<()> {
    insert_rows!(rows, connection, schedules::schedules, id, do_update[date_modified, staff_id, schedule_type, notes]);
    Ok(())
}

async fn associate_job_schedules(rows: &[insertables::NewJobSchedule], connection: &mut DbConnection) -> anyhow::Result<()> {
    insert_rows!(rows, connection, job_schedules::job_schedules, (schedule_id, job_id, cost_center_id), do_nothing);
    Ok(())
}

async fn associate_lead_schedules(rows: &[insertables::NewLeadSchedule], connection: &mut DbConnection) -> anyhow::Result<()> {
    insert_rows!(rows, connection, lead_schedules::lead_schedules, (schedule_id, lead_id), do_nothing);
    Ok(())
}

async fn associate_quote_schedules(rows: &[insertables::NewQuoteSchedule], connection: &mut DbConnection) -> anyhow::Result<()> {
    insert_rows!(rows, connection, quote_schedules::quote_schedules, (schedule_id, quote_id), do_update[cost_center_id]);
    Ok(())
}

async fn associate_activity_schedules(rows: &[insertables::NewActivitySchedule], connection: &mut DbConnection) -> anyhow::Result<()> {
    insert_rows!(rows, connection, activity_schedules::activity_schedules, (schedule_id, activity_id), do_nothing);
    Ok(())
}

async fn upsert_schedule_blocks(rows: &[insertables::NewScheduleBlock], connection: &mut DbConnection) -> anyhow::Result<()> {
    insert_rows!(rows, connection, schedule_blocks::schedule_blocks, (schedule_id, iso8601_start_time, iso8601_end_time), do_update[schedule_rate]);
    Ok(())
}

struct JobRows<'a> {
    job_statuses: Vec<insertables::NewJobStatuse<'a>>,
    company_customers: Vec<insertables::NewCompanyCustomer<'a>>,
    jobs: Vec<insertables::NewJob<'a>>,
}

/// ------------------------------------------------------------------------------------
/// The simPRO API returns unique records. This deduplicates flattened nested objects
/// such as `Job.job_status` for insertion into their own tables (e.g. [`crate::db::table::job_statuses`])
/// Deduplication is achieved after sort with [`Vec::sort_by_key`]
/// because [`Vec::dedup_by_key`] removes only consecutive duplicates.
/// ------------------------------------------------------------------------------------
fn sort_and_dedup_rows<T, K>(rows: &mut Vec<T>, key: fn(&T) -> K)
where
    K: Ord,
{
    rows.sort_by_key(key);
    rows.dedup_by_key(|row| key(row));
}

fn prepare_job_rows(records: &[api::Job]) -> anyhow::Result<JobRows<'_>> {
    
    let mut job_statuses = records.iter().map(|job| insertables::NewJobStatuse::try_from(&job.status)).collect::<Result<Vec<_>, _>>()?;
    sort_and_dedup_rows(&mut job_statuses, |row| row.id);

    let mut job_company_customers = records.iter().map(insertables::NewCompanyCustomer::try_from).collect::<Result<Vec<_>, _>>()?;
    sort_and_dedup_rows(&mut job_company_customers, |row| row.id);

    let jobs = records.iter().map(insertables::NewJob::try_from).collect::<Result<Vec<_>, _>>()?;

    Ok(JobRows { job_statuses, company_customers: job_company_customers, jobs })
}

async fn insert_job_statuses(rows: &[insertables::NewJobStatuse<'_>], connection: &mut DbConnection) -> anyhow::Result<()> {
    insert_rows!(rows, connection, job_statuses::job_statuses, id, do_update[name, color]);
    Ok(())
}

async fn insert_company_customers(rows: &[insertables::NewCompanyCustomer<'_>], connection: &mut DbConnection) -> anyhow::Result<()> {
    insert_rows!(rows, connection, company_customers::company_customers, id, do_update[company_name]);
    Ok(())
}

async fn insert_jobs(rows: &[insertables::NewJob<'_>], connection: &mut DbConnection) -> anyhow::Result<()> {
    insert_rows!(rows, connection, jobs::jobs, id, do_update[
        name, customer_id, date_modified, description, site_id, stage, status_id, job_type
    ]);
    Ok(())
}
