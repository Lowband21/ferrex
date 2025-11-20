use std::collections::HashMap;

use iced::{widget::scrollable, Point};

use crate::{
    carousel::CarouselMessage,
    media_library::{Library, MediaFile},
    models::{SeasonDetails, TvShow, TvShowDetails},
    player,
    state::{ScanProgress, SortBy, ViewMode},
    MediaEvent,
};

#[derive(Clone)]
pub enum Message {
    // Library messages
    LibraryLoaded(Result<Vec<MediaFile>, String>),
    RefreshLibrary,
    
    // Library management
    LibrariesLoaded(Result<Vec<Library>, String>),
    LoadLibraries,
    CreateLibrary(Library),
    LibraryCreated(Result<Library, String>),
    UpdateLibrary(Library),
    LibraryUpdated(Result<Library, String>),
    DeleteLibrary(String), // library_id
    LibraryDeleted(Result<String, String>), // library_id or error
    SelectLibrary(String), // library_id
    LibrarySelected(String, Result<Vec<MediaFile>, String>), // library_id, media_files
    ScanLibrary_(String), // library_id - renamed to avoid conflict with existing ScanLibrary
    
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
    ViewDetails(MediaFile),
    ViewTvShow(String),      // show_name
    ViewSeason(String, u32), // show_name, season_num
    ViewEpisode(MediaFile),
    SetViewMode(ViewMode), // Switch between All/Movies/TV Shows
    SetSortBy(SortBy),     // Change sort field
    ToggleSortOrder,       // Toggle ascending/descending
    ScanLibrary, // Legacy - scans all libraries
    ScanStarted(Result<String, String>), // scan_id or error
    ScanProgressUpdate(ScanProgress),
    ScanCompleted(Result<String, String>),
    ForceRescan,
    ClearScanProgress,
    ToggleScanProgress,                    // Toggle scan progress overlay
    CheckActiveScans,                      // Check for active scans on startup
    ActiveScansChecked(Vec<ScanProgress>), // Result of active scans check
    FetchMissingPosters,                   // Manually fetch posters for items without them
    CheckPosterUpdates,                    // Periodically check for new posters

    // Media events from server
    MediaEventReceived(MediaEvent),
    MediaEventsError(String),

    // Poster monitoring background task
    PosterMonitorTick,

    // Background organization
    MediaOrganized(Vec<MediaFile>, HashMap<String, TvShow>),

    // Periodic batch operations
    BatchSort,

    // Virtual scrolling messages
    MoviesGridScrolled(scrollable::Viewport),
    TvShowsGridScrolled(scrollable::Viewport),
    CheckScrollStopped,                                 // Check if scrolling has stopped
    LoadPoster(String),                                 // media_id
    CheckPostersBatch(Vec<String>),                     // Check availability of multiple posters
    PostersBatchChecked(Vec<(String, Option<String>)>), // (media_id, poster_url)

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
    SetVolume(f64),
    EndOfStream,
    // MissingPlugin(gstreamer::Message), // Not available in standard iced_video_player
    NewFrame,
    Reload,
    ShowControls,

    // New player controls
    Stop,
    ToggleFullscreen,
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

    // No operation message
    NoOp,
    ToggleSubtitles,
    ToggleSubtitleMenu,
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
    VideoLoaded(bool), // Success flag - video is stored in state

    // Poster loading
    PosterLoaded(String, Result<Vec<u8>, String>), // media_id, result
    PosterProcessed(String, Result<(iced::widget::image::Handle, iced::widget::image::Handle, bool), String>), // media_id, (thumbnail, full_size, was_visible)
    PostersBatchLoaded(Vec<(String, Result<Vec<u8>, String>)>), // batch of (media_id, result)
    ProcessPosterQueue,                            // Process next poster in queue
    AnimatePoster(String),                         // Animate poster fade-in
    MarkPostersForLoading(Vec<String>, usize),     // IDs to mark, new progress position

    // Image loading (generic)
    ImageLoaded(String, Result<Vec<u8>, String>), // cache_key, result

