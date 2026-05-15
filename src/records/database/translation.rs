//! This module is used to parse simPRO response objects
//! into database row insertions (Diesel Insertables)

use crate::api::types as api;
use crate::db::insertables::*;
use crate::db::models::CompanyCustomer;
use crate::db::table::*;
use crate::db::{self, insertables};
use crate::parse::schedule::reference::ScheduleReference;
use crate::utils::time::rfc3339_utc;
use crate::webhook::variants::Resource;
use anyhow::{Result, anyhow};
use chrono::{DateTime, FixedOffset, Utc};
use diesel::prelude::{AsChangeset, Insertable};
use diesel::{PgConnection, QueryResult, Queryable, Selectable};
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

impl<'a> TryFrom<&'a api::Job> for NewCompanyCustomer<'a> {
    type Error = anyhow::Error;
    fn try_from(job: &'a api::Job) -> anyhow::Result<Self> {
        Ok(Self {
            id: job.customer.id,
            company_name: &job.customer.company_name,
        })
    }
}

impl<'a> TryFrom<&'a api::Job> for NewJobStatuse<'a> {
    type Error = anyhow::Error;
    fn try_from(job: &'a api::Job) -> anyhow::Result<Self> {
        Ok(Self {
            id: job.status.id,
            color: &job.status.color,
            name: &job.status.name,
        })
    }
}

impl<'a> TryFrom<&'a api::Schedule> for NewSchedule<'a> {
    type Error = anyhow::Error;
    fn try_from(record: &'a api::Schedule) -> anyhow::Result<Self> {
        Ok(Self {
            id: record.id,
            date_modified: rfc3339_utc(&record.date_modified)?,
            notes: Some(&record.notes),
            staff_id: record.staff.id,
            schedule_type: record.type_.into(),
        })
    }
}

impl<'a> TryFrom<(i64, &api::ScheduleBlock)> for NewScheduleBlock {
    type Error = anyhow::Error;
    fn try_from((schedule_id, block): (i64, &api::ScheduleBlock)) -> anyhow::Result<Self> {
        Ok(Self {
            iso8601_start_time: rfc3339_utc(&block.iso8601_start_time)?,
            iso8601_end_time: rfc3339_utc(&block.iso8601_end_time)?,
            schedule_id,
            schedule_rate: block.schedule_rate.id,
        })
    }
}

impl<'a> TryFrom<&'a api::Activity> for NewActivity<'a> {
    type Error = anyhow::Error;

    fn try_from(record: &'a api::Activity) -> Result<Self> {
        Ok(Self {
            id: record.id,
            name: &record.name,
        })
    }
}

impl<'a> TryFrom<&'a api::CostCenter> for NewCostCenter<'a> {
    type Error = anyhow::Error;

    fn try_from(record: &'a api::CostCenter) -> Result<Self> {
        Ok(Self {
            id: record.id,
            name: &record.name,
        })
    }
}

impl<'a> TryFrom<&'a api::Employee> for NewEmployee<'a> {
    type Error = anyhow::Error;

    fn try_from(e: &'a api::Employee) -> Result<Self> {
        Ok(Self {
            id: e.id,
            name: &e.name,
            position: &e.position,
        })
    }
}

impl<'a> TryFrom<&'a api::Lead> for NewLead<'a> {
    type Error = anyhow::Error;

    fn try_from(record: &'a api::Lead) -> Result<Self> {
        Ok(Self {
            id: record.id,
            name: &record.name,
        })
    }
}

impl<'a> TryFrom<&'a api::Quote> for NewQuote<'a> {
    type Error = anyhow::Error;

    fn try_from(record: &'a api::Quote) -> anyhow::Result<Self> {
        Ok(Self {
            id: record.id,
            name: &record.name,
        })
    }
}

impl<'a> TryFrom<&'a api::ScheduleRate> for NewScheduleRate<'a> {
    type Error = anyhow::Error;

    fn try_from(record: &'a api::ScheduleRate) -> Result<Self> {
        Ok(Self {
            id: record.id,
            name: &record.name,
        })
    }
}

impl<'a> TryFrom<&'a api::JobStatus> for NewJobStatuse<'a> {
    type Error = anyhow::Error;

    fn try_from(record: &'a api::JobStatus) -> Result<Self> {
        Ok(Self {
            id: record.id,
            name: &record.name,
            color: &record.color,
        })
    }
}

impl<'a> TryFrom<&'a api::Site> for NewSite<'a> {
    type Error = anyhow::Error;

    fn try_from(record: &'a api::Site) -> Result<Self> {
        Ok(Self {
            id: record.id,
            address_address: Some(&record.address.address),
            address_city: Some(&record.address.city),
            address_country: Some(&record.address.country),
            address_postal_code: &record.address.postal_code,
            date_modified: Some(rfc3339_utc(&record.date_modified)?),
        })
    }
}

