//! On-disk cache helpers for the player.
//!
//! The player uses a `cacache`-backed content-addressed store (via `ferrex-core`)
//! to persist image bytes on disk and avoid unbounded RAM growth over time.

pub mod iced_image_handle;
pub mod image_disk_cache;
pub mod media_repo_disk_cache;

pub use iced_image_handle::*;
pub use image_disk_cache::*;
pub use media_repo_disk_cache::*;
