use std::collections::HashMap;

use ferrex_core::{EpisodeID, MediaId, SeasonID, SeriesID, SeriesReference};
use iced::{widget::scrollable, Point};
use iced_video_player::{ToneMappingAlgorithm, ToneMappingPreset};
use uuid::Uuid;

use crate::{
    api_types::{LibraryMediaResponse, MediaReference, MovieReference, ScanProgress},
    media_library::{Library, MediaFile},
    models::{SeasonDetails, TvShow, TvShowDetails},
    player,
    state::{SortBy, ViewMode},
    views::carousel::CarouselMessage,
};

#[derive(Clone)]
pub enum Message {
    // Library messages
    LibraryLoaded(Result<Vec<MediaFile>, String>),
    MoviesLoaded(Result<Vec<crate::models::MovieDetails>, String>),
    TvShowsLoaded(Result<Vec<crate::models::TvShowDetails>, String>),
    RefreshLibrary,

    // Library management
    LibrariesLoaded(Result<Vec<Library>, String>),
    LoadLibraries,
    CreateLibrary(Library),
    LibraryCreated(Result<Library, String>),
    UpdateLibrary(Library),
    LibraryUpdated(Result<Library, String>),
    DeleteLibrary(Uuid),                                   // library_id
    LibraryDeleted(Result<Uuid, String>),                  // library_id or error
    SelectLibrary(Option<Uuid>),                           // library_id
    LibrarySelected(Uuid, Result<Vec<MediaFile>, String>), // library_id, media_files
    ScanLibrary_(Uuid), // library_id - renamed to avoid conflict with existing ScanLibrary

    // Library form management
    ShowLibraryForm(Option<Library>), // None for create, Some(library) for edit
    HideLibraryForm,
    UpdateLibraryFormName(String),
    UpdateLibraryFormType(String),
    UpdateLibraryFormPaths(String), // comma-separated paths
    UpdateLibraryFormScanInterval(String),
    ToggleLibraryFormEnabled,
    SubmitLibraryForm,

    // View management
    ShowLibraryManagement,
    HideLibraryManagement,
    ShowAdminDashboard,
    HideAdminDashboard,
    PlayMedia(MediaFile),
    PlayMediaWithId(MediaFile, ferrex_core::api_types::MediaId), // New: includes MediaId for watch tracking
    ViewDetails(MediaFile),
    ViewMovieDetails(MovieReference), // New efficient movie detail navigation
    ViewTvShow(SeriesID),             // series_id
    ViewSeason(SeriesID, SeasonID),   // series_id, season_id
    ViewEpisode(EpisodeID),           // episode_id
    SetViewMode(ViewMode),            // Switch between All/Movies/TV Shows
    SetSortBy(SortBy),                // Change sort field
    ToggleSortOrder,                  // Toggle ascending/descending
    ScanLibrary,                      // Legacy - scans all libraries
    ScanStarted(Result<String, String>), // scan_id or error
    ScanProgressUpdate(ScanProgress),
    ScanCompleted(Result<String, String>),
    ForceRescan,
    ClearScanProgress,
    ToggleScanProgress,                    // Toggle scan progress overlay
    CheckActiveScans,                      // Check for active scans on startup
    ActiveScansChecked(Vec<ScanProgress>), // Result of active scans check


    // Background organization
    MediaOrganized(Vec<MediaFile>, HashMap<String, TvShow>),

    // Animation and transition messages
    UpdateTransitions, // Update color and backdrop transitions

    // Virtual scrolling messages
    MoviesGridScrolled(scrollable::Viewport),
    TvShowsGridScrolled(scrollable::Viewport),
    CheckScrollStopped,                       // Check if scrolling has stopped
    RecalculateGridsAfterResize,              // Recalculate grid states after window resize
    DetailViewScrolled(scrollable::Viewport), // Scroll events in detail views

    // TV show data loading
    TvShowLoaded(String, Result<TvShowDetails, String>), // show_name, result
    SeasonLoaded(String, u32, Result<SeasonDetails, String>), // show_name, season_num, result

    // Player messages
    BackToLibrary,
    Play,
    Pause,
    PlayPause,
    Seek(f64),
    SeekRelative(f64),
    SeekRelease,
    SeekBarPressed,
    SeekBarMoved(Point),
    SeekDone,
    SetVolume(f64),
    EndOfStream,
    // MissingPlugin(gstreamer::Message), // Not available in standard iced_video_player
    NewFrame,
    Reload,
    ShowControls,

    // New player controls
    Stop,
    ToggleFullscreen,
    ExitFullscreen,
    ToggleMute,
    SetPlaybackSpeed(f64),
    ToggleSettings,
    SetAspectRatio(player::state::AspectRatio),
    SeekForward,  // +15s
    SeekBackward, // -15s
    MouseMoved,
    VideoClicked,
    VideoDoubleClicked,

    // Track selection
    AudioTrackSelected(i32),
    SubtitleTrackSelected(Option<i32>),

    // Tone mapping controls
    ToggleToneMapping(bool),
    SetToneMappingPreset(ToneMappingPreset),
    SetToneMappingAlgorithm(ToneMappingAlgorithm),
    SetToneMappingWhitePoint(f32),
    SetToneMappingExposure(f32),
    SetToneMappingSaturation(f32),
    SetHableShoulderStrength(f32),
    SetHableLinearStrength(f32),
    SetHableLinearAngle(f32),
    SetHableToeStrength(f32),
    SetMonitorBrightness(f32),
    SetToneMappingBrightness(f32),
    SetToneMappingContrast(f32),
    SetToneMappingSaturationBoost(f32),

    // No operation message
    NoOp,
    InitializeMetadataService,
    ToggleSubtitles,
    ToggleSubtitleMenu,
    ToggleQualityMenu,
    CycleAudioTrack,
    CycleSubtitleTrack,
    CycleSubtitleSimple, // New: Simple subtitle cycling for left-click
    TracksLoaded,

    // General
    ClearError,
    Tick,

    // Media availability
    MediaAvailabilityChecked(MediaFile),
    MediaUnavailable(String, String), // reason, message

