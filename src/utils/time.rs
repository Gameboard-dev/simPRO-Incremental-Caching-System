use chrono::{DateTime, Months, ParseResult, SecondsFormat, Utc};

pub(crate) trait TimeRangeExt {
    fn to_rfc3339(self, secform: SecondsFormat, use_z: bool) -> (String, String);
    fn to_format(self, fmt: &str) -> (String, String);
}

impl TimeRangeExt for (DateTime<Utc>, DateTime<Utc>) {
    fn to_rfc3339(self, secform: SecondsFormat, use_z: bool) -> (String, String) {
        (
            self.0.to_rfc3339_opts(secform, use_z),
            self.1.to_rfc3339_opts(secform, use_z),
        )
    }
    fn to_format(self, fmt: &str) -> (String, String) {
        (
            self.0.format(fmt).to_string(),
            self.1.format(fmt).to_string(),
        )
    }
}

/// Returns a `(start, end)` [`DateTime<Utc>`] tuple as a time range.
pub(crate) fn time_range(
    base: DateTime<Utc>,
    sub_months: u32,
    add_months: u32,
) -> (DateTime<Utc>, DateTime<Utc>) {

    let start: DateTime<Utc> = base
        .checked_sub_months(Months::new(sub_months))
        .expect("Invalid start date");

    let end: DateTime<Utc> = base
        .checked_add_months(Months::new(add_months))
        .expect("invalid end date");

    return (start, end);
}

/// Parses an RFC 3339 timestamp string and normalizes it to UTC.
/// Used for API timestamps before storing them as PostgreSQL `Timestamptz`.
pub(crate) fn rfc3339_utc(value: &str) -> ParseResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value).map(|dt| dt.with_timezone(&Utc))
}