//! Library update handlers
//!
//! Contains specific update logic for library-related messages

pub mod library_loaded;
pub mod library_management;
pub mod refresh_library;
pub mod scan_updates;
pub mod select_library;

// Re-export update functions
pub use library_loaded::*;
pub use library_management::*;
pub use refresh_library::*;
pub use scan_updates::*;
pub use select_library::*;
