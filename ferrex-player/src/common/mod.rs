//! Common module containing shared utilities and types
//!
//! This module provides common functionality used across multiple domains

pub mod clear_database;
pub mod messages;
pub mod prelude;
pub mod ui_utils;

// Re-export commonly used items
pub use prelude::*;
