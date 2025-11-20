pub mod media_events_subscription;
pub mod scan_subscription;
pub mod subscriptions;

use crate::domains::library::media_root_browser;
use crate::infrastructure::api_types::{Library, Media, MediaID};
use ferrex_core::player_prelude::{
    LibraryID, LibraryMediaResponse, MediaFile, MediaIDLike, ScanConfig,
    ScanMetrics, ScanProgressEvent, ScanSnapshotDto,
};
use rkyv::util::AlignedVec;
use uuid::Uuid;

#[derive(Clone)]
pub enum Message {
    // Core library loading
    //TvShowsLoaded(Result<Vec<crate::domains::media::models::TvShowDetails>, String>),
    RefreshLibrary,

    // Library management
    LibrariesLoaded(Result<AlignedVec, String>),
    LoadLibraries,
    CreateLibrary {
        library: Library,
        start_scan: bool,
    },
    LibraryCreated(Result<Library, String>),
    UpdateLibrary(Library),
    LibraryUpdated(Result<Library, String>),
    DeleteLibrary(LibraryID),
    LibraryDeleted(Result<LibraryID, String>),
    SelectLibrary(Option<LibraryID>),
    LibrarySelected(Uuid, Result<Vec<MediaFile>, String>),
    ScanLibrary(LibraryID),

    // Library form management
    ShowLibraryForm(Option<Library>),
    HideLibraryForm,
    UpdateLibraryFormName(String),
    UpdateLibraryFormType(String),
    UpdateLibraryFormPaths(String),
    UpdateLibraryFormScanInterval(String),
    ToggleLibraryFormEnabled,
    ToggleLibraryFormStartScan,
    SubmitLibraryForm,
    MediaRootBrowser(media_root_browser::Message),

    // Scanning
    ScanStarted {
        library_id: LibraryID,
        scan_id: Uuid,
        correlation_id: Uuid,
    },
    ScanProgressFrame(ScanProgressEvent),
    FetchActiveScans,
    ActiveScansUpdated(Vec<ScanSnapshotDto>),
    ScanCommandFailed {
        library_id: Option<LibraryID>,
        error: String,
    },
    PauseScan {
        library_id: LibraryID,
        scan_id: Uuid,
    },
    ResumeScan {
        library_id: LibraryID,
        scan_id: Uuid,
    },
    CancelScan {
        library_id: LibraryID,
        scan_id: Uuid,
    },
    #[cfg(feature = "demo")]
    FetchDemoStatus,
    #[cfg(feature = "demo")]
    DemoStatusLoaded(
        Result<crate::infrastructure::api_types::DemoStatus, String>,
    ),
    #[cfg(feature = "demo")]
    ApplyDemoSizing(crate::infrastructure::api_types::DemoResetRequest),
    #[cfg(feature = "demo")]
    DemoSizingApplied(
        Result<crate::infrastructure::api_types::DemoStatus, String>,
    ),
    // Scanner metrics/config
    FetchScanMetrics,
    ScanMetricsLoaded(Result<ScanMetrics, String>),
    FetchScanConfig,
    ScanConfigLoaded(Result<ScanConfig, String>),

    // Destructive: delete and recreate library with start_scan=true
    ResetLibrary(LibraryID),
    ResetLibraryDone(Result<(), String>),

    // Media references
    LibraryMediasLoaded(Result<LibraryMediaResponse, String>),

    // Library operations
    RefreshCurrentLibrary,
    ScanCurrentLibrary,

    // Media events from server
    MediaDiscovered(Vec<Media>),
    MediaUpdated(Media),
    MediaDeleted(MediaID),

    // No-operation message
    NoOp,

    // Batch metadata handling
    MediaDetailsBatch(Vec<Media>),
    BatchMetadataComplete,

    // View model updates
    RefreshViewModels,
}

