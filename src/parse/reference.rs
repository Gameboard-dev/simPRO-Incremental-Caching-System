use crate::{api::types as api, webhook::variants::Resource};

// A bin of IDs from a parsed 'Reference' field 
// in a simPRO 'Schedule' API response object
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
}

impl api::Schedule {
    pub(crate) fn parse_id_reference(&self, bin: &mut IDs) -> anyhow::Result<()> {
        fn separate_delimited_ids(s: &str, delimiter: char) -> (Option<i64>, Option<i64>) {
            s.split_once(delimiter)
                .map(|(a, b)| (a.parse().ok(), b.parse().ok()))
                .unwrap_or((None, None))
        }
        match self.type_ {
            api::ScheduleType::Activity => {
                bin.activity_ids.push(self.reference.parse()?);
            }
            api::ScheduleType::Lead => {
                bin.lead_ids.push(self.reference.parse()?);
            }
            api::ScheduleType::Job => {
                let (job_id, cost_center_id) = separate_delimited_ids(&self.reference, '-');
                if let Some(id) = job_id {
                    bin.job_ids.push(id);
                }
                if let Some(id) = cost_center_id {
                    bin.cost_center_ids.push(id);
                }
            }
            api::ScheduleType::Quote => {
                let (quote_id, cost_center_id) = separate_delimited_ids(&self.reference, '-');
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
