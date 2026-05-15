use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_with::skip_serializing_none;

/// Matches the Event object structure of the consuming clients
/// * https://github.com/vkurko/calendar?#event-object
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineerEvent {
    pub id: String,
    pub iso8601_start_time: DateTime<Utc>,
    pub iso8601_end_time: DateTime<Utc>,
    #[serde(rename = "resourceIds")]
    pub resource_ids: Vec<i64>,
    pub extended_props: ExtendedProps,
}

#[skip_serializing_none]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtendedProps {
    #[serde(rename = "JobID")]
    pub job_id: Option<String>,
    #[serde(rename = "ActivityID")]
    pub activity_id: Option<i64>,
    #[serde(rename = "CostCentreID")]
    pub cost_centre_id: Option<String>,
    #[serde(rename = "SiteID")]
    pub site_id: Option<String>,
    #[serde(rename = "Status")]
    pub status: Option<String>,
    #[serde(rename = "Project")]
    pub project: Option<String>,
    #[serde(rename = "ScheduleID")]
    pub schedule_id: i64,
    pub schedule_date: Option<String>,
    pub employee_name: String,
    pub employee_position: String,
    #[serde(rename = "isCoreEngineer")]
    pub is_core_engineer: bool,
}