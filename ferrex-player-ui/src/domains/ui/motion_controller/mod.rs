pub mod config;
pub mod controller;
pub mod messages;
pub mod update;

// Re-export primary types for convenience
pub use config::MotionControllerConfig;
pub use controller::MotionController;
pub use messages::{Direction, MotionMessage};
