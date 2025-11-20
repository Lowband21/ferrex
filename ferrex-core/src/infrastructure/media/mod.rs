//! Media infrastructure adapters.
//!
//! Hosts integrations that touch external systems (database, HTTP, FFmpeg)
//! so the media domain can stay decoupled from runtime dependencies.

#[cfg(feature = "database")]
pub mod image_service;

#[cfg(feature = "database")]
pub mod indices;

#[cfg(feature = "ffmpeg")]
pub mod metadata;

pub mod providers;
