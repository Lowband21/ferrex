//! Libraries section state (Admin)
//!
//! This will integrate with the existing library management state.

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

/// Library type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LibraryType {
    Movies,
    TvShows,
    Music,
    Photos,
    Mixed,
}

impl std::fmt::Display for LibraryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Movies => write!(f, "Movies"),
            Self::TvShows => write!(f, "TV Shows"),
            Self::Music => write!(f, "Music"),
            Self::Photos => write!(f, "Photos"),
            Self::Mixed => write!(f, "Mixed"),
        }
    }
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
