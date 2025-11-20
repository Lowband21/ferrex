use std::collections::HashMap;

use iced::{widget::scrollable, Point};

use crate::{
    carousel::CarouselMessage,
    media_library::MediaFile,
    models::{SeasonDetails, TvShow, TvShowDetails},
    player,
    state::{ScanProgress, SortBy, ViewMode},
    MediaEvent,
};

#[derive(Debug, Clone)]
pub enum Message {
    // Library messages
    LibraryLoaded(Result<Vec<MediaFile>, String>),
    RefreshLibrary,
    PlayMedia(MediaFile),
    ViewDetails(MediaFile),
    ViewTvShow(String),      // show_name
    ViewSeason(String, u32), // show_name, season_num
    ViewEpisode(MediaFile),
    SetViewMode(ViewMode), // Switch between All/Movies/TV Shows
    SetSortBy(SortBy),     // Change sort field
    ToggleSortOrder,       // Toggle ascending/descending
    ScanLibrary,
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
