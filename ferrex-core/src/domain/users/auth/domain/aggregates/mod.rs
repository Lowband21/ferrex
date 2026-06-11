// Authentication domain aggregates
// These are the main domain entities that enforce business rules
// and maintain consistency boundaries

mod device_session;
mod user_authentication;

pub(crate) use device_session::DeviceSessionHydration;
pub use device_session::{
    DeviceSession, DeviceSessionClientMetadata, DeviceSessionError,
    DeviceStatus,
};
pub use user_authentication::{UserAuthentication, UserAuthenticationError};
