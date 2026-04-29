//! This module is used to parse responses from the simPRO API
//! into database rows (diesel insertable structs)
//! and to parse foreign key references nested in strings

use crate::api::types as record;
use crate::db::enums::*;
use crate::db::insertables::*;
use crate::db::table::*;
use anyhow::{Result, anyhow};
use chrono::{DateTime, FixedOffset, Utc};
use diesel::prelude::{AsChangeset, Insertable};
use diesel::{PgConnection, QueryResult};
use diesel_derive_enum::DbEnum;

impl From<record::ScheduleType> for ScheduleType {
    fn from(t: record::ScheduleType) -> Self {
        match t {
            record::ScheduleType::Lead => {
                ScheduleType::Lead
            }
            record::ScheduleType::Quote => {
                ScheduleType::Quote
            }
            record::ScheduleType::Job => ScheduleType::Job,
            record::ScheduleType::Activity => {
                ScheduleType::Activity
            }
        }
    }
}

impl From<record::JobType> for JobType {
    fn from(t: record::JobType) -> Self {
        match t {
            record::JobType::Project => JobType::Project,
            record::JobType::Service => JobType::Service,
            record::JobType::Prepaid => JobType::Prepaid,
        }
    }
}

impl<'a> TryFrom<&'a record::Schedule> for NewSchedule<'a> {
    type Error = anyhow::Error;
    fn try_from(
        record: &'a record::Schedule,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            id: record.id.parse::<i64>()?,
            date_modified: DateTime::parse_from_rfc3339(
                &record.date_modified,
            )?
            .with_timezone(&Utc),
            notes: Some(&record.notes),
            staff_id: record.staff.id.parse::<i64>()?,
            schedule_type: record.type_.into(),
        })
    }
}

impl<'a> TryFrom<&'a record::Activity> for NewActivity<'a> {
    type Error = anyhow::Error;

    fn try_from(
        record: &'a record::Activity,
    ) -> Result<Self> {
        Ok(Self {
            id: record.id.parse::<i64>()?,
            name: &record.name,
        })
    }
}

impl<'a> TryFrom<&'a record::CostCenter>
    for NewCostCenter<'a>
{
    type Error = anyhow::Error;

    fn try_from(
        record: &'a record::CostCenter,
    ) -> Result<Self> {
        Ok(Self {
            id: record.id.parse::<i64>()?,
            name: &record.name,
        })
    }
}

impl<'a> TryFrom<&'a record::Employee> for NewEmployee<'a> {
    type Error = anyhow::Error;

    fn try_from(
        record: &'a record::Employee,
    ) -> Result<Self> {
        Ok(Self {
            id: record.id.parse::<i64>()?,
            name: &record.name,
        })
    }
}

impl<'a> TryFrom<&'a record::Lead> for NewLead<'a> {
    type Error = anyhow::Error;

    fn try_from(record: &'a record::Lead) -> Result<Self> {
        Ok(Self {
            id: record.id.parse::<i64>()?,
            name: &record.name,
        })
    }
}

impl<'a> TryFrom<&'a record::Quote> for NewQuote<'a> {
    type Error = anyhow::Error;

    fn try_from(record: &'a record::Quote) -> Result<Self> {
        Ok(Self {
            id: record.id.parse::<i64>()?,
            name: &record.name,
        })
    }
}

impl<'a> TryFrom<&'a record::ScheduleRate>
    for NewScheduleRate<'a>
{
    type Error = anyhow::Error;

    fn try_from(
        record: &'a record::ScheduleRate,
    ) -> Result<Self> {
        Ok(Self {
            id: record.id.parse::<i64>()?,
            name: &record.name,
        })
    }
}

impl<'a> TryFrom<&'a record::JobStatus>
    for NewJobStatuse<'a>
{
    type Error = anyhow::Error;

    fn try_from(
        record: &'a record::JobStatus,
    ) -> Result<Self> {
        Ok(Self {
            id: record.id.parse::<i64>()?,
            name: &record.name,
            color: &record.color,
        })
    }
}

impl<'a> TryFrom<&'a record::Site> for NewSite<'a> {
    type Error = anyhow::Error;

    fn try_from(record: &'a record::Site) -> Result<Self> {
        Ok(Self {
            id: record.id.parse::<i64>()?,
            address_address: Some(&record.address.address),
            address_city: Some(&record.address.city),
            address_country: Some(&record.address.country),
            address_postal_code: &record
                .address
                .postal_code,
            date_modified: Some(
                DateTime::parse_from_rfc3339(
                    &record.date_modified,
                )?
                .with_timezone(&Utc),
            ),
        })
    }
}

impl<'a> TryFrom<&'a record::Job> for NewJob<'a> {
    type Error = anyhow::Error;

    fn try_from(record: &'a record::Job) -> Result<Self> {
        Ok(Self {
            id: record.id.parse::<i64>()?,

            customer_company_name: &record
                .customer
                .company_name,

            date_modified: DateTime::parse_from_rfc3339(
                &record.date_modified,
            )?
            .with_timezone(&Utc),

            description: record
                .description
                .as_deref()
                .unwrap_or_default(),

            name: &record.name,
            site_id: record.site.id.parse::<i64>()?,
            stage: &record.stage,
            status_id: record.status.id.parse::<i64>()?,
            job_type: record.type_.into(),
        })
    }
}

impl<'a> TryFrom<(&'a record::ScheduleBlock, i64)>
    for NewScheduleBlock
{
    type Error = anyhow::Error;

    fn try_from(
        (record, schedule_id): (
            &'a record::ScheduleBlock,
            i64,
        ),
    ) -> Result<Self> {
        Ok(Self {
            schedule_id,

            iso8601_start_time:
                DateTime::parse_from_rfc3339(
                    &record.iso8601_start_time,
                )?
                .with_timezone(&Utc),

            iso8601_end_time: DateTime::parse_from_rfc3339(
                &record.iso8601_end_time,
            )?
            .with_timezone(&Utc),

            schedule_rate: record
                .schedule_rate
                .id
                .parse::<i64>()?,
        })
    }
}

impl record::Schedule {
    /// Parses the `Reference` value in a `Schedule`
    /// record retrieved from simPRO into one (or multiple) IDs
    fn parse_reference(self) -> anyhow::Result<()> {
        fn parse_splittable(
            s: &str,
            delimiter: char,
        ) -> (Option<i64>, Option<i64>) {
            s.split_once(delimiter)
                .map(|(a, b)| {
                    (a.parse().ok(), b.parse().ok())
                })
                .unwrap_or((None, None))
        }
        // ----------------------------------------------
        let mut activity_id: Option<i64> = None;
        let mut lead_id: Option<i64> = None;
        let mut job_id: Option<i64> = None;
        let mut cost_center_id: Option<i64> = None;
        let mut quote_id: Option<i64> = None;
        // ----------------------------------------------
        match self.type_ {
            record::ScheduleType::Activity => {
                activity_id =
                    Some(self.reference.parse::<i64>()?);
            }
            record::ScheduleType::Lead => {
                lead_id =
                    Some(self.reference.parse::<i64>()?);
            }
            record::ScheduleType::Job => {
                (job_id, cost_center_id) =
                    parse_splittable(&self.reference, '-');
            }
            record::ScheduleType::Quote => {
                (quote_id, cost_center_id) =
                    parse_splittable(&self.reference, '-');
            }
        }
        Ok(())
    }
}