    // Metadata fetching
    FetchMetadata(String),                       // Fetch metadata for media_id
    MetadataFetched(String, Result<(), String>), // media_id, result
    RefreshShowMetadata(String),                 // Refresh metadata for all episodes in a show
    RefreshSeasonMetadata(String, u32),          // Refresh metadata for all episodes in a season
    RefreshEpisodeMetadata(String),              // Refresh metadata for a single episode

    // Carousel navigation
    CarouselNavigation(CarouselMessage),

    // Window events
    WindowResized(iced::Size),
    
    // Hover events
    MediaHovered(String),  // media_id
    MediaUnhovered,
}

impl Message {
    /// Get a human-readable name for this message variant for profiling
    pub fn name(&self) -> &'static str {
        match self {
            // Library messages
            Message::LibraryLoaded(_) => "LibraryLoaded",
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
            Message::ViewDetails(_) => "ViewDetails",
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
            Message::FetchMissingPosters => "FetchMissingPosters",
            Message::CheckPosterUpdates => "CheckPosterUpdates",

            // Media events from server
            Message::MediaEventReceived(_) => "MediaEventReceived",
            Message::MediaEventsError(_) => "MediaEventsError",

            // Poster monitoring background task
            Message::PosterMonitorTick => "PosterMonitorTick",

            // Background organization
            Message::MediaOrganized(_, _) => "MediaOrganized",

            // Periodic batch operations
            Message::BatchSort => "BatchSort",

            // Virtual scrolling messages
            Message::MoviesGridScrolled(_) => "MoviesGridScrolled",
            Message::TvShowsGridScrolled(_) => "TvShowsGridScrolled",
            Message::LoadPoster(_) => "LoadPoster",
            Message::CheckPostersBatch(_) => "CheckPostersBatch",
            Message::PostersBatchChecked(_) => "PostersBatchChecked",

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
            Message::SetVolume(_) => "SetVolume",
            Message::EndOfStream => "EndOfStream",
            Message::NewFrame => "NewFrame",
            Message::Reload => "Reload",
            Message::ShowControls => "ShowControls",

            // New player controls
            Message::Stop => "Stop",
            Message::ToggleFullscreen => "ToggleFullscreen",
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

            // No operation message
            Message::NoOp => "NoOp",
            Message::ToggleSubtitles => "ToggleSubtitles",
            Message::ToggleSubtitleMenu => "ToggleSubtitleMenu",
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

            // Poster loading
            Message::PosterLoaded(_, _) => "PosterLoaded",
            Message::PosterProcessed(_, _) => "PosterProcessed",
            Message::PostersBatchLoaded(_) => "PostersBatchLoaded",
            Message::ProcessPosterQueue => "ProcessPosterQueue",
            Message::AnimatePoster(_) => "AnimatePoster",
            Message::MarkPostersForLoading(_, _) => "MarkPostersForLoading",

            // Image loading (generic)
            Message::ImageLoaded(_, _) => "ImageLoaded",

            // Metadata fetching
            Message::FetchMetadata(_) => "FetchMetadata",
            Message::MetadataFetched(_, _) => "MetadataFetched",
            Message::RefreshShowMetadata(_) => "RefreshShowMetadata",
            Message::RefreshSeasonMetadata(_, _) => "RefreshSeasonMetadata",
            Message::RefreshEpisodeMetadata(_) => "RefreshEpisodeMetadata",

            // Carousel navigation
            Message::CarouselNavigation(_) => "CarouselNavigation",

            // Window events
            Message::WindowResized(_) => "WindowResized",
            Message::CheckScrollStopped => "CheckScrollStopped",
            
            // Hover events
            Message::MediaHovered(_) => "MediaHovered",
            Message::MediaUnhovered => "MediaUnhovered",
        }
    }
}

