use crate::records::database::r#macro::execute_deletion;
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
/// initialization and mutation.
///
/// Each resource maps its own [`Mutex<HashSet<i64>>`] using the
/// [`Resource`] `#[repr(u8)]` discriminant as the array index,
/// reducing lock contention between unrelated resources.
pub(crate) static IDS_DELETED: LazyLock<[Mutex<HashSet<i64>>; Resource::COUNT]> =
    LazyLock::new(|| array::from_fn(|_| Mutex::new(HashSet::<i64>::new())));

/// Access the [`Mutex<HashSet<i64>>`] ofhe [`IDS_DELETED`] for a given [`Resource`] index
pub(crate) fn ids_deleted_for_resource(
    resource: Resource,
) -> std::sync::MutexGuard<'static, HashSet<i64>> {
    IDS_DELETED[resource as usize]
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

impl Resource {
    /// Deletes records for this resource in a single batched PostgreSQL request using
    /// [`diesel_async`] and [`AsyncPgConnection`], so database work does not block
    /// Tokio worker threads. An empty `ids` slice is treated as a no-op.
    ///
    /// This assumes each [`Resource`] table has an `id` primary key. Deleting IDs that
    /// do not exist is allowed, and referential integrity is left to PostgreSQL and
    /// any configured `ON DELETE` behavior.
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
            Resource::Schedule => execute_deletion!(schedules, ids, connection),
            Resource::Job => execute_deletion!(jobs, ids, connection),
            Resource::Site => execute_deletion!(sites, ids, connection),
            Resource::Employee => execute_deletion!(employees, ids, connection),
            Resource::Activity => execute_deletion!(activities, ids, connection),
            Resource::CostCenter => execute_deletion!(cost_centers, ids, connection),
            Resource::Quote => execute_deletion!(quotes, ids, connection),
            Resource::Lead => execute_deletion!(leads, ids, connection),
        }
        Ok(())
    }
}
