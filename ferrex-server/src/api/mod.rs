//! API module for centralized endpoint management
//! 
//! This module provides a clean separation of API concerns with dedicated
//! handlers for different functional areas of the application.

pub mod user_management;

pub use user_management::*;