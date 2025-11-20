//! Library domain types

use ferrex_core::player_prelude::LibraryID;

/// Library form data for creating/editing libraries
#[derive(Debug, Clone)]
pub struct LibraryFormData {
    pub id: LibraryID,
    pub name: String,
    pub library_type: String,
    pub paths: String, // comma-separated paths as entered by user
    pub scan_interval_minutes: String,
    pub enabled: bool,
    pub editing: bool, // true if editing existing library, false if creating new
    pub start_scan: bool,
}