impl Message {
    pub fn name(&self) -> &'static str {
        match self {
            // Core library loading
            //Self::TvShowsLoaded(_) => "Library::TvShowsLoaded",
            Self::RefreshLibrary => "Library::RefreshLibrary",

            // Library management
            Self::LibrariesLoaded(_) => "Library::LibrariesLoaded",
            Self::LoadLibraries => "Library::LoadLibraries",
            Self::CreateLibrary { .. } => "Library::CreateLibrary",
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
            Self::UpdateLibraryFormPaths(_) => {
                "Library::UpdateLibraryFormPaths"
            }
            Self::UpdateLibraryFormScanInterval(_) => {
                "Library::UpdateLibraryFormScanInterval"
            }
            Self::ToggleLibraryFormEnabled => {
                "Library::ToggleLibraryFormEnabled"
            }
            Self::ToggleLibraryFormStartScan => {
                "Library::ToggleLibraryFormStartScan"
            }
            Self::SubmitLibraryForm => "Library::SubmitLibraryForm",
            Self::MediaRootBrowser(msg) => msg.name(),

            // Scanning
            Self::ScanLibrary(_) => "Library::ScanLibrary",
            Self::ScanStarted { .. } => "Library::ScanStarted",
            Self::ScanProgressFrame(_) => "Library::ScanProgressFrame",
            Self::FetchActiveScans => "Library::FetchActiveScans",
            Self::ActiveScansUpdated(_) => "Library::ActiveScansUpdated",
            Self::ScanCommandFailed { .. } => "Library::ScanCommandFailed",
            Self::PauseScan { .. } => "Library::PauseScan",
            Self::ResumeScan { .. } => "Library::ResumeScan",
            Self::CancelScan { .. } => "Library::CancelScan",
            #[cfg(feature = "demo")]
            Self::FetchDemoStatus => "Library::FetchDemoStatus",
            #[cfg(feature = "demo")]
            Self::DemoStatusLoaded(_) => "Library::DemoStatusLoaded",
            #[cfg(feature = "demo")]
            Self::ApplyDemoSizing(_) => "Library::ApplyDemoSizing",
            #[cfg(feature = "demo")]
            Self::DemoSizingApplied(_) => "Library::DemoSizingApplied",

            // Media references
            Self::LibraryMediasLoaded(_) => "Library::LibraryMediasLoaded",

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
            // Scanner diagnostics
            Self::FetchScanMetrics => "Library::FetchScanMetrics",
            Self::ScanMetricsLoaded(_) => "Library::ScanMetricsLoaded",
            Self::FetchScanConfig => "Library::FetchScanConfig",
            Self::ScanConfigLoaded(_) => "Library::ScanConfigLoaded",
            // Reset
            Self::ResetLibrary(_) => "Library::ResetLibrary",
            Self::ResetLibraryDone(_) => "Library::ResetLibraryDone",
        }
    }
}

impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Core library loading
            //Self::TvShowsLoaded(result) => match result {
            //    Ok(shows) => write!(f, "Library::TvShowsLoaded(Ok: {} shows)", shows.len()),
            //    Err(e) => write!(f, "Library::TvShowsLoaded(Err: {})", e),
            //},
            Self::RefreshLibrary => write!(f, "Library::RefreshLibrary"),

            // Library management
            Self::LibrariesLoaded(result) => match result {
                Ok(libs) => write!(
                    f,
                    "Library::LibrariesLoaded(Ok: {} libraries)",
                    libs.len()
                ),
                Err(e) => write!(f, "Library::LibrariesLoaded(Err: {})", e),
            },
            Self::LoadLibraries => write!(f, "Library::LoadLibraries"),
            Self::CreateLibrary {
                library,
                start_scan,
            } => write!(
                f,
                "Library::CreateLibrary({}, start_scan={})",
                library.name, start_scan
            ),
            Self::LibraryCreated(result) => match result {
                Ok(lib) => {
                    write!(f, "Library::LibraryCreated(Ok: {})", lib.name)
                }
                Err(e) => write!(f, "Library::LibraryCreated(Err: {})", e),
            },
            Self::UpdateLibrary(lib) => {
                write!(f, "Library::UpdateLibrary({})", lib.name)
            }
            Self::LibraryUpdated(result) => match result {
                Ok(lib) => {
                    write!(f, "Library::LibraryUpdated(Ok: {})", lib.name)
                }
                Err(e) => write!(f, "Library::LibraryUpdated(Err: {})", e),
            },
            Self::DeleteLibrary(id) => {
                write!(f, "Library::DeleteLibrary({})", id)
            }
            Self::LibraryDeleted(result) => match result {
                Ok(id) => write!(f, "Library::LibraryDeleted(Ok: {})", id),
                Err(e) => write!(f, "Library::LibraryDeleted(Err: {})", e),
            },
            Self::SelectLibrary(id) => {
                write!(f, "Library::SelectLibrary({:?})", id)
            }
            Self::LibrarySelected(id, result) => match result {
                Ok(files) => write!(
                    f,
                    "Library::LibrarySelected({}, Ok: {} files)",
                    id,
                    files.len()
                ),
                Err(e) => {
                    write!(f, "Library::LibrarySelected({}, Err: {})", id, e)
                }
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
            Self::UpdateLibraryFormType(t) => {
                write!(f, "Library::UpdateLibraryFormType({})", t)
            }
            Self::UpdateLibraryFormPaths(paths) => {
                write!(f, "Library::UpdateLibraryFormPaths({})", paths)
            }
            Self::UpdateLibraryFormScanInterval(i) => {
                write!(f, "Library::UpdateLibraryFormScanInterval({})", i)
            }
            Self::ToggleLibraryFormEnabled => {
                write!(f, "Library::ToggleLibraryFormEnabled")
            }
            Self::ToggleLibraryFormStartScan => {
                write!(f, "Library::ToggleLibraryFormStartScan")
            }
            Self::SubmitLibraryForm => write!(f, "Library::SubmitLibraryForm"),
            Self::MediaRootBrowser(inner) => {
                write!(f, "Library::MediaRootBrowser({:?})", inner)
            }

            // Scanning
            Self::ScanLibrary(_) => write!(f, "Library::ScanLibrary"),
            Self::ScanStarted {
                library_id,
                scan_id,
                correlation_id,
            } => write!(
                f,
                "Library::ScanStarted(library={}, scan={}, correlation={})",
                library_id, scan_id, correlation_id
            ),
            Self::ScanProgressFrame(frame) => write!(
                f,
                "Library::ScanProgressFrame(lib={}, seq={})",
                frame.library_id, frame.sequence
            ),
            Self::FetchActiveScans => write!(f, "Library::FetchActiveScans"),
            Self::ActiveScansUpdated(scans) => {
                write!(f, "Library::ActiveScansUpdated({} scans)", scans.len())
            }
            Self::ScanCommandFailed { library_id, error } => write!(
                f,
                "Library::ScanCommandFailed(library={:?}, error={})",
                library_id, error
            ),
            Self::PauseScan {
                library_id,
                scan_id,
            } => write!(f, "Library::PauseScan({}, {})", library_id, scan_id),
            Self::ResumeScan {
                library_id,
                scan_id,
            } => write!(f, "Library::ResumeScan({}, {})", library_id, scan_id),
            Self::CancelScan {
                library_id,
                scan_id,
            } => write!(f, "Library::CancelScan({}, {})", library_id, scan_id),
            #[cfg(feature = "demo")]
            Self::FetchDemoStatus => write!(f, "Library::FetchDemoStatus"),
            #[cfg(feature = "demo")]
            Self::DemoStatusLoaded(result) => match result {
                Ok(status) => write!(
                    f,
                    "Library::DemoStatusLoaded(Ok: {} libraries)",
                    status.libraries.len()
                ),
                Err(err) => {
                    write!(f, "Library::DemoStatusLoaded(Err: {})", err)
                }
            },
            #[cfg(feature = "demo")]
            Self::ApplyDemoSizing(_) => write!(f, "Library::ApplyDemoSizing"),
            #[cfg(feature = "demo")]
            Self::DemoSizingApplied(result) => match result {
                Ok(status) => write!(
                    f,
                    "Library::DemoSizingApplied(Ok: {} libraries)",
                    status.libraries.len()
                ),
                Err(err) => {
                    write!(f, "Library::DemoSizingApplied(Err: {})", err)
                }
            },

            // Media references
            Self::LibraryMediasLoaded(result) => match result {
                Ok(response) => write!(
                    f,
                    "Library::LibraryMediasLoaded(Ok: {:?})",
                    response.library.name
                ),
                Err(e) => write!(f, "Library::LibraryMediasLoaded(Err: {})", e),
            },

            // Library operations
            Self::RefreshCurrentLibrary => {
                write!(f, "Library::RefreshCurrentLibrary")
            }
            Self::ScanCurrentLibrary => {
                write!(f, "Library::ScanCurrentLibrary")
            }

            // Media events from server
            Self::MediaDiscovered(refs) => {
                write!(f, "Library::MediaDiscovered({} items)", refs.len())
            }
            Self::MediaUpdated(media) => match media {
                Media::Movie(m) => {
                    write!(
                        f,
                        "Library::MediaUpdated(Movie: {})",
                        m.title.as_str()
                    )
                }
                Media::Series(s) => {
                    write!(
                        f,
                        "Library::MediaUpdated(Series: {})",
                        s.title.as_str()
                    )
                }
                Media::Season(s) => {
                    let mut buf = Uuid::encode_buffer();
                    write!(
                        f,
                        "Library::MediaUpdated(Season: {})",
                        s.id.as_str(&mut buf)
                    )
                }
                Media::Episode(e) => {
                    write!(
                        f,
                        "Library::MediaUpdated(Series ID: {}, Episode: S{:02}E{:02})",
                        e.series_id,
                        e.season_number.value(),
                        e.episode_number.value()
                    )
                }
            },
            Self::MediaDeleted(id) => {
                write!(f, "Library::MediaDeleted({})", id)
            }

            // No-op
            Self::NoOp => write!(f, "Library::NoOp"),

            // Batch metadata handling
            Self::MediaDetailsBatch(batch) => {
                write!(f, "Library::MediaDetailsBatch({} items)", batch.len())
            }
            Self::BatchMetadataComplete => {
                write!(f, "Library::BatchMetadataComplete")
            }

            // View model refresh
            Self::RefreshViewModels => write!(f, "Library::RefreshViewModels"),
            // Scanner diagnostics
            Self::FetchScanMetrics => write!(f, "Library::FetchScanMetrics"),
            Self::ScanMetricsLoaded(result) => match result {
                Ok(m) => write!(
                    f,
                    "Library::ScanMetricsLoaded(Ok: active={})",
                    m.active_scans
                ),
                Err(e) => write!(f, "Library::ScanMetricsLoaded(Err: {})", e),
            },
            Self::FetchScanConfig => write!(f, "Library::FetchScanConfig"),
            Self::ScanConfigLoaded(_) => write!(f, "Library::ScanConfigLoaded"),
            // Reset
            Self::ResetLibrary(id) => {
                write!(f, "Library::ResetLibrary({})", id)
            }
            Self::ResetLibraryDone(result) => match result {
                Ok(()) => write!(f, "Library::ResetLibraryDone(Ok)"),
                Err(e) => write!(f, "Library::ResetLibraryDone(Err: {})", e),
            },
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
