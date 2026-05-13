//! This module is used to parse simPRO response objects 
//! into database row insertions (Diesel Insertables)

use crate::api::types as api;
use crate::db;
use crate::db::insertables::*;
use crate::db::models::CompanyCustomer;
use crate::db::table::*;
use crate::webhook::variants::Resource;
use anyhow::{Result, anyhow};
use chrono::{DateTime, FixedOffset, Utc};
use diesel::prelude::{AsChangeset, Insertable};
use diesel::{PgConnection, QueryResult};
use diesel_derive_enum::DbEnum;

impl From<api::ScheduleType> for db::enums::ScheduleType {
    fn from(t: api::ScheduleType) -> Self {
        match t {
            api::ScheduleType::Lead => db::enums::ScheduleType::Lead,
            api::ScheduleType::Quote => db::enums::ScheduleType::Quote,
            api::ScheduleType::Job => db::enums::ScheduleType::Job,
            api::ScheduleType::Activity => db::enums::ScheduleType::Activity,
        }
    }
}

impl From<api::JobType> for db::enums::JobType {
    fn from(t: api::JobType) -> Self {
        match t {
            api::JobType::Project => db::enums::JobType::Project,
            api::JobType::Service => db::enums::JobType::Service,
            api::JobType::Prepaid => db::enums::JobType::Prepaid,
        }
    }
}

/// We use the lightweight CompanyCustomer reference object in the simPRO `/jobs` response.
/// Currently we only need the customer name, so additional lookup is unnecessary.
/// This logic is needed to normalize customers across the database
/// and to ensure referenced customers are added before jobs reference them.
impl<'a> TryFrom<&'a api::Job> for NewCompanyCustomer<'a> {
    type Error = anyhow::Error;
    fn try_from(job: &'a api::Job) -> anyhow::Result<Self> {
        Ok(Self {
            id: job.customer.id.parse::<i64>()?,
            company_name: &job.customer.company_name,
        })
    }
}

impl<'a> TryFrom<&'a api::Schedule> for NewSchedule<'a> {
    type Error = anyhow::Error;
    fn try_from(record: &'a api::Schedule) -> anyhow::Result<Self> {
        Ok(Self {
            id: record.id.parse::<i64>()?,
            date_modified: DateTime::parse_from_rfc3339(&record.date_modified)?.with_timezone(&Utc),
            notes: Some(&record.notes),
            staff_id: record.staff.id.parse::<i64>()?,
            schedule_type: record.type_.into(),
        })
    }
}

impl<'a> TryFrom<&'a api::Activity> for NewActivity<'a> {
    type Error = anyhow::Error;

    fn try_from(record: &'a api::Activity) -> Result<Self> {
        Ok(Self {
            id: record.id.parse::<i64>()?,
            name: &record.name,
        })
    }
}

impl<'a> TryFrom<&'a api::CostCenter> for NewCostCenter<'a> {
    type Error = anyhow::Error;

    fn try_from(record: &'a api::CostCenter) -> Result<Self> {
        Ok(Self {
            id: record.id.parse::<i64>()?,
            name: &record.name,
        })
    }
}

impl<'a> TryFrom<&'a api::Employee> for NewEmployee<'a> {
    type Error = anyhow::Error;

    fn try_from(record: &'a api::Employee) -> Result<Self> {
        Ok(Self {
            id: record.id.parse::<i64>()?,
            name: &record.name,
        })
    }
}

impl<'a> TryFrom<&'a api::Lead> for NewLead<'a> {
    type Error = anyhow::Error;

    fn try_from(record: &'a api::Lead) -> Result<Self> {
        Ok(Self {
            id: record.id.parse::<i64>()?,
            name: &record.name,
        })
    }
}

impl<'a> TryFrom<&'a api::Quote> for NewQuote<'a> {
    type Error = anyhow::Error;

    fn try_from(record: &'a api::Quote) -> Result<Self> {
        Ok(Self {
            id: record.id.parse::<i64>()?,
            name: &record.name,
        })
    }
}

impl<'a> TryFrom<&'a api::ScheduleRate> for NewScheduleRate<'a> {
    type Error = anyhow::Error;

    fn try_from(record: &'a api::ScheduleRate) -> Result<Self> {
        Ok(Self {
            id: record.id.parse::<i64>()?,
            name: &record.name,
        })
    }
}

impl<'a> TryFrom<&'a api::JobStatus> for NewJobStatuse<'a> {
    type Error = anyhow::Error;

    fn try_from(record: &'a api::JobStatus) -> Result<Self> {
        Ok(Self {
            id: record.id.parse::<i64>()?,
            name: &record.name,
            color: &record.color,
        })
    }
}

impl<'a> TryFrom<&'a api::Site> for NewSite<'a> {
    type Error = anyhow::Error;

    fn try_from(record: &'a api::Site) -> Result<Self> {
        Ok(Self {
            id: record.id.parse::<i64>()?,
            address_address: Some(&record.address.address),
            address_city: Some(&record.address.city),
            address_country: Some(&record.address.country),
            address_postal_code: &record.address.postal_code,
            date_modified: Some(DateTime::parse_from_rfc3339(&record.date_modified)?.with_timezone(&Utc)),
        })
    }
}

impl<'a> TryFrom<&'a api::Job> for NewJob<'a> {
    type Error = anyhow::Error;

    fn try_from(record: &'a api::Job) -> Result<Self> {
        Ok(Self {
            id: record.id.parse::<i64>()?,

            customer_id: record.customer.id.parse::<i64>()?,

            date_modified: DateTime::parse_from_rfc3339(&record.date_modified)?.with_timezone(&Utc),

            description: record.description.as_deref().unwrap_or_default(),

            name: &record.name,
            site_id: record.site.id.parse::<i64>()?,
            stage: &record.stage,
            status_id: record.status.id.parse::<i64>()?,
            job_type: record.type_.into(),
        })
    }
}

impl<'a> TryFrom<(&'a api::ScheduleBlock, i64)> for NewScheduleBlock {
    type Error = anyhow::Error;

    fn try_from((record, schedule_id): (&'a api::ScheduleBlock, i64)) -> Result<Self> {
        Ok(Self {
            schedule_id,

            iso8601_start_time: DateTime::parse_from_rfc3339(&record.iso8601_start_time)?.with_timezone(&Utc),

            iso8601_end_time: DateTime::parse_from_rfc3339(&record.iso8601_end_time)?.with_timezone(&Utc),

            schedule_rate: record.schedule_rate.id.parse::<i64>()?,
        })
    }
}
