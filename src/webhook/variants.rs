use strum_macros::{EnumCount, EnumIter};

#[repr(u8)]
#[derive(EnumIter, EnumCount, Debug, Copy, Clone)]
pub enum Resource {
    Job = 0,
    Site = 1,
    Schedule = 2,
    Activity = 3,
    Employee = 4,
    CostCenter = 5,
    Quote = 6,
    Lead = 7,
}

/// The enum discriminant ordering is significant because
/// [`EventBuffer::index`] stores `(Resource, Operation)` pairs
/// in Row-Major indexing and `sync_once` iterates the buffer sequentially.
///
/// This means operations are processed in the same order as the
/// enum discriminants:
///
/// ```text
/// Created -> Updated -> Deleted
/// ```
///
/// Changing variant order or discriminant values will therefore
/// change synchronization execution order.
/// 
/// The following edge cases are expected to occur rarely:
/// 
/// Deleted(id=123)      -- NOT IN INTERNAL DATABASE
/// Created(id=123)      (Webhook Timestamp)
///
/// Updated(id=123)      -- NOT IN EXTERNAL DATABASE
/// Deleted(id=123)      (Webhook Timestamp)
/// 
/// Mitigations:
/// 
/// DELETED -> CREATED
/// -- Keep track of IDs deleted and ignore IDs deleted in creation
/// -- [`crate::records::remove_records::IDS_DELETED`]
///
/// UPDATED -> DELETED
/// -- Treat missing records in responses as a strong indication of deletion
/// 
#[repr(u8)]
#[derive(EnumIter, EnumCount, Debug, Copy, Clone, PartialEq, Eq)]
pub enum Operation {
    Created = 0,
    Updated = 1,
    Deleted = 2,
}

