//! Sorting module for hybrid client/server sorting functionality
//! 
//! This module provides:
//! - Core traits for sortable entities
//! - Field marker types for compile-time safe sorting
//! - Sort key extraction and comparison
//! - Strategy pattern for composable sorting

pub mod traits;
pub mod fields;
pub mod keys;
pub mod fieldsets;
pub mod impls;
pub mod strategy;
pub mod utils;
pub mod performance;
pub mod fallback;

#[cfg(test)]
mod tests;

pub use traits::*;
pub use fields::*;
pub use keys::*;
pub use fieldsets::*;
pub use strategy::*;
pub use fallback::*;
pub use performance::*;