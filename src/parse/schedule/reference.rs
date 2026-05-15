use crate::{api::types as api, webhook::variants::Resource};

#[derive(Default, Debug)]
pub struct IDs {
    pub activity_ids: Vec<i64>,
    pub lead_ids: Vec<i64>,
    pub job_ids: Vec<i64>,
    pub cost_center_ids: Vec<i64>,
    pub quote_ids: Vec<i64>,
}

impl IDs {
    pub fn resources(&self) -> [(&[i64], Resource); 5] {
        [
            (&self.job_ids, Resource::Job),
            (&self.activity_ids, Resource::Activity),
            (&self.cost_center_ids, Resource::CostCenter),
            (&self.quote_ids, Resource::Quote),
            (&self.lead_ids, Resource::Lead),
        ]
    }

    pub fn extend(&mut self, reference: ScheduleReference) {
        match reference {
            ScheduleReference::Activity { activity_id } => {
                self.activity_ids.push(activity_id);
            }

            ScheduleReference::Lead { lead_id } => {
                self.lead_ids.push(lead_id);
            }

            ScheduleReference::Job {
                job_id,
                cost_center_id,
            } => {
                self.job_ids.push(job_id);
                self.cost_center_ids.push(cost_center_id);
            }

            ScheduleReference::Quote {
                quote_id,
                cost_center_id,
            } => {
                self.quote_ids.push(quote_id);
                self.cost_center_ids.push(cost_center_id);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleReference {
    Activity {
        activity_id: i64,
    },
    Lead {
        lead_id: i64,
    },
    Job {
        job_id: i64,
        cost_center_id: i64,
    },
    Quote {
        quote_id: i64,
        cost_center_id: i64,
    },
}

impl api::Schedule {
    pub(crate) fn reference_ids(&self) -> anyhow::Result<ScheduleReference> {

        fn delimit_ids(id_string: &str) -> anyhow::Result<(i64, i64)> {
            let (left, right) = id_string.split_once('-').ok_or_else(|| {
                anyhow::anyhow!("invalid schedule reference pair: {id_string}")
            })?;
            Ok((left.parse()?, right.parse()?))
        }

        match self.type_ {
            api::ScheduleType::Activity => Ok(ScheduleReference::Activity {
                activity_id: self.reference.parse()?,
            }),

            api::ScheduleType::Lead => Ok(ScheduleReference::Lead {
                lead_id: self.reference.parse()?,
            }),

            api::ScheduleType::Job => {
                let (job_id, cost_center_id) = delimit_ids(&self.reference)?;

                Ok(ScheduleReference::Job {
                    job_id,
                    cost_center_id,
                })
            }

            api::ScheduleType::Quote => {
                let (quote_id, cost_center_id) = delimit_ids(&self.reference)?;

                Ok(ScheduleReference::Quote {
                    quote_id,
                    cost_center_id,
                })
            }
        }
    }
}