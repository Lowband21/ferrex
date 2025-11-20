//! HTTP request handlers organized by functionality

pub mod setup;

// Re-export commonly used handlers
pub use setup::{check_setup_status, create_initial_admin};