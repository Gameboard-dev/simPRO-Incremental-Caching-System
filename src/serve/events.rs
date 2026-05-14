
/// Retrieve events 1 month up to the Date on startup
/// cache them in the database and write the events concatenation query route here
/// 

/// Edge cases:
///  A: 
///     (1) Utc::now() is SET
///     (2) Record created
///     (3) Date_Created > Utc::now()
///     (4) Future Event Not Caught
///  B: 
///     (1) Record created
///     (2) Date_Created < Utc::now()
///     (3) Date Webhook Triggered > Date_Created
///  C:
///     (1) Webhook Failure
///  D: 
///     (1) simPRO Unavailability
///  E:
///     Server Failure
///     --> Persistence pipeline for IDs pending upsertion 
///         retriggered idiomatically on startup from JSON
///     --> File updated on succesful synchronization
///     --> IDs separated between Operation/Resource combinations
///         UPDATE != CREATE
///
/// To deal with edge cases we will verify IDs are in the database
/// when a request is made and pull lazily if not found.
/// 
 