impl<'a> TryFrom<&'a api::Job> for NewJob<'a> {
    type Error = anyhow::Error;

    fn try_from(record: &'a api::Job) -> Result<Self> {
        Ok(Self {
            id: record.id,

            customer_id: record.customer.id,

            date_modified: rfc3339_utc(&record.date_modified)?,

            description: record.description.as_deref().unwrap_or_default(),

            name: &record.name,
            site_id: record.site.id,
            stage: &record.stage,
            status_id: record.status.id,
            job_type: record.type_.into(),
        })
    }
}

impl<'a> TryFrom<(i64, i64, i64)> for NewJobSchedule {
    type Error = anyhow::Error;
    fn try_from((schedule_id, job_id, cost_center_id): (i64, i64, i64)) -> Result<Self> {
        Ok(Self {
            schedule_id,
            job_id,
            cost_center_id,
        })
    }
}

impl<'a> TryFrom<(i64, i64)> for NewActivitySchedule {
    type Error = anyhow::Error;
    fn try_from((schedule_id, activity_id): (i64, i64)) -> Result<Self> {
        Ok(Self {
            schedule_id,
            activity_id,
        })
    }
}

impl<'a> TryFrom<(i64, i64, i64)> for NewQuoteSchedule {
    type Error = anyhow::Error;
    fn try_from((schedule_id, quote_id, cost_center_id): (i64, i64, i64)) -> Result<Self> {
        Ok(Self {
            schedule_id,
            quote_id,
            cost_center_id,
        })
    }
}

impl<'a> TryFrom<(i64, i64)> for NewLeadSchedule {
    type Error = anyhow::Error;
    fn try_from((schedule_id, lead_id): (i64, i64)) -> Result<Self> {
        Ok(Self {
            schedule_id,
            lead_id,
        })
    }
}

impl<'a> TryFrom<(&'a api::ScheduleBlock, i64)> for NewScheduleBlock {
    type Error = anyhow::Error;

    fn try_from((r, schedule_id): (&'a api::ScheduleBlock, i64)) -> Result<Self> {
        Ok(Self {
            schedule_id,
            iso8601_start_time: rfc3339_utc(&r.iso8601_start_time)?,
            iso8601_end_time: rfc3339_utc(&r.iso8601_end_time)?,
            schedule_rate: r.schedule_rate.id,
        })
    }
}

pub(crate) struct ScheduleRows<'a> {
    pub(crate) schedules: Vec<insertables::NewSchedule<'a>>,
    pub(crate) job_schedules: Vec<insertables::NewJobSchedule>,
    pub(crate) lead_schedules: Vec<insertables::NewLeadSchedule>,
    pub(crate) quote_schedules: Vec<insertables::NewQuoteSchedule>,
    pub(crate) activity_schedules: Vec<insertables::NewActivitySchedule>,
    pub(crate) schedule_blocks: Vec<insertables::NewScheduleBlock>,
}

pub(crate) fn prepare_schedule_rows(records: &[api::Schedule]) -> anyhow::Result<ScheduleRows<'_>> {
    let mut rows = ScheduleRows {
        schedules: Vec::with_capacity(records.len()),
        job_schedules: Vec::new(),
        lead_schedules: Vec::new(),
        quote_schedules: Vec::new(),
        activity_schedules: Vec::new(),
        schedule_blocks: Vec::new(),
    };

    for schedule in records {
        rows.schedules
            .push(insertables::NewSchedule::try_from(schedule)?);

        match schedule.parse_reference()? {
            ScheduleReference::Job {
                job_id,
                cost_center_id,
            } => {
                rows.job_schedules
                    .push(insertables::NewJobSchedule::try_from((
                        schedule.id,
                        job_id,
                        cost_center_id,
                    ))?);
            }

            ScheduleReference::Lead { lead_id } => {
                rows.lead_schedules
                    .push(insertables::NewLeadSchedule::try_from((
                        schedule.id,
                        lead_id,
                    ))?);
            }

            ScheduleReference::Quote {
                quote_id,
                cost_center_id,
            } => {
                rows.quote_schedules
                    .push(insertables::NewQuoteSchedule::try_from((
                        schedule.id,
                        quote_id,
                        cost_center_id,
                    ))?);
            }

            ScheduleReference::Activity { activity_id } => {
                rows.activity_schedules
                    .push(insertables::NewActivitySchedule::try_from((
                        schedule.id,
                        activity_id,
                    ))?);
            }
        }

        for block in &schedule.blocks {
            rows.schedule_blocks
                .push(insertables::NewScheduleBlock::try_from((
                    block,
                    schedule.id,
                ))?);
        }
    }

    Ok(rows)
}
