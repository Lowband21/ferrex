//! On-disk cache infra.
//!
//! This module provides a typed facade around `cacache` for integrity-checked
//! blob storage used by the image provider.

pub mod image_store;
pub mod media_store;

pub use image_store::*;
pub use media_store::*;
