//! Device management settings sub-domain.

pub mod messages;
pub mod state;

pub use messages::DevicesMessage;
pub use state::{DeviceManagementState, UserDevice};
