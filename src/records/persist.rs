//! This module is responsible for getting the corresponding simPRO records for batched IDs
//! and upserting them into the database

use crate::ApiClient;
use crate::AppState;
use crate::api::types as api;
use crate::db;
use crate::db::insertables;
use crate::records::hydrate::Records;
use crate::webhook::variants::Resource;
use diesel::ExpressionMethods;
use diesel::RunQueryDsl;
use diesel::pg::PgConnection;
use diesel::r2d2::ConnectionManager;
use r2d2::PooledConnection;
use std::collections::HashSet;
use std::sync::Arc;

/// A macro expression that generates Diesel `ON CONFLICT DO UPDATE` assignments 
/// equivalent to SQL: `UPDATE SET column = EXCLUDED.column` which matches one or more 
/// comma-separated column identifiers $($col:ident),+ with an optional trailing comma $(,)?
macro_rules! to_update {($($col:ident),+ $(,)?) => {($( $col.eq(diesel::upsert::excluded($col)) ),+)};}

impl Resource {
    /// * [Activity](https://developer.simprogroup.com/apidoc/?page=d78ed35383108fb6c04c16d0a11b20fe#tag/Activities/operation/c88605b27f7e8a3873047d9af3a93574)
    /// * [Site](https://developer.simprogroup.com/apidoc/?page=3faa64303d5f5bcd043bb88f6768e603#tag/Sites/operation/101d05972386dfa7536b58fe655d382e)
    /// * [Job](https://developer.simprogroup.com/apidoc/?page=12ceff2290bb9039beaa8f36d5dec226#tag/Jobs/operation/9ca8d728df9f031d2828e79cbb093702)
    /// * [Employee](https://developer.simprogroup.com/apidoc/?page=eb626c94531ec554f93b2b78a77c8b1b#tag/Employees/operation/ad2cdcfe3653fce4e460e4468acc2867)
    /// * [Schedule](https://developer.simprogroup.com/apidoc/?page=ccdb7bf9d93e5652b57cabcc8c41e061#tag/Schedules/operation/4a005958478750b0f96cb00b3c9da0f6)
    #[allow(unused)]
    #[tracing::instrument(skip(self, records, connection))]
    pub(crate) fn upsert_records(
        &self,
        records: Records,
        connection: &mut PooledConnection<ConnectionManager<PgConnection>>,
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
                    .execute(connection)?;
            }
            Records::Job(records) => {
                {
                    use crate::db::table::company_customers::dsl::*;

                    diesel::insert_into(company_customers)
                        .values(
                            records
                                .iter()
                                .map(insertables::NewCompanyCustomer::try_from)
                                .collect::<Result<Vec<_>, _>>()?,
                        )
                        .on_conflict(id)
                        .do_update()
                        .set(to_update!(id, company_name))
                        .execute(connection)?;
                }

                {
                    use crate::db::table::jobs::dsl::*;

                    diesel::insert_into(jobs)
                        .values(
                            records
                                .iter()
                                .map(insertables::NewJob::try_from)
                                .collect::<Result<Vec<_>, _>>()?,
                        )
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
                        .execute(connection)?;
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
                    .execute(connection)?;
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
                    .execute(connection)?;
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
                    .execute(connection)?;
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
                    .execute(connection)?;
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
                    .execute(connection)?;
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
                    .execute(connection)?;
            }
        }

        Ok(())
    }
}
