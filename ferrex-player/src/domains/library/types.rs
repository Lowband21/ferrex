//! Library domain types

use uuid::Uuid;

/// Library form data for creating/editing libraries
#[derive(Debug, Clone)]
pub struct LibraryFormData {
    pub id: Uuid,
    pub name: String,
    pub library_type: String,
    pub paths: String, // comma-separated paths as entered by user
    pub scan_interval_minutes: String,
    pub enabled: bool,
    pub editing: bool, // true if editing existing library, false if creating new
}