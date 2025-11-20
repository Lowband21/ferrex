pub mod media_events_subscription;
pub mod scan_subscription;
pub mod subscriptions;

use crate::domains::media::library::MediaFile;
use crate::infrastructure::api_types::{Library, MediaReference};
use ferrex_core::api_types::{LibraryMediaResponse, ScanProgress};
use uuid::Uuid;

#[derive(Clone)]
pub enum Message {
    // Core library loading
    TvShowsLoaded(Result<Vec<crate::domains::media::models::TvShowDetails>, String>),
    RefreshLibrary,

    // Library management
    LibrariesLoaded(Result<Vec<Library>, String>),
    LoadLibraries,
    CreateLibrary(Library),
    LibraryCreated(Result<Library, String>),
    UpdateLibrary(Library),
    LibraryUpdated(Result<Library, String>),
    DeleteLibrary(Uuid),
    LibraryDeleted(Result<Uuid, String>),
    SelectLibrary(Option<Uuid>),
    LibrarySelected(Uuid, Result<Vec<MediaFile>, String>),
    ScanLibrary(Uuid),

    // Library form management
    ShowLibraryForm(Option<Library>),
    HideLibraryForm,
    UpdateLibraryFormName(String),
    UpdateLibraryFormType(String),
    UpdateLibraryFormPaths(String),
    UpdateLibraryFormScanInterval(String),
    ToggleLibraryFormEnabled,
    SubmitLibraryForm,

    // Scanning
    ScanStarted(Result<String, String>),
    ScanProgressUpdate(ScanProgress),
    ScanCompleted(Result<String, String>),
    ClearScanProgress,
    ToggleScanProgress,
    CheckActiveScans,
    ActiveScansChecked(Vec<ScanProgress>),

    // Media references
    LibraryMediaReferencesLoaded(Result<LibraryMediaResponse, String>),

    // Library operations
    RefreshCurrentLibrary,
    ScanCurrentLibrary,

    // Media events from server
    MediaDiscovered(Vec<MediaReference>),
    MediaUpdated(MediaReference),
    MediaDeleted(String), // File ID - we don't know the media type at deletion time

    // No-operation message
    NoOp,

    // Batch metadata handling
    MediaDetailsBatch(Vec<MediaReference>),
    BatchMetadataComplete,

    // View model updates
    RefreshViewModels,
}

impl Message {
    pub fn name(&self) -> &'static str {
        match self {
            // Core library loading
            Self::TvShowsLoaded(_) => "Library::TvShowsLoaded",
            Self::RefreshLibrary => "Library::RefreshLibrary",

            // Library management
            Self::LibrariesLoaded(_) => "Library::LibrariesLoaded",
            Self::LoadLibraries => "Library::LoadLibraries",
            Self::CreateLibrary(_) => "Library::CreateLibrary",
            Self::LibraryCreated(_) => "Library::LibraryCreated",
            Self::UpdateLibrary(_) => "Library::UpdateLibrary",
            Self::LibraryUpdated(_) => "Library::LibraryUpdated",
            Self::DeleteLibrary(_) => "Library::DeleteLibrary",
            Self::LibraryDeleted(_) => "Library::LibraryDeleted",
            Self::SelectLibrary(_) => "Library::SelectLibrary",
            Self::LibrarySelected(_, _) => "Library::LibrarySelected",

            // Library form management
            Self::ShowLibraryForm(_) => "Library::ShowLibraryForm",
            Self::HideLibraryForm => "Library::HideLibraryForm",
            Self::UpdateLibraryFormName(_) => "Library::UpdateLibraryFormName",
            Self::UpdateLibraryFormType(_) => "Library::UpdateLibraryFormType",
            Self::UpdateLibraryFormPaths(_) => "Library::UpdateLibraryFormPaths",
            Self::UpdateLibraryFormScanInterval(_) => "Library::UpdateLibraryFormScanInterval",
            Self::ToggleLibraryFormEnabled => "Library::ToggleLibraryFormEnabled",
            Self::SubmitLibraryForm => "Library::SubmitLibraryForm",

            // Scanning
            Self::ScanLibrary(uuid) => "Library::ScanLibrary",
            Self::ScanStarted(_) => "Library::ScanStarted",
            Self::ScanProgressUpdate(_) => "Library::ScanProgressUpdate",
            Self::ScanCompleted(_) => "Library::ScanCompleted",
            Self::ClearScanProgress => "Library::ClearScanProgress",
            Self::ToggleScanProgress => "Library::ToggleScanProgress",
            Self::CheckActiveScans => "Library::CheckActiveScans",
            Self::ActiveScansChecked(_) => "Library::ActiveScansChecked",

            // Media references
            Self::LibraryMediaReferencesLoaded(_) => "Library::LibraryMediaReferencesLoaded",

            // Library operations
            Self::RefreshCurrentLibrary => "Library::RefreshCurrentLibrary",
            Self::ScanCurrentLibrary => "Library::ScanCurrentLibrary",

            // Media events from server
            Self::MediaDiscovered(_) => "Library::MediaDiscovered",
            Self::MediaUpdated(_) => "Library::MediaUpdated",
            Self::MediaDeleted(_) => "Library::MediaDeleted",

            // No-op
            Self::NoOp => "Library::NoOp",

            // Batch metadata handling
            Self::MediaDetailsBatch(_) => "Library::MediaDetailsBatch",
            Self::BatchMetadataComplete => "Library::BatchMetadataComplete",

            // View model updates
            Self::RefreshViewModels => "Library::RefreshViewModels",
        }
    }
}

impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Core library loading
            Self::TvShowsLoaded(result) => match result {
                Ok(shows) => write!(f, "Library::TvShowsLoaded(Ok: {} shows)", shows.len()),
                Err(e) => write!(f, "Library::TvShowsLoaded(Err: {})", e),
            },
            Self::RefreshLibrary => write!(f, "Library::RefreshLibrary"),

            // Library management
            Self::LibrariesLoaded(result) => match result {
                Ok(libs) => write!(f, "Library::LibrariesLoaded(Ok: {} libraries)", libs.len()),
                Err(e) => write!(f, "Library::LibrariesLoaded(Err: {})", e),
            },
            Self::LoadLibraries => write!(f, "Library::LoadLibraries"),
            Self::CreateLibrary(lib) => write!(f, "Library::CreateLibrary({})", lib.name),
            Self::LibraryCreated(result) => match result {
                Ok(lib) => write!(f, "Library::LibraryCreated(Ok: {})", lib.name),
                Err(e) => write!(f, "Library::LibraryCreated(Err: {})", e),
            },
            Self::UpdateLibrary(lib) => write!(f, "Library::UpdateLibrary({})", lib.name),
            Self::LibraryUpdated(result) => match result {
                Ok(lib) => write!(f, "Library::LibraryUpdated(Ok: {})", lib.name),
                Err(e) => write!(f, "Library::LibraryUpdated(Err: {})", e),
            },
            Self::DeleteLibrary(id) => write!(f, "Library::DeleteLibrary({})", id),
            Self::LibraryDeleted(result) => match result {
                Ok(id) => write!(f, "Library::LibraryDeleted(Ok: {})", id),
                Err(e) => write!(f, "Library::LibraryDeleted(Err: {})", e),
            },
            Self::SelectLibrary(id) => write!(f, "Library::SelectLibrary({:?})", id),
            Self::LibrarySelected(id, result) => match result {
                Ok(files) => write!(
                    f,
                    "Library::LibrarySelected({}, Ok: {} files)",
                    id,
                    files.len()
                ),
                Err(e) => write!(f, "Library::LibrarySelected({}, Err: {})", id, e),
            },

            // Library form management
            Self::ShowLibraryForm(lib) => {
                if let Some(l) = lib {
                    write!(f, "Library::ShowLibraryForm(Some: {})", l.name)
                } else {
                    write!(f, "Library::ShowLibraryForm(None)")
                }
            }
            Self::HideLibraryForm => write!(f, "Library::HideLibraryForm"),
            Self::UpdateLibraryFormName(name) => {
                write!(f, "Library::UpdateLibraryFormName({})", name)
            }
            Self::UpdateLibraryFormType(t) => write!(f, "Library::UpdateLibraryFormType({})", t),
            Self::UpdateLibraryFormPaths(paths) => {
                write!(f, "Library::UpdateLibraryFormPaths({})", paths)
            }
            Self::UpdateLibraryFormScanInterval(i) => {
                write!(f, "Library::UpdateLibraryFormScanInterval({})", i)
            }
            Self::ToggleLibraryFormEnabled => write!(f, "Library::ToggleLibraryFormEnabled"),
            Self::SubmitLibraryForm => write!(f, "Library::SubmitLibraryForm"),

            // Scanning
            Self::ScanLibrary(uuid) => write!(f, "Library::ScanLibrary"),
            Self::ScanStarted(result) => match result {
                Ok(scan_id) => write!(f, "Library::ScanStarted(Ok: {})", scan_id),
                Err(e) => write!(f, "Library::ScanStarted(Err: {})", e),
            },
            Self::ScanProgressUpdate(progress) => {
                write!(f, "Library::ScanProgressUpdate({:?})", progress.status)
            }
            Self::ScanCompleted(result) => match result {
                Ok(scan_id) => write!(f, "Library::ScanCompleted(Ok: {})", scan_id),
                Err(e) => write!(f, "Library::ScanCompleted(Err: {})", e),
            },
            Self::ClearScanProgress => write!(f, "Library::ClearScanProgress"),
            Self::ToggleScanProgress => write!(f, "Library::ToggleScanProgress"),
            Self::CheckActiveScans => write!(f, "Library::CheckActiveScans"),
            Self::ActiveScansChecked(scans) => {
                write!(f, "Library::ActiveScansChecked({} scans)", scans.len())
            }

            // Media references
            Self::LibraryMediaReferencesLoaded(result) => match result {
                Ok(response) => write!(
                    f,
                    "Library::LibraryMediaReferencesLoaded(Ok: {:?})",
                    response.library.name
                ),
                Err(e) => write!(f, "Library::LibraryMediaReferencesLoaded(Err: {})", e),
            },

            // Library operations
            Self::RefreshCurrentLibrary => write!(f, "Library::RefreshCurrentLibrary"),
            Self::ScanCurrentLibrary => write!(f, "Library::ScanCurrentLibrary"),

            // Media events from server
            Self::MediaDiscovered(refs) => {
                write!(f, "Library::MediaDiscovered({} items)", refs.len())
            }
            Self::MediaUpdated(media) => match media {
                MediaReference::Movie(m) => {
                    write!(f, "Library::MediaUpdated(Movie: {})", m.title.as_str())
                }
                MediaReference::Series(s) => {
                    write!(f, "Library::MediaUpdated(Series: {})", s.title.as_str())
                }
                MediaReference::Season(s) => {
                    write!(f, "Library::MediaUpdated(Season: {})", s.id.as_str())
                }
                MediaReference::Episode(e) => {
                    write!(
                        f,
                        "Library::MediaUpdated(Series ID: {}, Episode: S{:02}E{:02})",
                        e.series_id,
                        e.season_number.value(),
                        e.episode_number.value()
                    )
                }
            },
            Self::MediaDeleted(id) => write!(f, "Library::MediaDeleted({})", id),

            // No-op
            Self::NoOp => write!(f, "Library::NoOp"),

            // Batch metadata handling
            Self::MediaDetailsBatch(batch) => {
                write!(f, "Library::MediaDetailsBatch({} items)", batch.len())
            }
            Self::BatchMetadataComplete => write!(f, "Library::BatchMetadataComplete"),

            // View model refresh
            Self::RefreshViewModels => write!(f, "Library::RefreshViewModels"),
        }
    }
}

/// Cross-domain events that library domain can emit
#[derive(Clone, Debug)]
pub enum LibraryEvent {
    LibraryCreated(Library),
    LibraryUpdated(Library),
    LibraryDeleted(Uuid),
    LibrarySelected(Uuid),
    ScanStarted(String),   // scan_id
    ScanCompleted(String), // scan_id
    MediaDiscovered(Vec<MediaFile>),
}
