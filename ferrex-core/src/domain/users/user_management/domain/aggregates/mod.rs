// User management domain aggregates
// These are the main domain entities that enforce business rules
// and maintain consistency boundaries for user management operations

mod user;

pub use user::{UserAggregate, UserAggregateError};