// Custom Debug implementation to avoid printing large image data
impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Special handling for PosterProcessed to avoid printing image handles
            Message::PosterProcessed(media_id, result) => {
                f.debug_struct("PosterProcessed")
                    .field("media_id", media_id)
                    .field("result", &match result {
                        Ok((_, _, was_visible)) => format!("Ok(<handles>, {})", was_visible),
                        Err(e) => format!("Err({})", e),
                    })
                    .finish()
            }
            // Special handling for PosterLoaded to avoid printing image bytes
            Message::PosterLoaded(media_id, result) => {
                f.debug_struct("PosterLoaded")
                    .field("media_id", media_id)
                    .field("result", &match result {
                        Ok(bytes) => format!("Ok(<{} bytes>)", bytes.len()),
                        Err(e) => format!("Err({})", e),
                    })
                    .finish()
            }
            // Special handling for PostersBatchLoaded
            Message::PostersBatchLoaded(batch) => {
                f.debug_struct("PostersBatchLoaded")
                    .field("batch", &batch.iter().map(|(id, result)| {
                        (id, match result {
                            Ok(bytes) => format!("Ok(<{} bytes>)", bytes.len()),
                            Err(e) => format!("Err({})", e),
                        })
                    }).collect::<Vec<_>>())
                    .finish()
            }
            // Special handling for ImageLoaded
            Message::ImageLoaded(cache_key, result) => {
                f.debug_struct("ImageLoaded")
                    .field("cache_key", cache_key)
                    .field("result", &match result {
                        Ok(bytes) => format!("Ok(<{} bytes>)", bytes.len()),
                        Err(e) => format!("Err({})", e),
                    })
                    .finish()
            }
            // For all other variants, use default Debug formatting
            Message::LibraryLoaded(r) => f.debug_tuple("LibraryLoaded").field(r).finish(),
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
            Message::LibrarySelected(id, r) => f.debug_tuple("LibrarySelected").field(id).field(r).finish(),
            Message::ScanLibrary_(id) => f.debug_tuple("ScanLibrary_").field(id).finish(),
            
            // View management
            Message::ShowLibraryManagement => f.write_str("ShowLibraryManagement"),
            Message::HideLibraryManagement => f.write_str("HideLibraryManagement"),
            Message::PlayMedia(m) => f.debug_tuple("PlayMedia").field(m).finish(),
            Message::ViewDetails(m) => f.debug_tuple("ViewDetails").field(m).finish(),
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
            Message::FetchMissingPosters => f.write_str("FetchMissingPosters"),
            Message::CheckPosterUpdates => f.write_str("CheckPosterUpdates"),
            Message::MediaEventReceived(e) => f.debug_tuple("MediaEventReceived").field(e).finish(),
            Message::MediaEventsError(e) => f.debug_tuple("MediaEventsError").field(e).finish(),
            Message::PosterMonitorTick => f.write_str("PosterMonitorTick"),
            Message::MediaOrganized(m, t) => f.debug_tuple("MediaOrganized").field(m).field(t).finish(),
            Message::BatchSort => f.write_str("BatchSort"),
            Message::MoviesGridScrolled(v) => f.debug_tuple("MoviesGridScrolled").field(v).finish(),
            Message::TvShowsGridScrolled(v) => f.debug_tuple("TvShowsGridScrolled").field(v).finish(),
            Message::CheckScrollStopped => f.write_str("CheckScrollStopped"),
            Message::LoadPoster(id) => f.debug_tuple("LoadPoster").field(id).finish(),
            Message::CheckPostersBatch(ids) => f.debug_tuple("CheckPostersBatch").field(ids).finish(),
            Message::PostersBatchChecked(r) => f.debug_tuple("PostersBatchChecked").field(r).finish(),
            Message::TvShowLoaded(n, r) => f.debug_tuple("TvShowLoaded").field(n).field(r).finish(),
            Message::SeasonLoaded(n, s, r) => f.debug_tuple("SeasonLoaded").field(n).field(s).field(r).finish(),
            Message::BackToLibrary => f.write_str("BackToLibrary"),
            Message::Play => f.write_str("Play"),
            Message::Pause => f.write_str("Pause"),
            Message::PlayPause => f.write_str("PlayPause"),
            Message::Seek(pos) => f.debug_tuple("Seek").field(pos).finish(),
            Message::SeekRelative(delta) => f.debug_tuple("SeekRelative").field(delta).finish(),
            Message::SeekRelease => f.write_str("SeekRelease"),
            Message::SeekBarPressed => f.write_str("SeekBarPressed"),
            Message::SeekBarMoved(p) => f.debug_tuple("SeekBarMoved").field(p).finish(),
            Message::SetVolume(v) => f.debug_tuple("SetVolume").field(v).finish(),
            Message::EndOfStream => f.write_str("EndOfStream"),
            Message::NewFrame => f.write_str("NewFrame"),
            Message::Reload => f.write_str("Reload"),
            Message::ShowControls => f.write_str("ShowControls"),
            Message::Stop => f.write_str("Stop"),
            Message::ToggleFullscreen => f.write_str("ToggleFullscreen"),
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
            Message::SubtitleTrackSelected(t) => f.debug_tuple("SubtitleTrackSelected").field(t).finish(),
            Message::NoOp => f.write_str("NoOp"),
            Message::ToggleSubtitles => f.write_str("ToggleSubtitles"),
            Message::ToggleSubtitleMenu => f.write_str("ToggleSubtitleMenu"),
            Message::CycleAudioTrack => f.write_str("CycleAudioTrack"),
            Message::CycleSubtitleTrack => f.write_str("CycleSubtitleTrack"),
            Message::CycleSubtitleSimple => f.write_str("CycleSubtitleSimple"),
            Message::TracksLoaded => f.write_str("TracksLoaded"),
            Message::ClearError => f.write_str("ClearError"),
            Message::Tick => f.write_str("Tick"),
            Message::MediaAvailabilityChecked(m) => f.debug_tuple("MediaAvailabilityChecked").field(m).finish(),
            Message::MediaUnavailable(r, m) => f.debug_tuple("MediaUnavailable").field(r).field(m).finish(),
            Message::VideoLoaded(s) => f.debug_tuple("VideoLoaded").field(s).finish(),
            Message::ProcessPosterQueue => f.write_str("ProcessPosterQueue"),
            Message::AnimatePoster(id) => f.debug_tuple("AnimatePoster").field(id).finish(),
            Message::MarkPostersForLoading(ids, pos) => f.debug_tuple("MarkPostersForLoading").field(ids).field(pos).finish(),
            Message::FetchMetadata(id) => f.debug_tuple("FetchMetadata").field(id).finish(),
            Message::MetadataFetched(id, r) => f.debug_tuple("MetadataFetched").field(id).field(r).finish(),
            Message::RefreshShowMetadata(n) => f.debug_tuple("RefreshShowMetadata").field(n).finish(),
            Message::RefreshSeasonMetadata(n, s) => f.debug_tuple("RefreshSeasonMetadata").field(n).field(s).finish(),
            Message::RefreshEpisodeMetadata(id) => f.debug_tuple("RefreshEpisodeMetadata").field(id).finish(),
            Message::CarouselNavigation(m) => f.debug_tuple("CarouselNavigation").field(m).finish(),
            Message::WindowResized(s) => f.debug_tuple("WindowResized").field(s).finish(),
            Message::MediaHovered(id) => f.debug_tuple("MediaHovered").field(id).finish(),
            Message::MediaUnhovered => f.write_str("MediaUnhovered"),
            
            // Library form messages
            Message::ShowLibraryForm(lib) => f.debug_tuple("ShowLibraryForm").field(lib).finish(),
            Message::HideLibraryForm => f.write_str("HideLibraryForm"),
            Message::UpdateLibraryFormName(name) => f.debug_tuple("UpdateLibraryFormName").field(name).finish(),
            Message::UpdateLibraryFormType(library_type) => f.debug_tuple("UpdateLibraryFormType").field(library_type).finish(),
            Message::UpdateLibraryFormPaths(paths) => f.debug_tuple("UpdateLibraryFormPaths").field(paths).finish(),
            Message::UpdateLibraryFormScanInterval(interval) => f.debug_tuple("UpdateLibraryFormScanInterval").field(interval).finish(),
            Message::ToggleLibraryFormEnabled => f.write_str("ToggleLibraryFormEnabled"),
            Message::SubmitLibraryForm => f.write_str("SubmitLibraryForm"),
            
            // Admin dashboard messages
            Message::ShowAdminDashboard => f.write_str("ShowAdminDashboard"),
            Message::HideAdminDashboard => f.write_str("HideAdminDashboard"),
        }
    }
}
