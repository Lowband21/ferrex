//! Performance settings sub-domain.

pub mod messages;
pub mod state;

pub use messages::PerformanceMessage;
pub use state::{EasingKind, PerformanceState};
