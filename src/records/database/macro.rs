
/// Executes a series of asynchronous Diesel statements inside a single atomic
/// (*all-or-nothing*) PostgreSQL transaction.
///
/// The macro issues `BEGIN`, awaits the supplied async block, and then commits a single transaction
/// only when that block returns `Ok(())`. If that block returns an error, the macro attempts a
/// best-effort `ROLLBACK` and returns the original error so the caller sees the failure that
/// caused the transaction to abort, rather than any rollback cleanup result.
///
/// The supplied block must evaluate to [`anyhow::Result<()>`], and the macro is intended to be
/// invoked from async functions that also return [`anyhow::Result<()>`].
macro_rules! in_transaction {
    ($connection:expr, $body:block) => {{

        diesel::sql_query("BEGIN")
            .execute(&mut *$connection)
            .await?;

        let response: anyhow::Result<()> = async $body.await;

        match response {
            Ok(()) => {
                diesel::sql_query("COMMIT")
                    .execute(&mut *$connection)
                    .await?;
            }
            Err(err) => return Err({
                let _ = diesel::sql_query("ROLLBACK")
                    .execute(&mut *$connection)
                    .await;
                err
            }),
        }
    }};
}
pub(crate) use in_transaction;

macro_rules! upsert_api_records {
    (
        $records:expr,
        $connection:expr,
        $table_mod:ident::$table:ident,
        $insertable:ty,
        $conflict:tt,
        [$($update_col:ident),+ $(,)?]
    ) => {{
        let insertables = $records
            .iter()
            .map(<$insertable>::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        if insertables.is_empty() {
            return Ok(());
        }

        use $crate::db::table::$table_mod::dsl::*;
        
        diesel::insert_into($table)
            .values(&insertables)
            .on_conflict($conflict)
            .do_update()
            .set(($( $update_col.eq(diesel::upsert::excluded($update_col)) ),+))
            .execute(&mut *$connection)
            .await?;
    }};
}
pub(crate) use upsert_api_records;

/// Inserts a slice of  [`diesel::Insertable`] rows and applies the requested 'ON CONFLICT' action.
/// The macro returns early with `Ok(())` when the provided slice is empty. Otherwise it performs 
/// `INSERT ... ON CONFLICT` against the provided table and conflict target.
///
/// ### Scenario 1: Do Update
/// Use `do_update [columns...]` when existing rows should be updated.
/// Each listed column is assigned from PostgreSQL's `EXCLUDED` pseudo-table,
/// making the update equivalent to `SET column = EXCLUDED.column` for every listed column.
///
/// ### Scenario 2: Do Nothing
/// Use `do_nothing` for tables where the existence of the row is enough
/// and an update should be ignored.
macro_rules! insert_rows {
    (
        $rows:expr,
        $connection:expr,
        $table_mod:ident::$table:ident,
        $conflict:tt,
        do_update
        [$($update_col:ident),+ $(,)?]
    ) => {{
        if $rows.is_empty() {
            return Ok(());
        }

        use $crate::db::table::$table_mod::dsl::*;

        diesel::insert_into($table)
            .values($rows)
            .on_conflict($conflict)
            .do_update()
            .set(($( $update_col.eq(diesel::upsert::excluded($update_col)) ),+))
            .execute(&mut *$connection)
            .await?;
    }};
    (
        $rows:expr,
        $connection:expr,
        $table_mod:ident::$table:ident,
        $conflict:tt,
        do_nothing
    ) => {{
        if $rows.is_empty() {
            return Ok(());
        }

        use $crate::db::table::$table_mod::dsl::*;

        diesel::insert_into($table)
            .values($rows)
            .on_conflict($conflict)
            .do_nothing()
            .execute(&mut *$connection)
            .await?;
    }};
}
pub(crate) use insert_rows;

/// Deletes rows from one Diesel table by matching a batch of primary-key IDs.
/// The table must live under [`crate::db::table`] and expose an `id` column in
/// its `dsl` module.
macro_rules! execute_deletion {
    ($table:ident, $ids:expr, $connection:expr) => {{
        use $crate::db::table::$table::dsl::*;
        diesel::delete($table.filter(id.eq_any($ids)))
            .execute($connection)
            .await?;
    }};
}
pub(crate) use execute_deletion;
