use chrono::{DateTime, Utc};
use diesel::QueryableByName;
use diesel::sql_types::{
    BigInt, Nullable, Text, Timestamptz,
};

#[derive(Debug, QueryableByName)]
pub(crate) struct EngineerEventRow {
    #[diesel(sql_type = BigInt)]
    pub schedule_id: i64,

    #[diesel(sql_type = Timestamptz)]
    pub iso8601_start_time: DateTime<Utc>,

    #[diesel(sql_type = Timestamptz)]
    pub iso8601_end_time: DateTime<Utc>,

    #[diesel(sql_type = BigInt)]
    pub employee_id: i64,

    #[diesel(sql_type = Text)]
    pub employee_name: String,

    #[diesel(sql_type = Text)]
    pub employee_position: String,

    #[diesel(sql_type = Nullable<BigInt>)]
    pub job_id: Option<i64>,

    #[diesel(sql_type = Nullable<BigInt>)]
    pub quote_id: Option<i64>,

    #[diesel(sql_type = Nullable<BigInt>)]
    pub lead_id: Option<i64>,

    #[diesel(sql_type = Nullable<BigInt>)]
    pub activity_id: Option<i64>,

    #[diesel(sql_type = Nullable<BigInt>)]
    pub job_cost_center_id: Option<i64>,

    #[diesel(sql_type = Nullable<BigInt>)]
    pub quote_cost_center_id: Option<i64>,

    #[diesel(sql_type = Nullable<BigInt>)]
    pub site_id: Option<i64>,

    #[diesel(sql_type = Nullable<Text>)]
    pub status: Option<String>,

    #[diesel(sql_type = Nullable<Text>)]
    pub project: Option<String>,

    #[diesel(sql_type = diesel::sql_types::Bool)]
    pub is_core_engineer: bool,
}
