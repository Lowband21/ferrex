//! UI update handlers
//!
//! Contains specific update logic for UI-related messages

#[cfg(feature = "demo")]
pub mod demo_controls;
pub mod navigation_updates;
pub mod scroll_updates;
pub mod window_update;

// Re-export update functions
#[cfg(feature = "demo")]
pub use demo_controls::*;
pub use navigation_updates::*;
pub use scroll_updates::*;
pub use window_update::*;
