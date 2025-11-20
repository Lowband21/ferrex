//! HTTP request handlers organized by functionality

pub mod folder_inventory;
pub mod scan_handlers;
pub mod scan_manager;

// Re-export commonly used handlers
pub use folder_inventory::{get_folder_inventory, get_scan_progress};
