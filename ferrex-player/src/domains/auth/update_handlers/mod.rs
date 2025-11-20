//! Authentication update handlers
//!
//! Contains specific update logic for authentication-related messages

pub mod auth_flow;
pub mod first_run;

// Re-export update functions for the update router
pub use auth_flow::*;
pub use first_run::*;
