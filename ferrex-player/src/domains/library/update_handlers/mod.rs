//! Library update handlers
//!
//! Contains specific update logic for library-related messages

#[cfg(feature = "demo")]
pub mod demo_controls;
pub mod library_loaded;
pub mod library_management;
pub mod media_events;
pub mod media_root_browser;
pub mod refresh_library;
pub mod scan_updates;
pub mod select_library;

// Re-export update functions
#[cfg(feature = "demo")]
pub use demo_controls::*;
pub use library_loaded::*;
pub use library_management::*;
pub use media_events::*;
pub use media_root_browser::*;
pub use refresh_library::*;
pub use scan_updates::*;
pub use select_library::*;
