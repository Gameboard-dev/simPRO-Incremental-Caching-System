//! Collects secondary record IDs referenced by simPRO API results.
//! These IDs are accumulated while parsing records so related records
//! can be inserted into the database before upserting records with 
//! foreign key dependencies on them.

use crate::{api::types::{Schedule, ScheduleType}, webhook::variants::Resource};

#[derive(Default, Debug)]
pub struct BinOfIDs {
    pub activity_ids: Vec<i64>,
    pub lead_ids: Vec<i64>,
    pub job_ids: Vec<i64>,
    pub cost_center_ids: Vec<i64>,
    pub quote_ids: Vec<i64>,
}

impl BinOfIDs {
    pub fn resources(&self) -> [(&[i64], Resource); 5] {
        [
            (&self.job_ids, Resource::Job),
            (&self.activity_ids, Resource::Activity),
            (&self.cost_center_ids, Resource::CostCenter),
            (&self.quote_ids, Resource::Quote),
            (&self.lead_ids, Resource::Lead),
        ]
    }
}

impl Schedule {
    pub(crate) fn parse_reference(&self, bin: &mut BinOfIDs) -> anyhow::Result<()> {
        fn ids(s: &str, delimiter: char) -> (Option<i64>, Option<i64>) {
            s.split_once(delimiter)
                .map(|(a, b)| (a.parse().ok(), b.parse().ok()))
                .unwrap_or((None, None))
        }
        match self.type_ {
            ScheduleType::Activity => {
                bin.activity_ids.push(self.reference.parse()?);
            }
            ScheduleType::Lead => {
                bin.lead_ids.push(self.reference.parse()?);
            }
            ScheduleType::Job => {
                let (job_id, cost_center_id) = ids(&self.reference, '-');
                if let Some(id) = job_id {
                    bin.job_ids.push(id);
                }
                if let Some(id) = cost_center_id {
                    bin.cost_center_ids.push(id);
                }
            }
            ScheduleType::Quote => {
                let (quote_id, cost_center_id) = ids(&self.reference, '-');
                if let Some(id) = quote_id {
                    bin.quote_ids.push(id);
                }
                if let Some(id) = cost_center_id {
                    bin.cost_center_ids.push(id);
                }
            }
        }
        Ok(())
    }
}