    // Video loading
    VideoLoaded(bool), // Success flag - video is stored in state temporarily
    VideoCreated(Result<std::sync::Arc<iced_video_player::Video>, String>), // Async video creation result with video wrapped in Arc

    // Transcoding and streaming
    TranscodingStarted(Result<String, String>), // job_id or error
    TranscodingStatusUpdate(
        Result<
            (
                crate::player::state::TranscodingStatus,
                Option<f64>,
                Option<String>,
            ),
            String,
        >,
    ),
    CheckTranscodingStatus,      // Periodic check for transcoding status
    StartSegmentPrefetch(usize), // Start prefetching segment at index
    SegmentPrefetched(usize, Result<Vec<u8>, String>), // segment index, data or error
    QualityVariantSelected(String), // profile name
    BandwidthMeasured(u64),      // bits per second
    MasterPlaylistLoaded(Option<crate::server::hls::MasterPlaylist>), // Master playlist from server
    MasterPlaylistReady(Option<crate::server::hls::MasterPlaylist>), // Master playlist exists and ready for playback

    // Image loading will be handled by UnifiedImageService

    // Image loading (generic)
    ImageLoaded(String, Result<Vec<u8>, String>), // cache_key, result

    // Unified image loading
    UnifiedImageLoaded(
        crate::image_types::ImageRequest,
        iced::widget::image::Handle,
    ),
    UnifiedImageLoadFailed(crate::image_types::ImageRequest, String),

    // Update backdrop handle for detail views
    UpdateBackdropHandle(iced::widget::image::Handle),

    RefreshShowMetadata(SeriesID), // Refresh metadata for all episodes in a show
    RefreshSeasonMetadata(SeasonID, u32), // Refresh metadata for all episodes in a season
    RefreshEpisodeMetadata(EpisodeID), // Refresh metadata for a single episode

    // New reference-based messages
    LibraryMediaReferencesLoaded(Result<LibraryMediaResponse, String>),
    AllLibrariesLoaded(Vec<(Uuid, Result<LibraryMediaResponse, String>)>), // Parallel load results
    MediaDetailsUpdated(MediaReference), // Full details fetched for a media item
    MediaDetailsBatch(Vec<MediaReference>), // Batch of media details for efficient processing
    QueueVisibleDetailsForFetch,         // Queue visible items for background detail fetching
    SeriesSortingCompleted(Vec<SeriesReference>), // Series sorted in background
    CheckDetailsFetcherQueue,            // Check if background fetcher has completed items

    // Carousel navigation
    CarouselNavigation(CarouselMessage),

    // Window events
    WindowResized(iced::Size),

    // Hover events
    MediaHovered(String),   // media_id
    MediaUnhovered(String), // media_id being unhovered

    // TV show metadata refresh
    ShowMetadataRefreshed(String),             // show_name
    ShowMetadataRefreshFailed(String, String), // show_name, error

    // Database maintenance
    ShowClearDatabaseConfirm,
    HideClearDatabaseConfirm,
    ClearDatabase,
    DatabaseCleared(Result<(), String>),

    // Backdrop aspect ratio control
    ToggleBackdropAspectMode,
    
    // Debounced ViewModel refresh
    RefreshViewModels,

    // Header navigation
    NavigateHome,
    UpdateSearchQuery(String),
    ExecuteSearch,
    ShowLibraryMenu,
    ShowAllLibrariesMenu,
    RefreshCurrentLibrary,
    ScanCurrentLibrary,
    ShowProfile,

    // Library aggregation
    AggregateAllLibraries,
    
    // Batch metadata completion
    BatchMetadataComplete,
    
    // Authentication messages
    CheckAuthStatus,
    AuthStatusConfirmedWithPin,
    CheckSetupStatus,
    SetupStatusChecked(bool),  // needs_setup
    LoadUsers,
    UsersLoaded(Result<Vec<ferrex_core::user::User>, String>),
    SelectUser(Uuid),
    ShowCreateUser,
    BackToUserSelection,
    ShowPinEntry(ferrex_core::user::User),
    PinDigitPressed(char),
    PinBackspace,
    PinClear,
    PinSubmit,
    LoginSuccess(ferrex_core::user::User, ferrex_core::rbac::UserPermissions),
    LoginError(String),
    WatchStatusLoaded(Result<ferrex_core::watch_status::UserWatchState, String>),
    Logout,
    LogoutComplete,
    
    // Password login messages
    ShowPasswordLogin(String), // username
    PasswordLoginUpdateUsername(String),
    PasswordLoginUpdatePassword(String),
    PasswordLoginToggleVisibility,
    PasswordLoginToggleRemember,
    PasswordLoginSubmit,
    
    // Device authentication flow messages
    AuthFlowDeviceStatusChecked(ferrex_core::user::User, Result<crate::auth::DeviceAuthStatus, String>),
    AuthFlowUpdateCredential(String),
    AuthFlowSubmitCredential,
    AuthFlowTogglePasswordVisibility,
    AuthFlowToggleRememberDevice,
    AuthFlowAuthResult(Result<crate::auth::PlayerAuthResult, String>),
    AuthFlowSetupPin,
    AuthFlowUpdatePin(String),
    AuthFlowUpdateConfirmPin(String),
    AuthFlowSubmitPin,
    AuthFlowPinSet(Result<(), String>),
    AuthFlowRetry,
    AuthFlowBack,
    
    // First-run setup messages
    FirstRunUpdateUsername(String),
    FirstRunUpdateDisplayName(String),
    FirstRunUpdatePassword(String),
    FirstRunUpdateConfirmPassword(String),
    FirstRunTogglePasswordVisibility,
    FirstRunSubmit,
    FirstRunSuccess,
    FirstRunError(String),
}

