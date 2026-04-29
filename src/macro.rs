/// ```sql
/// INSERT INTO table_name (col1, col2, col3)
///    VALUES (val1, val2, val3)
///    ON CONFLICT (unique_column)
///    DO UPDATE
///    SET col2 = EXCLUDED.col2,   
///        col3 = EXCLUDED.col3;
/// ```
#[macro_export]
macro_rules! update {
    ($($col:ident),+ $(,)?) => {
        (
            $( $col.eq(diesel::upsert::excluded($col)) ),+
        )
    };
}
pub use update;
