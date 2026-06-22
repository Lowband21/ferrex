//! Reusable infrastructure adapters used below the server transport layer.
//!
//! This namespace contains archive conversion helpers, cache adapters, media
//! metadata/image integrations, and external provider clients shared by server
//! and player-facing code.

/// Conversion helpers for archived persistence snapshots.
#[cfg(feature = "rkyv")]
pub mod archive;
/// On-disk cache infrastructure.
pub mod cache;
/// Media metadata, image, and provider integrations.
pub mod media;

#[cfg(feature = "rkyv")]
pub use archive::*;
pub use cache::*;
pub use media::*;