impl Message {
    /// Get a human-readable name for this message variant for profiling
    pub fn name(&self) -> &'static str {
        match self {
            // Library messages
            Message::LibraryLoaded(_) => "LibraryLoaded",
            Message::MoviesLoaded(_) => "MoviesLoaded",
            Message::TvShowsLoaded(_) => "TvShowsLoaded",
            Message::RefreshLibrary => "RefreshLibrary",

            // Library management
            Message::LibrariesLoaded(_) => "LibrariesLoaded",
            Message::LoadLibraries => "LoadLibraries",
            Message::CreateLibrary(_) => "CreateLibrary",
            Message::LibraryCreated(_) => "LibraryCreated",
            Message::UpdateLibrary(_) => "UpdateLibrary",
            Message::LibraryUpdated(_) => "LibraryUpdated",
            Message::DeleteLibrary(_) => "DeleteLibrary",
            Message::LibraryDeleted(_) => "LibraryDeleted",
            Message::SelectLibrary(_) => "SelectLibrary",
            Message::LibrarySelected(_, _) => "LibrarySelected",
            Message::ScanLibrary_(_) => "ScanLibrary_",

            // Library form management
            Message::ShowLibraryForm(_) => "ShowLibraryForm",
            Message::HideLibraryForm => "HideLibraryForm",
            Message::UpdateLibraryFormName(_) => "UpdateLibraryFormName",
            Message::UpdateLibraryFormType(_) => "UpdateLibraryFormType",
            Message::UpdateLibraryFormPaths(_) => "UpdateLibraryFormPaths",
            Message::UpdateLibraryFormScanInterval(_) => "UpdateLibraryFormScanInterval",
            Message::ToggleLibraryFormEnabled => "ToggleLibraryFormEnabled",
            Message::SubmitLibraryForm => "SubmitLibraryForm",

            // View management
            Message::ShowLibraryManagement => "ShowLibraryManagement",
            Message::HideLibraryManagement => "HideLibraryManagement",
            Message::ShowAdminDashboard => "ShowAdminDashboard",
            Message::HideAdminDashboard => "HideAdminDashboard",
            Message::PlayMedia(_) => "PlayMedia",
            Message::PlayMediaWithId(_, _) => "PlayMediaWithId",
            Message::ViewDetails(_) => "ViewDetails",
            Message::ViewMovieDetails(_) => "ViewMovieDetails",
            Message::ViewTvShow(_) => "ViewTvShow",
            Message::ViewSeason(_, _) => "ViewSeason",
            Message::ViewEpisode(_) => "ViewEpisode",
            Message::SetViewMode(_) => "SetViewMode",
            Message::SetSortBy(_) => "SetSortBy",
            Message::ToggleSortOrder => "ToggleSortOrder",
            Message::ScanLibrary => "ScanLibrary",
            Message::ScanStarted(_) => "ScanStarted",
            Message::ScanProgressUpdate(_) => "ScanProgressUpdate",
            Message::ScanCompleted(_) => "ScanCompleted",
            Message::ForceRescan => "ForceRescan",
            Message::ClearScanProgress => "ClearScanProgress",
            Message::ToggleScanProgress => "ToggleScanProgress",
            Message::CheckActiveScans => "CheckActiveScans",
            Message::ActiveScansChecked(_) => "ActiveScansChecked",


            // Background organization
            Message::MediaOrganized(_, _) => "MediaOrganized",

            // Animation and transition messages
            Message::UpdateTransitions => "UpdateTransitions",

            // Virtual scrolling messages
            Message::MoviesGridScrolled(_) => "MoviesGridScrolled",
            Message::TvShowsGridScrolled(_) => "TvShowsGridScrolled",
            Message::DetailViewScrolled(_) => "DetailViewScrolled",
            Message::CheckScrollStopped => "CheckScrollStopped",
            Message::RecalculateGridsAfterResize => "RecalculateGridsAfterResize",

            // TV show data loading
            Message::TvShowLoaded(_, _) => "TvShowLoaded",
            Message::SeasonLoaded(_, _, _) => "SeasonLoaded",

            // Player messages
            Message::BackToLibrary => "BackToLibrary",
            Message::Play => "Play",
            Message::Pause => "Pause",
            Message::PlayPause => "PlayPause",
            Message::Seek(_) => "Seek",
            Message::SeekRelative(_) => "SeekRelative",
            Message::SeekRelease => "SeekRelease",
            Message::SeekBarPressed => "SeekBarPressed",
            Message::SeekBarMoved(_) => "SeekBarMoved",
            Message::SeekDone => "SeekDone",
            Message::SetVolume(_) => "SetVolume",
            Message::EndOfStream => "EndOfStream",
            Message::NewFrame => "NewFrame",
            Message::Reload => "Reload",
            Message::ShowControls => "ShowControls",

            // New player controls
            Message::Stop => "Stop",
            Message::ToggleFullscreen => "ToggleFullscreen",
            Message::ExitFullscreen => "ExitFullscreen",
            Message::ToggleMute => "ToggleMute",
            Message::SetPlaybackSpeed(_) => "SetPlaybackSpeed",
            Message::ToggleSettings => "ToggleSettings",
            Message::SetAspectRatio(_) => "SetAspectRatio",
            Message::SeekForward => "SeekForward",
            Message::SeekBackward => "SeekBackward",
            Message::MouseMoved => "MouseMoved",
            Message::VideoClicked => "VideoClicked",
            Message::VideoDoubleClicked => "VideoDoubleClicked",

            // Track selection
            Message::AudioTrackSelected(_) => "AudioTrackSelected",
            Message::SubtitleTrackSelected(_) => "SubtitleTrackSelected",

            // Tone mapping controls
            Message::ToggleToneMapping(_) => "ToggleToneMapping",
            Message::SetToneMappingPreset(_) => "SetToneMappingPreset",
            Message::SetToneMappingAlgorithm(_) => "SetToneMappingAlgorithm",
            Message::SetToneMappingWhitePoint(_) => "SetToneMappingWhitePoint",
            Message::SetToneMappingExposure(_) => "SetToneMappingExposure",
            Message::SetToneMappingSaturation(_) => "SetToneMappingSaturation",
            Message::SetHableShoulderStrength(_) => "SetHableShoulderStrength",
            Message::SetHableLinearStrength(_) => "SetHableLinearStrength",
            Message::SetHableLinearAngle(_) => "SetHableLinearAngle",
            Message::SetHableToeStrength(_) => "SetHableToeStrength",
            Message::SetMonitorBrightness(_) => "SetMonitorBrightness",
            Message::SetToneMappingBrightness(_) => "SetToneMappingBrightness",
            Message::SetToneMappingContrast(_) => "SetToneMappingContrast",
            Message::SetToneMappingSaturationBoost(_) => "SetToneMappingSaturationBoost",

            // No operation message
            Message::NoOp => "NoOp",
            Message::InitializeMetadataService => "InitializeMetadataService",
            Message::ToggleSubtitles => "ToggleSubtitles",
            Message::ToggleSubtitleMenu => "ToggleSubtitleMenu",
            Message::ToggleQualityMenu => "ToggleQualityMenu",
            Message::CycleAudioTrack => "CycleAudioTrack",
            Message::CycleSubtitleTrack => "CycleSubtitleTrack",
            Message::CycleSubtitleSimple => "CycleSubtitleSimple",
            Message::TracksLoaded => "TracksLoaded",

            // General
            Message::ClearError => "ClearError",
            Message::Tick => "Tick",

            // Media availability
            Message::MediaAvailabilityChecked(_) => "MediaAvailabilityChecked",
            Message::MediaUnavailable(_, _) => "MediaUnavailable",

            // Video loading
            Message::VideoLoaded(_) => "VideoLoaded",
            Message::VideoCreated(_) => "VideoCreated",

            // Transcoding and streaming
            Message::TranscodingStarted(_) => "TranscodingStarted",
            Message::TranscodingStatusUpdate(_) => "TranscodingStatusUpdate",
            Message::CheckTranscodingStatus => "CheckTranscodingStatus",
            Message::StartSegmentPrefetch(_) => "StartSegmentPrefetch",
            Message::SegmentPrefetched(_, _) => "SegmentPrefetched",
            Message::QualityVariantSelected(_) => "QualityVariantSelected",
            Message::BandwidthMeasured(_) => "BandwidthMeasured",
            Message::MasterPlaylistLoaded(_) => "MasterPlaylistLoaded",
            Message::MasterPlaylistReady(_) => "MasterPlaylistReady",

            // Image loading (generic)
            Message::ImageLoaded(_, _) => "ImageLoaded",
            Message::UnifiedImageLoaded(_, _) => "UnifiedImageLoaded",
            Message::UnifiedImageLoadFailed(_, _) => "UnifiedImageLoadFailed",
            Message::UpdateBackdropHandle(_) => "UpdateBackdropHandle",

            Message::RefreshShowMetadata(_) => "RefreshShowMetadata",
            Message::RefreshSeasonMetadata(_, _) => "RefreshSeasonMetadata",
            Message::RefreshEpisodeMetadata(_) => "RefreshEpisodeMetadata",

            // Carousel navigation
            Message::CarouselNavigation(_) => "CarouselNavigation",

            // Window events
            Message::WindowResized(_) => "WindowResized",

            // Hover events
            Message::MediaHovered(_) => "MediaHovered",
            Message::MediaUnhovered(_) => "MediaUnhovered",

            // TV show metadata refresh
            Message::ShowMetadataRefreshed(_) => "ShowMetadataRefreshed",
            Message::ShowMetadataRefreshFailed(_, _) => "ShowMetadataRefreshFailed",

            // Database maintenance
            Message::ShowClearDatabaseConfirm => "ShowClearDatabaseConfirm",
            Message::HideClearDatabaseConfirm => "HideClearDatabaseConfirm",
            Message::ClearDatabase => "ClearDatabase",
            Message::DatabaseCleared(_) => "DatabaseCleared",

            // New reference-based messages
            Message::LibraryMediaReferencesLoaded(_) => "LibraryMediaReferencesLoaded",
            Message::AllLibrariesLoaded(_) => "AllLibrariesLoaded",
            Message::MediaDetailsUpdated(_) => "MediaDetailsUpdated",
            Message::MediaDetailsBatch(_) => "MediaDetailsBatch",
            Message::QueueVisibleDetailsForFetch => "QueueVisibleDetailsForFetch",
            Message::SeriesSortingCompleted(_) => "SeriesSortingCompleted",
            Message::CheckDetailsFetcherQueue => "CheckDetailsFetcherQueue",

            // Backdrop aspect ratio control
            Message::ToggleBackdropAspectMode => "ToggleBackdropAspectMode",
            Message::RefreshViewModels => "RefreshViewModels",

            // Header navigation
            Message::NavigateHome => "NavigateHome",
            Message::UpdateSearchQuery(_) => "UpdateSearchQuery",
            Message::ExecuteSearch => "ExecuteSearch",
            Message::ShowLibraryMenu => "ShowLibraryMenu",
            Message::ShowAllLibrariesMenu => "ShowAllLibrariesMenu",
            Message::RefreshCurrentLibrary => "RefreshCurrentLibrary",
            Message::ScanCurrentLibrary => "ScanCurrentLibrary",
            Message::ShowProfile => "ShowProfile",
            Message::AggregateAllLibraries => "AggregateAllLibraries",
            Message::BatchMetadataComplete => "BatchMetadataComplete",
            
            // Authentication messages
            Message::CheckAuthStatus => "CheckAuthStatus",
            Message::AuthStatusConfirmedWithPin => "AuthStatusConfirmedWithPin",
            Message::CheckSetupStatus => "CheckSetupStatus",
            Message::SetupStatusChecked(_) => "SetupStatusChecked",
            Message::LoadUsers => "LoadUsers",
            Message::UsersLoaded(_) => "UsersLoaded",
            Message::SelectUser(_) => "SelectUser",
            Message::ShowCreateUser => "ShowCreateUser",
            Message::BackToUserSelection => "BackToUserSelection",
            Message::ShowPinEntry(_) => "ShowPinEntry",
            Message::PinDigitPressed(_) => "PinDigitPressed",
            Message::PinBackspace => "PinBackspace",
            Message::PinClear => "PinClear",
            Message::PinSubmit => "PinSubmit",
            Message::LoginSuccess(_, _) => "LoginSuccess",
            Message::LoginError(_) => "LoginError",
            Message::WatchStatusLoaded(_) => "WatchStatusLoaded",
            Message::Logout => "Logout",
            Message::LogoutComplete => "LogoutComplete",
            
            // Password login messages
            Message::ShowPasswordLogin(_) => "ShowPasswordLogin",
            Message::PasswordLoginUpdateUsername(_) => "PasswordLoginUpdateUsername",
            Message::PasswordLoginUpdatePassword(_) => "PasswordLoginUpdatePassword",
            Message::PasswordLoginToggleVisibility => "PasswordLoginToggleVisibility",
            Message::PasswordLoginToggleRemember => "PasswordLoginToggleRemember",
            Message::PasswordLoginSubmit => "PasswordLoginSubmit",
            
            // Device authentication flow messages
            Message::AuthFlowDeviceStatusChecked(_, _) => "AuthFlowDeviceStatusChecked",
            Message::AuthFlowUpdateCredential(_) => "AuthFlowUpdateCredential",
            Message::AuthFlowSubmitCredential => "AuthFlowSubmitCredential",
            Message::AuthFlowTogglePasswordVisibility => "AuthFlowTogglePasswordVisibility",
            Message::AuthFlowToggleRememberDevice => "AuthFlowToggleRememberDevice",
            Message::AuthFlowAuthResult(_) => "AuthFlowAuthResult",
            Message::AuthFlowSetupPin => "AuthFlowSetupPin",
            Message::AuthFlowUpdatePin(_) => "AuthFlowUpdatePin",
            Message::AuthFlowUpdateConfirmPin(_) => "AuthFlowUpdateConfirmPin",
            Message::AuthFlowSubmitPin => "AuthFlowSubmitPin",
            Message::AuthFlowPinSet(_) => "AuthFlowPinSet",
            Message::AuthFlowRetry => "AuthFlowRetry",
            Message::AuthFlowBack => "AuthFlowBack",
            
            // First-run setup messages
            Message::FirstRunUpdateUsername(_) => "FirstRunUpdateUsername",
            Message::FirstRunUpdateDisplayName(_) => "FirstRunUpdateDisplayName",
            Message::FirstRunUpdatePassword(_) => "FirstRunUpdatePassword",
            Message::FirstRunUpdateConfirmPassword(_) => "FirstRunUpdateConfirmPassword",
            Message::FirstRunTogglePasswordVisibility => "FirstRunTogglePasswordVisibility",
            Message::FirstRunSubmit => "FirstRunSubmit",
            Message::FirstRunSuccess => "FirstRunSuccess",
            Message::FirstRunError(_) => "FirstRunError",
        }
    }
}

