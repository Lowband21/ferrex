//! Sorting module for hybrid client/server sorting functionality
//!
//! This module provides:
//! - Core traits for sortable entities
//! - Field marker types for compile-time safe sorting
//! - Sort key extraction and comparison
//! - Strategy pattern for composable sorting
//!
pub mod impls;
pub mod simple;
pub mod utils;

pub mod fallback;
pub mod fields;
pub mod fieldsets;
pub mod keys;
pub mod performance;
pub mod strategy;
pub mod traits;

pub use fallback::*;
pub use fields::*;
pub use fieldsets::*;
pub use keys::*;
pub use performance::*;
pub use simple::*;
pub use strategy::*;
pub use traits::*;
