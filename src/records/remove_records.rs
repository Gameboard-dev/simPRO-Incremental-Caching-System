use crate::webhook::variants::Resource;
use anyhow::Result;
use diesel::ExpressionMethods;
use diesel::query_dsl::methods::FilterDsl;
use diesel_async::pooled_connection::deadpool::Object;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use std::array;
use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};
use strum::EnumCount;

/// Deleted record IDs indexed by [`Resource`]. [`LazyLock`] defers allocation until 
/// first access. A plain `const` cannot be used because [`HashSet`] requires runtime 
/// initialization and the collection is mutated while the program is running.
/// 
/// Each resource maps to its own [`Mutex<HashSet<i64>>`] using the
/// [`Resource`] `#[repr(u8)]` discriminant as the array index,
/// reducing lock contention between unrelated resources.
pub(crate) static IDS_DELETED: LazyLock<[Mutex<HashSet<i64>>; Resource::COUNT]> =
    LazyLock::new(|| array::from_fn(|_| Mutex::new(HashSet::<i64>::new())));

/// Access the [`Mutex<HashSet<i64>>`] of the [`IDS_DELETED`] array for a provided [`Resource`] index
pub(crate) fn ids_deleted_for_resource(
    resource: Resource,
) -> std::sync::MutexGuard<'static, HashSet<i64>> {
    IDS_DELETED[resource as usize]
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

impl Resource {
    /// This function performs batched `DELETE` operations using [`diesel_async`]
    /// with [`AsyncPgConnection`] on a slice of IDs so PostgreSQL queries execute without blocking
    /// Tokio worker threads.
    ///
    /// # Assumptions
    /// * Every table has an `id` column.
    /// * The `id` column is always the primary key.
    /// * An empty `ids` list is treated as a no-op.
    /// * Attempting to delete non-existent rows succeeds without error.
    /// * Referential integrity is delegated to PostgreSQL and any configured `ON DELETE` behavior.
    ///
    /// # Example
    /// ```rust,ignore
    /// Resource::Job.delete_records(
    ///     vec![1024, 1023],
    ///     &mut connection,
    /// ).await?;
    /// ```
    ///
    /// This would execute a batched delete against the `jobs` table
    /// for IDs `1024`, `1023`.
    #[allow(unused)]
    #[tracing::instrument(skip(self, ids, connection))]
    pub(crate) async fn remove_records_by_id(
        &self,
        ids: &[i64],
        connection: &mut Object<AsyncPgConnection>,
    ) -> anyhow::Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        match self {
            Resource::Schedule => {
                use crate::db::table::schedules::dsl::*;

                diesel::delete(schedules.filter(id.eq_any(ids)))
                    .execute(connection)
                    .await?;
            }

            Resource::Job => {
                use crate::db::table::jobs::dsl::*;

                diesel::delete(jobs.filter(id.eq_any(ids)))
                    .execute(connection)
                    .await?;
            }

            Resource::Site => {
                use crate::db::table::sites::dsl::*;

                diesel::delete(sites.filter(id.eq_any(ids)))
                    .execute(connection)
                    .await?;
            }

            Resource::Employee => {
                use crate::db::table::employees::dsl::*;

                diesel::delete(employees.filter(id.eq_any(ids)))
                    .execute(connection)
                    .await?;
            }

            Resource::Activity => {
                use crate::db::table::activities::dsl::*;

                diesel::delete(activities.filter(id.eq_any(ids)))
                    .execute(connection)
                    .await?;
            }

            Resource::CostCenter => {
                use crate::db::table::cost_centers::dsl::*;

                diesel::delete(cost_centers.filter(id.eq_any(ids)))
                    .execute(connection)
                    .await?;
            }

            Resource::Quote => {
                use crate::db::table::quotes::dsl::*;

                diesel::delete(quotes.filter(id.eq_any(ids)))
                    .execute(connection)
                    .await?;
            }

            Resource::Lead => {
                use crate::db::table::leads::dsl::*;

                diesel::delete(leads.filter(id.eq_any(ids)))
                    .execute(connection)
                    .await?;
            }
        }

        Ok(())
    }
}