// Custom Debug implementation to avoid printing large image data
impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Special handling for ImageLoaded
            Message::ImageLoaded(cache_key, result) => f
                .debug_struct("ImageLoaded")
                .field("cache_key", cache_key)
                .field(
                    "result",
                    &match result {
                        Ok(bytes) => format!("Ok(<{} bytes>)", bytes.len()),
                        Err(e) => format!("Err({})", e),
                    },
                )
                .finish(),
            // Unified image messages
            Message::UnifiedImageLoaded(request, _handle) => f
                .debug_struct("UnifiedImageLoaded")
                .field("request", request)
                .field("handle", &"<image_handle>")
                .finish(),
            Message::UnifiedImageLoadFailed(request, error) => f
                .debug_struct("UnifiedImageLoadFailed")
                .field("request", request)
                .field("error", error)
                .finish(),
            Message::UpdateBackdropHandle(_handle) => f
                .debug_struct("UpdateBackdropHandle")
                .field("handle", &"<image_handle>")
                .finish(),
            // For all other variants, use default Debug formatting
            Message::LibraryLoaded(r) => f.debug_tuple("LibraryLoaded").field(r).finish(),
            Message::MoviesLoaded(r) => f.debug_tuple("MoviesLoaded").field(r).finish(),
            Message::TvShowsLoaded(r) => f.debug_tuple("TvShowsLoaded").field(r).finish(),
            Message::RefreshLibrary => f.write_str("RefreshLibrary"),

            // Library management
            Message::LibrariesLoaded(r) => f.debug_tuple("LibrariesLoaded").field(r).finish(),
            Message::LoadLibraries => f.write_str("LoadLibraries"),
            Message::CreateLibrary(l) => f.debug_tuple("CreateLibrary").field(l).finish(),
            Message::LibraryCreated(r) => f.debug_tuple("LibraryCreated").field(r).finish(),
            Message::UpdateLibrary(l) => f.debug_tuple("UpdateLibrary").field(l).finish(),
            Message::LibraryUpdated(r) => f.debug_tuple("LibraryUpdated").field(r).finish(),
            Message::DeleteLibrary(id) => f.debug_tuple("DeleteLibrary").field(id).finish(),
            Message::LibraryDeleted(r) => f.debug_tuple("LibraryDeleted").field(r).finish(),
            Message::SelectLibrary(id) => f.debug_tuple("SelectLibrary").field(id).finish(),
            Message::LibrarySelected(id, r) => {
                f.debug_tuple("LibrarySelected").field(id).field(r).finish()
            }
            Message::ScanLibrary_(id) => f.debug_tuple("ScanLibrary_").field(id).finish(),

            // View management
            Message::ShowLibraryManagement => f.write_str("ShowLibraryManagement"),
            Message::HideLibraryManagement => f.write_str("HideLibraryManagement"),
            Message::PlayMedia(m) => f.debug_tuple("PlayMedia").field(m).finish(),
            Message::PlayMediaWithId(m, id) => f.debug_tuple("PlayMediaWithId").field(m).field(id).finish(),
            Message::ViewDetails(m) => f.debug_tuple("ViewDetails").field(m).finish(),
            Message::ViewMovieDetails(m) => f.debug_tuple("ViewMovieDetails").field(m).finish(),
            Message::ViewTvShow(s) => f.debug_tuple("ViewTvShow").field(s).finish(),
            Message::ViewSeason(s, n) => f.debug_tuple("ViewSeason").field(s).field(n).finish(),
            Message::ViewEpisode(m) => f.debug_tuple("ViewEpisode").field(m).finish(),
            Message::SetViewMode(v) => f.debug_tuple("SetViewMode").field(v).finish(),
            Message::SetSortBy(s) => f.debug_tuple("SetSortBy").field(s).finish(),
            Message::ToggleSortOrder => f.write_str("ToggleSortOrder"),
            Message::ScanLibrary => f.write_str("ScanLibrary"),
            Message::ScanStarted(r) => f.debug_tuple("ScanStarted").field(r).finish(),
            Message::ScanProgressUpdate(p) => f.debug_tuple("ScanProgressUpdate").field(p).finish(),
            Message::ScanCompleted(r) => f.debug_tuple("ScanCompleted").field(r).finish(),
            Message::ForceRescan => f.write_str("ForceRescan"),
            Message::ClearScanProgress => f.write_str("ClearScanProgress"),
            Message::ToggleScanProgress => f.write_str("ToggleScanProgress"),
            Message::CheckActiveScans => f.write_str("CheckActiveScans"),
            Message::ActiveScansChecked(p) => f.debug_tuple("ActiveScansChecked").field(p).finish(),
            Message::MediaOrganized(m, t) => {
                f.debug_tuple("MediaOrganized").field(m).field(t).finish()
            }
            Message::UpdateTransitions => f.write_str("UpdateTransitions"),
            Message::MoviesGridScrolled(v) => f.debug_tuple("MoviesGridScrolled").field(v).finish(),
            Message::TvShowsGridScrolled(v) => {
                f.debug_tuple("TvShowsGridScrolled").field(v).finish()
            }
            Message::DetailViewScrolled(v) => f.debug_tuple("DetailViewScrolled").field(v).finish(),
            Message::CheckScrollStopped => f.write_str("CheckScrollStopped"),
            Message::RecalculateGridsAfterResize => f.write_str("RecalculateGridsAfterResize"),
            Message::TvShowLoaded(n, r) => f.debug_tuple("TvShowLoaded").field(n).field(r).finish(),
            Message::SeasonLoaded(n, s, r) => f
                .debug_tuple("SeasonLoaded")
                .field(n)
                .field(s)
                .field(r)
                .finish(),
            Message::BackToLibrary => f.write_str("BackToLibrary"),
            Message::Play => f.write_str("Play"),
            Message::Pause => f.write_str("Pause"),
            Message::PlayPause => f.write_str("PlayPause"),
            Message::Seek(pos) => f.debug_tuple("Seek").field(pos).finish(),
            Message::SeekRelative(delta) => f.debug_tuple("SeekRelative").field(delta).finish(),
            Message::SeekRelease => f.write_str("SeekRelease"),
            Message::SeekBarPressed => f.write_str("SeekBarPressed"),
            Message::SeekBarMoved(p) => f.debug_tuple("SeekBarMoved").field(p).finish(),
            Message::SeekDone => f.write_str("SeekDone"),
            Message::SetVolume(v) => f.debug_tuple("SetVolume").field(v).finish(),
            Message::EndOfStream => f.write_str("EndOfStream"),
            Message::NewFrame => f.write_str("NewFrame"),
            Message::Reload => f.write_str("Reload"),
            Message::ShowControls => f.write_str("ShowControls"),
            Message::Stop => f.write_str("Stop"),
            Message::ToggleFullscreen => f.write_str("ToggleFullscreen"),
            Message::ExitFullscreen => f.write_str("ExitFullscreen"),
            Message::ToggleMute => f.write_str("ToggleMute"),
            Message::SetPlaybackSpeed(s) => f.debug_tuple("SetPlaybackSpeed").field(s).finish(),
            Message::ToggleSettings => f.write_str("ToggleSettings"),
            Message::SetAspectRatio(r) => f.debug_tuple("SetAspectRatio").field(r).finish(),
            Message::SeekForward => f.write_str("SeekForward"),
            Message::SeekBackward => f.write_str("SeekBackward"),
            Message::MouseMoved => f.write_str("MouseMoved"),
            Message::VideoClicked => f.write_str("VideoClicked"),
            Message::VideoDoubleClicked => f.write_str("VideoDoubleClicked"),
            Message::AudioTrackSelected(t) => f.debug_tuple("AudioTrackSelected").field(t).finish(),
            Message::SubtitleTrackSelected(t) => {
                f.debug_tuple("SubtitleTrackSelected").field(t).finish()
            }

            // Tone mapping controls
            Message::ToggleToneMapping(enabled) => {
                f.debug_tuple("ToggleToneMapping").field(enabled).finish()
            }
            Message::SetToneMappingPreset(preset) => {
                f.debug_tuple("SetToneMappingPreset").field(preset).finish()
            }
            Message::SetToneMappingAlgorithm(algo) => f
                .debug_tuple("SetToneMappingAlgorithm")
                .field(algo)
                .finish(),
            Message::SetToneMappingWhitePoint(value) => f
                .debug_tuple("SetToneMappingWhitePoint")
                .field(value)
                .finish(),
            Message::SetToneMappingExposure(value) => f
                .debug_tuple("SetToneMappingExposure")
                .field(value)
                .finish(),
            Message::SetToneMappingSaturation(value) => f
                .debug_tuple("SetToneMappingSaturation")
                .field(value)
                .finish(),
            Message::SetHableShoulderStrength(value) => f
                .debug_tuple("SetHableShoulderStrength")
                .field(value)
                .finish(),
            Message::SetHableLinearStrength(value) => f
                .debug_tuple("SetHableLinearStrength")
                .field(value)
                .finish(),
            Message::SetHableLinearAngle(value) => {
                f.debug_tuple("SetHableLinearAngle").field(value).finish()
            }
            Message::SetHableToeStrength(value) => {
                f.debug_tuple("SetHableToeStrength").field(value).finish()
            }
            Message::SetMonitorBrightness(value) => {
                f.debug_tuple("SetMonitorBrightness").field(value).finish()
            }
            Message::SetToneMappingBrightness(value) => f
                .debug_tuple("SetToneMappingBrightness")
                .field(value)
                .finish(),
            Message::SetToneMappingContrast(value) => f
                .debug_tuple("SetToneMappingContrast")
                .field(value)
                .finish(),
            Message::SetToneMappingSaturationBoost(value) => f
                .debug_tuple("SetToneMappingSaturationBoost")
                .field(value)
                .finish(),

            Message::NoOp => f.write_str("NoOp"),
            Message::InitializeMetadataService => f.write_str("InitializeMetadataService"),
            Message::ToggleSubtitles => f.write_str("ToggleSubtitles"),
            Message::ToggleSubtitleMenu => f.write_str("ToggleSubtitleMenu"),
            Message::ToggleQualityMenu => f.write_str("ToggleQualityMenu"),
            Message::CycleAudioTrack => f.write_str("CycleAudioTrack"),
            Message::CycleSubtitleTrack => f.write_str("CycleSubtitleTrack"),
            Message::CycleSubtitleSimple => f.write_str("CycleSubtitleSimple"),
            Message::TracksLoaded => f.write_str("TracksLoaded"),
            Message::ClearError => f.write_str("ClearError"),
            Message::Tick => f.write_str("Tick"),
            Message::MediaAvailabilityChecked(m) => {
                f.debug_tuple("MediaAvailabilityChecked").field(m).finish()
            }
            Message::MediaUnavailable(r, m) => {
                f.debug_tuple("MediaUnavailable").field(r).field(m).finish()
            }
            Message::VideoLoaded(s) => f.debug_tuple("VideoLoaded").field(s).finish(),
            Message::VideoCreated(r) => f.debug_tuple("VideoCreated").field(r).finish(),
            Message::TranscodingStarted(r) => f.debug_tuple("TranscodingStarted").field(r).finish(),
            Message::TranscodingStatusUpdate(r) => f
                .debug_tuple("TranscodingStatusUpdate")
                .field(&match r {
                    Ok((status, duration, path)) => {
                        format!("Ok({:?}, {:?}, {:?})", status, duration, path)
                    }
                    Err(e) => format!("Err({})", e),
                })
                .finish(),
            Message::CheckTranscodingStatus => f.write_str("CheckTranscodingStatus"),
            Message::StartSegmentPrefetch(idx) => {
                f.debug_tuple("StartSegmentPrefetch").field(idx).finish()
            }
            Message::SegmentPrefetched(idx, r) => f
                .debug_tuple("SegmentPrefetched")
                .field(idx)
                .field(&match r {
                    Ok(data) => format!("Ok({} bytes)", data.len()),
                    Err(e) => format!("Err({})", e),
                })
                .finish(),
            Message::QualityVariantSelected(p) => {
                f.debug_tuple("QualityVariantSelected").field(p).finish()
            }
            Message::BandwidthMeasured(bw) => f.debug_tuple("BandwidthMeasured").field(bw).finish(),
            Message::MasterPlaylistLoaded(p) => {
                f.debug_tuple("MasterPlaylistLoaded").field(p).finish()
            }
            Message::MasterPlaylistReady(p) => {
                f.debug_tuple("MasterPlaylistReady").field(p).finish()
            }
            Message::RefreshShowMetadata(n) => {
                f.debug_tuple("RefreshShowMetadata").field(n).finish()
            }
            Message::RefreshSeasonMetadata(n, s) => f
                .debug_tuple("RefreshSeasonMetadata")
                .field(n)
                .field(s)
                .finish(),
            Message::RefreshEpisodeMetadata(id) => {
                f.debug_tuple("RefreshEpisodeMetadata").field(id).finish()
            }
            Message::CarouselNavigation(m) => f.debug_tuple("CarouselNavigation").field(m).finish(),
            Message::WindowResized(s) => f.debug_tuple("WindowResized").field(s).finish(),
            Message::MediaHovered(id) => f.debug_tuple("MediaHovered").field(id).finish(),
            Message::MediaUnhovered(id) => f.debug_tuple("MediaUnhovered").field(id).finish(),

            // TV show metadata refresh
            Message::ShowMetadataRefreshed(show_name) => f
                .debug_tuple("ShowMetadataRefreshed")
                .field(show_name)
                .finish(),
            Message::ShowMetadataRefreshFailed(show_name, error) => f
                .debug_tuple("ShowMetadataRefreshFailed")
                .field(show_name)
                .field(error)
                .finish(),

            // Library form messages
            Message::ShowLibraryForm(lib) => f.debug_tuple("ShowLibraryForm").field(lib).finish(),
            Message::HideLibraryForm => f.write_str("HideLibraryForm"),
            Message::UpdateLibraryFormName(name) => {
                f.debug_tuple("UpdateLibraryFormName").field(name).finish()
            }
            Message::UpdateLibraryFormType(library_type) => f
                .debug_tuple("UpdateLibraryFormType")
                .field(library_type)
                .finish(),
            Message::UpdateLibraryFormPaths(paths) => f
                .debug_tuple("UpdateLibraryFormPaths")
                .field(paths)
                .finish(),
            Message::UpdateLibraryFormScanInterval(interval) => f
                .debug_tuple("UpdateLibraryFormScanInterval")
                .field(interval)
                .finish(),
            Message::ToggleLibraryFormEnabled => f.write_str("ToggleLibraryFormEnabled"),
            Message::SubmitLibraryForm => f.write_str("SubmitLibraryForm"),

            // Admin dashboard messages
            Message::ShowAdminDashboard => f.write_str("ShowAdminDashboard"),
            Message::HideAdminDashboard => f.write_str("HideAdminDashboard"),

            // Database maintenance
            Message::ShowClearDatabaseConfirm => f.write_str("ShowClearDatabaseConfirm"),
            Message::HideClearDatabaseConfirm => f.write_str("HideClearDatabaseConfirm"),
            Message::ClearDatabase => f.write_str("ClearDatabase"),
            Message::DatabaseCleared(r) => f.debug_tuple("DatabaseCleared").field(r).finish(),

            // New reference-based messages
            Message::LibraryMediaReferencesLoaded(r) => f
                .debug_tuple("LibraryMediaReferencesLoaded")
                .field(r)
                .finish(),
            Message::AllLibrariesLoaded(results) => f
                .debug_tuple("AllLibrariesLoaded")
                .field(&results.len())
                .finish(),
            Message::MediaDetailsUpdated(r) => {
                f.debug_tuple("MediaDetailsUpdated").field(r).finish()
            }
            Message::MediaDetailsBatch(batch) => f
                .debug_tuple("MediaDetailsBatch")
                .field(&batch.len())
                .finish(),
            Message::QueueVisibleDetailsForFetch => write!(f, "QueueVisibleDetailsForFetch"),
            Message::SeriesSortingCompleted(series) => f
                .debug_tuple("SeriesSortingCompleted")
                .field(&series.len())
                .finish(),
            Message::CheckDetailsFetcherQueue => write!(f, "CheckDetailsFetcherQueue"),
            Message::ToggleBackdropAspectMode => write!(f, "ToggleBackdropAspectMode"),
            Message::RefreshViewModels => write!(f, "RefreshViewModels"),

            // Header navigation
            Message::NavigateHome => write!(f, "NavigateHome"),
            Message::UpdateSearchQuery(q) => f.debug_tuple("UpdateSearchQuery").field(q).finish(),
            Message::ExecuteSearch => write!(f, "ExecuteSearch"),
            Message::ShowLibraryMenu => write!(f, "ShowLibraryMenu"),
            Message::ShowAllLibrariesMenu => write!(f, "ShowAllLibrariesMenu"),
            Message::RefreshCurrentLibrary => write!(f, "RefreshCurrentLibrary"),
            Message::ScanCurrentLibrary => write!(f, "ScanCurrentLibrary"),
            Message::ShowProfile => write!(f, "ShowProfile"),
            Message::AggregateAllLibraries => write!(f, "AggregateAllLibraries"),
            Message::BatchMetadataComplete => write!(f, "BatchMetadataComplete"),
            
            // Authentication messages
            Message::CheckAuthStatus => write!(f, "CheckAuthStatus"),
            Message::AuthStatusConfirmedWithPin => write!(f, "AuthStatusConfirmedWithPin"),
            Message::CheckSetupStatus => write!(f, "CheckSetupStatus"),
            Message::SetupStatusChecked(b) => f.debug_tuple("SetupStatusChecked").field(b).finish(),
            Message::LoadUsers => write!(f, "LoadUsers"),
            Message::UsersLoaded(r) => f.debug_tuple("UsersLoaded").field(r).finish(),
            Message::SelectUser(id) => f.debug_tuple("SelectUser").field(id).finish(),
            Message::ShowCreateUser => write!(f, "ShowCreateUser"),
            Message::BackToUserSelection => write!(f, "BackToUserSelection"),
            Message::ShowPinEntry(user) => f.debug_tuple("ShowPinEntry").field(&user.username).finish(),
            Message::PinDigitPressed(c) => f.debug_tuple("PinDigitPressed").field(c).finish(),
            Message::PinBackspace => write!(f, "PinBackspace"),
            Message::PinClear => write!(f, "PinClear"),
            Message::PinSubmit => write!(f, "PinSubmit"),
            Message::LoginSuccess(user, _permissions) => f.debug_tuple("LoginSuccess").field(&user.username).finish(),
            Message::LoginError(e) => f.debug_tuple("LoginError").field(e).finish(),
            Message::WatchStatusLoaded(r) => f.debug_tuple("WatchStatusLoaded").field(r).finish(),
            Message::Logout => write!(f, "Logout"),
            Message::LogoutComplete => write!(f, "LogoutComplete"),
            Message::ShowPasswordLogin(username) => f.debug_tuple("ShowPasswordLogin").field(username).finish(),
            Message::PasswordLoginUpdateUsername(s) => f.debug_tuple("PasswordLoginUpdateUsername").field(s).finish(),
            Message::PasswordLoginUpdatePassword(_) => write!(f, "PasswordLoginUpdatePassword(****)"),
            Message::PasswordLoginToggleVisibility => write!(f, "PasswordLoginToggleVisibility"),
            Message::PasswordLoginToggleRemember => write!(f, "PasswordLoginToggleRemember"),
            Message::PasswordLoginSubmit => write!(f, "PasswordLoginSubmit"),
            
            // Device authentication flow messages
            Message::AuthFlowDeviceStatusChecked(user, result) => f
                .debug_tuple("AuthFlowDeviceStatusChecked")
                .field(&user.username)
                .field(result)
                .finish(),
            Message::AuthFlowUpdateCredential(s) => f.debug_tuple("AuthFlowUpdateCredential").field(s).finish(),
            Message::AuthFlowSubmitCredential => write!(f, "AuthFlowSubmitCredential"),
            Message::AuthFlowTogglePasswordVisibility => write!(f, "AuthFlowTogglePasswordVisibility"),
            Message::AuthFlowToggleRememberDevice => write!(f, "AuthFlowToggleRememberDevice"),
            Message::AuthFlowAuthResult(r) => f.debug_tuple("AuthFlowAuthResult").field(r).finish(),
            Message::AuthFlowSetupPin => write!(f, "AuthFlowSetupPin"),
            Message::AuthFlowUpdatePin(_) => write!(f, "AuthFlowUpdatePin(****)"),
            Message::AuthFlowUpdateConfirmPin(_) => write!(f, "AuthFlowUpdateConfirmPin(****)"),
            Message::AuthFlowSubmitPin => write!(f, "AuthFlowSubmitPin"),
            Message::AuthFlowPinSet(r) => f.debug_tuple("AuthFlowPinSet").field(r).finish(),
            Message::AuthFlowRetry => write!(f, "AuthFlowRetry"),
            Message::AuthFlowBack => write!(f, "AuthFlowBack"),
            
            Message::FirstRunUpdateUsername(s) => f.debug_tuple("FirstRunUpdateUsername").field(s).finish(),
            Message::FirstRunUpdateDisplayName(s) => f.debug_tuple("FirstRunUpdateDisplayName").field(s).finish(),
            Message::FirstRunUpdatePassword(s) => f.debug_tuple("FirstRunUpdatePassword").field(s).finish(),
            Message::FirstRunUpdateConfirmPassword(s) => f.debug_tuple("FirstRunUpdateConfirmPassword").field(s).finish(),
            Message::FirstRunTogglePasswordVisibility => write!(f, "FirstRunTogglePasswordVisibility"),
            Message::FirstRunSubmit => write!(f, "FirstRunSubmit"),
            Message::FirstRunSuccess => write!(f, "FirstRunSuccess"),
            Message::FirstRunError(e) => f.debug_tuple("FirstRunError").field(e).finish(),
        }
    }
}
