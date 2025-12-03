//! Libraries section messages (Admin)

use super::state::LibraryType;
use uuid::Uuid;

/// Messages for the libraries settings section
#[derive(Debug, Clone)]
pub enum LibrariesMessage {
    // Library List
    /// Load list of libraries
    LoadLibraries,
    /// Libraries loaded result
    LibrariesLoaded(Result<Vec<super::state::LibrarySummary>, String>),
    /// Select library for editing
    SelectLibrary(Uuid),
    /// Delete library
    DeleteLibrary(Uuid),
    /// Delete result
    DeleteResult(Result<Uuid, String>),

    // Scan Controls
    /// Start library scan
    StartScan(Uuid),
    /// Pause library scan
    PauseScan(Uuid),
    /// Cancel library scan
    CancelScan(Uuid),
    /// Scan status update
    ScanStatusUpdated(Uuid, ScanStatus),

    // Library Form
    /// Show add library form
    ShowAddForm,
    /// Show edit library form
    ShowEditForm(Uuid),
    /// Update form name field
    UpdateFormName(String),
    /// Update form path field
    UpdateFormPath(String),
    /// Update form library type
    UpdateFormType(LibraryType),
    /// Browse for path
    BrowseForPath,
    /// Path selected from browser
    PathSelected(Option<String>),
    /// Submit form (create or update)
    SubmitForm,
    /// Form submission result
    FormResult(Result<Uuid, String>),
    /// Cancel form
    CancelForm,
}

/// Scan status
#[derive(Debug, Clone)]
pub enum ScanStatus {
    Idle,
    Scanning { progress: f32, current_file: String },
    Paused,
    Completed { items_found: usize },
    Failed(String),
}

impl LibrariesMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::LoadLibraries => "Libraries::LoadLibraries",
            Self::LibrariesLoaded(_) => "Libraries::LibrariesLoaded",
            Self::SelectLibrary(_) => "Libraries::SelectLibrary",
            Self::DeleteLibrary(_) => "Libraries::DeleteLibrary",
            Self::DeleteResult(_) => "Libraries::DeleteResult",
            Self::StartScan(_) => "Libraries::StartScan",
            Self::PauseScan(_) => "Libraries::PauseScan",
            Self::CancelScan(_) => "Libraries::CancelScan",
            Self::ScanStatusUpdated(_, _) => "Libraries::ScanStatusUpdated",
            Self::ShowAddForm => "Libraries::ShowAddForm",
            Self::ShowEditForm(_) => "Libraries::ShowEditForm",
            Self::UpdateFormName(_) => "Libraries::UpdateFormName",
            Self::UpdateFormPath(_) => "Libraries::UpdateFormPath",
            Self::UpdateFormType(_) => "Libraries::UpdateFormType",
            Self::BrowseForPath => "Libraries::BrowseForPath",
            Self::PathSelected(_) => "Libraries::PathSelected",
            Self::SubmitForm => "Libraries::SubmitForm",
            Self::FormResult(_) => "Libraries::FormResult",
            Self::CancelForm => "Libraries::CancelForm",
        }
    }
}
