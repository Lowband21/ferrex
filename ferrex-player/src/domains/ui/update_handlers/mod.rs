//! UI update handlers
//!
//! Contains specific update logic for UI-related messages

pub mod navigation_updates;
pub mod scroll_updates;
pub mod search_updates;

pub mod window_update;

// Re-export update functions
pub use navigation_updates::*;
pub use scroll_updates::*;
pub use search_updates::*;

pub use window_update::*;