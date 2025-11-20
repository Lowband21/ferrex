//! Authentication update handlers
//!
//! Contains specific update logic for authentication-related messages

pub mod auth_updates;
pub mod first_run_updates;

// Re-export update functions
pub use auth_updates::*;
pub use first_run_updates::*;