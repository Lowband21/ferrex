//! Media update handlers
//!
//! Contains specific update logic for media-related messages

pub mod media_events;
pub mod media_events_library;
pub mod media_organization;
pub mod play_media;
pub mod tv_details;

// Re-export update functions
pub use media_events::*;
pub use media_events_library::*;
pub use media_organization::*;
pub use play_media::*;
pub use tv_details::*;