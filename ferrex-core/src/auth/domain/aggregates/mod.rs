// Authentication domain aggregates
// These are the main domain entities that enforce business rules
// and maintain consistency boundaries

mod device_session;
mod user_authentication;

pub use device_session::{DeviceSession, DeviceSessionError, DeviceStatus};
pub use user_authentication::{UserAuthentication, UserAuthenticationError};