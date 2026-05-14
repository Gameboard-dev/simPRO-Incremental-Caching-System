use chrono::{DateTime, SecondsFormat, Utc};

pub(crate) trait TimeRangeExt {
    fn to_rfc3339(self, secform: SecondsFormat, use_z: bool) -> (String, String);
}

impl TimeRangeExt for (DateTime<Utc>, DateTime<Utc>) {
    fn to_rfc3339(self, secform: SecondsFormat, use_z: bool) -> (String, String) {
        (
            self.0.to_rfc3339_opts(secform, use_z),
            self.1.to_rfc3339_opts(secform, use_z),
        )
    }
}