//! HTTP request handlers organized by functionality

pub mod folder_inventory;
pub mod setup;

// Re-export commonly used handlers
pub use folder_inventory::{get_folder_inventory, get_scan_progress, trigger_folder_rescan};
pub use setup::{check_setup_status, create_initial_admin};
