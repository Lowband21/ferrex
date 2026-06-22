//! Media infra adapters.
//!
//! Hosts integrations that touch external systems (database, HTTP, FFmpeg)
//! so the media domain can stay decoupled from runtime dependencies.

#[cfg(feature = "database")]
pub mod image_service;

#[cfg(feature = "database")]
pub mod indices;

pub mod metadata;

#[cfg(feature = "database")]
pub mod timed_text;

pub mod providers;
