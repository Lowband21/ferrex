//! Libraries section state (Admin)
//!
//! This will integrate with the existing library management state.

use ferrex_model::LibraryType;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Libraries management state
#[derive(Debug, Clone, Default)]
pub struct LibrariesState {
    /// List of libraries
    pub libraries: Vec<LibrarySummary>,
    /// Currently selected library for editing
    pub selected_library_id: Option<Uuid>,
    /// Whether library list is loading
    pub loading: bool,
    /// Error message from last operation
    pub error: Option<String>,
    /// Library form state (for add/edit)
    pub form: Option<LibraryFormState>,
}

/// Summary info for a library
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibrarySummary {
    pub id: Uuid,
    pub name: String,
    pub path: String,
    pub library_type: LibraryType,
    pub item_count: usize,
    pub last_scan: Option<String>,
    pub scan_in_progress: bool,
}

/// State for library add/edit form
#[derive(Debug, Clone, Default)]
pub struct LibraryFormState {
    pub id: Option<Uuid>,
    pub name: String,
    pub path: String,
    pub library_type: Option<LibraryType>,
    pub saving: bool,
    pub error: Option<String>,
}
