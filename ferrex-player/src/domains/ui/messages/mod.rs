pub mod subscriptions;

use crate::domains::ui::{DisplayMode, views::carousel::CarouselMessage};
use ferrex_core::player_prelude::{
    EpisodeID, Library, LibraryID, MediaID, MediaIDLike, MovieID, SeasonID, SeriesID, SortBy,
    UiDecade, UiGenre, UiResolution, UiWatchStatus,
};
use iced::Size;
use iced::widget::scrollable;
use uuid::Uuid;

#[derive(Clone)]
pub enum Message {
    // View mode control
    SetDisplayMode(DisplayMode),     // New library-centric display mode
    SelectLibraryAndMode(LibraryID), // Select library and set to Library display mode
    ViewDetails(MediaID),
    ViewMovieDetails(MovieID),
    ViewTvShow(SeriesID),
    ViewSeason(SeriesID, SeasonID),
    ViewEpisode(EpisodeID),

    // Sorting
    SetSortBy(SortBy),                                      // Change sort field
    ToggleSortOrder,                                        // Toggle ascending/descending
    ApplySortedPositions(LibraryID, Option<u64>, Vec<u32>), // Apply position indices with optional cache key
    ApplyFilteredPositions(LibraryID, u64, Vec<u32>), // Apply filtered indices with cache key (Phase 1)
    RequestFilteredPositions, // Trigger fetching filtered positions for active library
    // Filter panel interactions
    ToggleFilterPanel,          // Open/close the filter panel
    ToggleFilterGenre(UiGenre), // Toggle a genre chip
    SetFilterDecade(UiDecade),  // Set a decade
    ClearFilterDecade,          // Clear decade selection
    SetFilterResolution(UiResolution),
    SetFilterWatchStatus(UiWatchStatus),
    ApplyFilters, // Build spec from UI inputs and request filtered positions
    ClearFilters, // Clear UI inputs and reset filters
    SortedIndexFailed(String), // Report fetch failure

    // Admin views
    ShowAdminDashboard,
    HideAdminDashboard,
    ShowLibraryManagement,
    HideLibraryManagement,

    // Database maintenance UI
    ShowClearDatabaseConfirm,
    HideClearDatabaseConfirm,
    ClearDatabase,
    DatabaseCleared(Result<(), String>),

    // Error handling
    ClearError, // Clear current error state

    // Scrolling
    TabGridScrolled(scrollable::Viewport), // Unified scroll message for tab system
    CheckScrollStopped,                    // Check if scrolling has stopped
    DetailViewScrolled(scrollable::Viewport), // Scroll events in detail views

    // Window events
    WindowResized(Size),
    WindowMoved(Option<iced::Point>),

    // Hover events
    MediaHovered(Uuid),
    MediaUnhovered(Uuid),

    // Header navigation
    NavigateHome,
    NavigateBack, // Navigate to previous view in history
    UpdateSearchQuery(String),
    ExecuteSearch,
    ShowLibraryMenu,
    ShowAllLibrariesMenu,
    ShowProfile,

    // User settings navigation
    ShowUserProfile,
    ShowUserPreferences,
    ShowUserSecurity,
    ShowDeviceManagement,
    BackToSettings,
    Logout,

    // Security settings
    ShowChangePassword,
    UpdatePasswordCurrent(String),
    UpdatePasswordNew(String),
    UpdatePasswordConfirm(String),
    TogglePasswordVisibility,
    SubmitPasswordChange,
    PasswordChangeResult(Result<(), String>),
    CancelPasswordChange,

    ShowSetPin,
    ShowChangePin,
    UpdatePinCurrent(String),
    UpdatePinNew(String),
    UpdatePinConfirm(String),
    SubmitPinChange,
    PinChangeResult(Result<(), String>),
    CancelPinChange,

    // Device management - now proxies to cross-domain events
    LoadDevices,
    DevicesLoaded(
        Result<Vec<crate::domains::ui::views::settings::device_management::UserDevice>, String>,
    ),
    RevokeDevice(String),                  // device_id
    DeviceRevoked(Result<String, String>), // device_id or error
    RefreshDevices,

    // User preferences
    ToggleAutoLogin(bool),
    AutoLoginToggled(Result<bool, String>), // Proxy for Auth::Logout

    // Carousel navigation
    CarouselNavigation(CarouselMessage),

    // Animation and transition messages
    UpdateTransitions, // Update color and backdrop transitions

    // Backdrop control
    ToggleBackdropAspectMode,
    UpdateBackdropHandle(iced::widget::image::Handle),

    // View model updates
    RefreshViewModels,           // Full refresh from MediaStore (expensive)
    UpdateViewModelFilters,      // Just update filters (lightweight)
    CheckMediaStoreRefresh,      // Check if MediaStore notifier indicates refresh needed
    QueueVisibleDetailsForFetch, // Queue visible items for background detail fetching

    // Cross-domain proxy messages
    ToggleFullscreen,                 // Proxy for Media::ToggleFullscreen
    SelectLibrary(Option<LibraryID>), // Proxy for Library::SelectLibrary
    PlayMediaWithId(MediaID),         // Proxy for Media::PlayMediaWithId
    PlaySeriesNextEpisode(SeriesID),  // Play next unwatched/in-progress episode

    // TV Show loading
    //TvShowLoaded(String, Result<TvShowDetails, String>), // series_id, result

    // Library aggregation
    AggregateAllLibraries, // Signal to aggregate all libraries

    // Library management proxies
    ShowLibraryForm(Option<Library>), // Proxy for Library::ShowLibraryForm
    HideLibraryForm,                  // Proxy for Library::HideLibraryForm
    ScanLibrary(LibraryID),           // Proxy for Library::ScanLibrary_
    DeleteLibrary(LibraryID),         // Proxy for Library::DeleteLibrary
    UpdateLibraryFormName(String),    // Proxy for Library::UpdateLibraryFormName
    UpdateLibraryFormType(String),    // Proxy for Library::UpdateLibraryFormType
    UpdateLibraryFormPaths(String),   // Proxy for Library::UpdateLibraryFormPaths
    UpdateLibraryFormScanInterval(String), // Proxy for Library::UpdateLibraryFormScanInterval
    ToggleLibraryFormEnabled,         // Proxy for Library::ToggleLibraryFormEnabled
    ToggleLibraryFormStartScan,       // Proxy for Library::ToggleLibraryFormStartScan
    SubmitLibraryForm,                // Proxy for Library::SubmitLibraryForm
    PauseLibraryScan(LibraryID, Uuid), // Proxy for Library::PauseScan
    ResumeLibraryScan(LibraryID, Uuid), // Proxy for Library::ResumeScan
    CancelLibraryScan(LibraryID, Uuid), // Proxy for Library::CancelScan
    // Scanner metrics + admin actions
    FetchScanMetrics,        // Proxy for Library::FetchScanMetrics
    ResetLibrary(LibraryID), // Proxy for Library::ResetLibrary

    // No-op variant for UI elements that are not yet implemented
    NoOp,
}

impl Message {
    pub fn name(&self) -> &'static str {
        match self {
            // View mode control
            Self::SetDisplayMode(_) => "UI::SetDisplayMode",
            Self::SelectLibraryAndMode(_) => "UI::SelectLibraryAndMode",
            Self::ViewDetails(_) => "UI::ViewDetails",
            Self::ViewMovieDetails(_) => "UI::ViewMovieDetails",
            Self::ViewTvShow(_) => "UI::ViewTvShow",
            Self::ViewSeason(_, _) => "UI::ViewSeason",
            Self::ViewEpisode(_) => "UI::ViewEpisode",

            // Sorting
            Self::SetSortBy(_) => "UI::SetSortBy",
            Self::ToggleSortOrder => "UI::ToggleSortOrder",
            Self::ApplySortedPositions(_, _, _) => "UI::ApplySortedPositions",
            Self::ApplyFilteredPositions(_, _, _) => "UI::ApplyFilteredPositions",
            Self::RequestFilteredPositions => "UI::RequestFilteredPositions",
            Self::ToggleFilterPanel => "UI::ToggleFilterPanel",
            Self::ToggleFilterGenre(_) => "UI::ToggleFilterGenre",
            Self::SetFilterDecade(_) => "UI::SetFilterDecade",
            Self::ClearFilterDecade => "UI::ClearFilterDecade",
            Self::SetFilterResolution(_) => "UI::SetFilterResolution",
            Self::SetFilterWatchStatus(_) => "UI::SetFilterWatchStatus",
            Self::ApplyFilters => "UI::ApplyFilters",
            Self::ClearFilters => "UI::ClearFilters",
            Self::SortedIndexFailed(_) => "UI::SortedIndexFailed",

            // Admin views
            Self::ShowAdminDashboard => "UI::ShowAdminDashboard",
            Self::HideAdminDashboard => "UI::HideAdminDashboard",
            Self::ShowLibraryManagement => "UI::ShowLibraryManagement",
            Self::HideLibraryManagement => "UI::HideLibraryManagement",

            // Database maintenance UI
            Self::ShowClearDatabaseConfirm => "UI::ShowClearDatabaseConfirm",
            Self::HideClearDatabaseConfirm => "UI::HideClearDatabaseConfirm",
            Self::ClearDatabase => "UI::ClearDatabase",
            Self::DatabaseCleared(_) => "UI::DatabaseCleared",

            // Error handling
            Self::ClearError => "UI::ClearError",

            // Scrolling
            Self::TabGridScrolled(_) => "UI::TabGridScrolled",
            Self::CheckScrollStopped => "UI::CheckScrollStopped",
            Self::DetailViewScrolled(_) => "UI::DetailViewScrolled",

            // Window events
            Self::WindowResized(_) => "UI::WindowResized",
            Self::WindowMoved(_) => "UI::WindowMoved",

            // Hover events
            Self::MediaHovered(_) => "UI::MediaHovered",
            Self::MediaUnhovered(_) => "UI::MediaUnhovered",

            // Header navigation
            Self::NavigateHome => "UI::NavigateHome",
            Self::NavigateBack => "UI::NavigateBack",
            Self::UpdateSearchQuery(_) => "UI::UpdateSearchQuery",
            Self::ExecuteSearch => "UI::ExecuteSearch",
            Self::ShowLibraryMenu => "UI::ShowLibraryMenu",
            Self::ShowAllLibrariesMenu => "UI::ShowAllLibrariesMenu",
            Self::ShowProfile => "UI::ShowProfile",

            // User settings navigation
            Self::ShowUserProfile => "UI::ShowUserProfile",
            Self::ShowUserPreferences => "UI::ShowUserPreferences",
            Self::ShowUserSecurity => "UI::ShowUserSecurity",
            Self::ShowDeviceManagement => "UI::ShowDeviceManagement",
            Self::BackToSettings => "UI::BackToSettings",
            Self::Logout => "UI::Logout",

            // Security settings
            Self::ShowChangePassword => "UI::ShowChangePassword",
            Self::UpdatePasswordCurrent(_) => "UI::UpdatePasswordCurrent",
            Self::UpdatePasswordNew(_) => "UI::UpdatePasswordNew",
            Self::UpdatePasswordConfirm(_) => "UI::UpdatePasswordConfirm",
            Self::TogglePasswordVisibility => "UI::TogglePasswordVisibility",
            Self::SubmitPasswordChange => "UI::SubmitPasswordChange",
            Self::PasswordChangeResult(_) => "UI::PasswordChangeResult",
            Self::CancelPasswordChange => "UI::CancelPasswordChange",

            Self::ShowSetPin => "UI::ShowSetPin",
            Self::ShowChangePin => "UI::ShowChangePin",
            Self::UpdatePinCurrent(_) => "UI::UpdatePinCurrent",
            Self::UpdatePinNew(_) => "UI::UpdatePinNew",
            Self::UpdatePinConfirm(_) => "UI::UpdatePinConfirm",
            Self::SubmitPinChange => "UI::SubmitPinChange",
            Self::PinChangeResult(_) => "UI::PinChangeResult",
            Self::CancelPinChange => "UI::CancelPinChange",

            // Device management
            Self::LoadDevices => "UI::LoadDevices",
            Self::DevicesLoaded(_) => "UI::DevicesLoaded",
            Self::RevokeDevice(_) => "UI::RevokeDevice",
            Self::DeviceRevoked(_) => "UI::DeviceRevoked",
            Self::RefreshDevices => "UI::RefreshDevices",

            // User preferences
            Self::ToggleAutoLogin(_) => "UI::ToggleAutoLogin",
            Self::AutoLoginToggled(_) => "UI::AutoLoginToggled",

            // Carousel navigation
            Self::CarouselNavigation(_) => "UI::CarouselNavigation",

            // Animation and transition messages
            Self::UpdateTransitions => "UI::UpdateTransitions",

            // Backdrop control
            Self::ToggleBackdropAspectMode => "UI::ToggleBackdropAspectMode",
            Self::UpdateBackdropHandle(_) => "UI::UpdateBackdropHandle",

            // View model updates
            Self::RefreshViewModels => "UI::RefreshViewModels",
            Self::UpdateViewModelFilters => "UI::UpdateViewModelFilters",
            Self::CheckMediaStoreRefresh => "UI::CheckMediaStoreRefresh",
            Self::QueueVisibleDetailsForFetch => "UI::QueueVisibleDetailsForFetch",

            // Cross-domain proxy messages
            Self::ToggleFullscreen => "UI::ToggleFullscreen",
            Self::SelectLibrary(_) => "UI::SelectLibrary",
            Self::PlayMediaWithId(_) => "UI::PlayMediaWithId",
            Self::PlaySeriesNextEpisode(_) => "UI::PlaySeriesNextEpisode",

            // TV Show loading
            //Self::TvShowLoaded(_, _) => "UI::TvShowLoaded",

            // Library aggregation
            Self::AggregateAllLibraries => "UI::AggregateAllLibraries",

            // Library management proxies
            Self::ShowLibraryForm(_) => "UI::ShowLibraryForm",
            Self::HideLibraryForm => "UI::HideLibraryForm",
            Self::ScanLibrary(_) => "UI::ScanLibrary_",
            Self::DeleteLibrary(_) => "UI::DeleteLibrary",
            Self::UpdateLibraryFormName(_) => "UI::UpdateLibraryFormName",
            Self::UpdateLibraryFormType(_) => "UI::UpdateLibraryFormType",
            Self::UpdateLibraryFormPaths(_) => "UI::UpdateLibraryFormPaths",
            Self::UpdateLibraryFormScanInterval(_) => "UI::UpdateLibraryFormScanInterval",
            Self::ToggleLibraryFormEnabled => "UI::ToggleLibraryFormEnabled",
            Self::ToggleLibraryFormStartScan => "UI::ToggleLibraryFormStartScan",
            Self::SubmitLibraryForm => "UI::SubmitLibraryForm",
            Self::PauseLibraryScan(_, _) => "UI::PauseLibraryScan",
            Self::ResumeLibraryScan(_, _) => "UI::ResumeLibraryScan",
            Self::CancelLibraryScan(_, _) => "UI::CancelLibraryScan",
            Self::FetchScanMetrics => "UI::FetchScanMetrics",
            Self::ResetLibrary(_) => "UI::ResetLibrary",

            // No-op
            Self::NoOp => "UI::NoOp",
        }
    }
}

impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SetDisplayMode(mode) => write!(f, "UI::SetDisplayMode({:?})", mode),
            Self::SelectLibraryAndMode(id) => write!(f, "UI::SelectLibraryAndMode({})", id),
            Self::ViewDetails(id) => write!(f, "UI::ViewDetails({})", id.to_uuid()),
            Self::ViewMovieDetails(movie) => {
                write!(f, "UI::ViewMovieDetails({:?})", movie) // FIX
            }
            Self::ViewTvShow(id) => write!(f, "UI::ViewTvShow({})", id),
            Self::ViewSeason(series_id, season_id) => {
                write!(f, "UI::ViewSeason({}, {})", series_id, season_id)
            }
            Self::ViewEpisode(id) => write!(f, "UI::ViewEpisode({})", id),
            Self::SetSortBy(sort) => write!(f, "UI::SetSortBy({:?})", sort),
            Self::ToggleSortOrder => write!(f, "UI::ToggleSortOrder"),
            Self::ApplySortedPositions(_, cache_key, _) => match cache_key {
                Some(hash) => write!(f, "UI::ApplySortedPositions(hash={hash})"),
                None => write!(f, "UI::ApplySortedPositions"),
            },
            Self::ApplyFilteredPositions(_, hash, _) => {
                write!(f, "UI::ApplyFilteredPositions(hash={hash})")
            }
            Self::RequestFilteredPositions => write!(f, "UI::RequestFilteredPositions"),
            Self::ToggleFilterPanel => write!(f, "UI::ToggleFilterPanel"),
            Self::ToggleFilterGenre(_) => write!(f, "UI::ToggleFilterGenre"),
            Self::SetFilterDecade(_) => write!(f, "UI::SetFilterDecade"),
            Self::ClearFilterDecade => write!(f, "UI::ClearFilterDecade"),
            Self::SetFilterResolution(_) => write!(f, "UI::SetFilterResolution"),
            Self::SetFilterWatchStatus(_) => write!(f, "UI::SetFilterWatchStatus"),
            Self::ApplyFilters => write!(f, "UI::ApplyFilters"),
            Self::ClearFilters => write!(f, "UI::ClearFilters"),
            Self::SortedIndexFailed(_) => write!(f, "UI::SortedIndexFailed"),
            Self::ShowAdminDashboard => write!(f, "UI::ShowAdminDashboard"),
            Self::HideAdminDashboard => write!(f, "UI::HideAdminDashboard"),
            Self::ShowLibraryManagement => write!(f, "UI::ShowLibraryManagement"),
            Self::HideLibraryManagement => write!(f, "UI::HideLibraryManagement"),
            Self::NavigateHome => write!(f, "UI::NavigateHome"),
            Self::WindowResized(size) => write!(f, "UI::WindowResized({:?})", size),
            Self::WindowMoved(position) => write!(f, "UI::WindowMoved({:?})", position),
            Self::ToggleBackdropAspectMode => write!(f, "UI::ToggleBackdropAspectMode"),
            //Self::TvShowLoaded(series_id, result) => match result {
            //    Ok(_) => write!(f, "UI::TvShowLoaded({}, Ok)", series_id),
            //    Err(e) => write!(f, "UI::TvShowLoaded({}, Err: {})", series_id, e),
            //},
            Self::AggregateAllLibraries => write!(f, "UI::AggregateAllLibraries"),
            Self::ShowLibraryForm(lib) => {
                if let Some(l) = lib {
                    write!(f, "UI::ShowLibraryForm(Some: {})", l.name)
                } else {
                    write!(f, "UI::ShowLibraryForm(None)")
                }
            }
            Self::HideLibraryForm => write!(f, "UI::HideLibraryForm"),
            Self::ScanLibrary(id) => write!(f, "UI::ScanLibrary_({})", id),
            Self::DeleteLibrary(id) => write!(f, "UI::DeleteLibrary({})", id),
            Self::UpdateLibraryFormName(name) => write!(f, "UI::UpdateLibraryFormName({})", name),
            Self::ShowClearDatabaseConfirm => write!(f, "UI::ShowClearDatabaseConfirm"),
            Self::HideClearDatabaseConfirm => write!(f, "UI::HideClearDatabaseConfirm"),
            Self::ClearDatabase => write!(f, "UI::ClearDatabase"),
            Self::DatabaseCleared(_) => write!(f, "UI::DatabaseCleared"),
            Self::ClearError => write!(f, "UI::ClearError"),
            Self::TabGridScrolled(viewport) => {
                write!(f, "UI::TabGridScrolled({:?})", viewport)
            }
            Self::CheckScrollStopped => write!(f, "UI::CheckScrollStopped"),
            Self::DetailViewScrolled(viewport) => {
                write!(f, "UI::DetailViewScrolled({:?})", viewport)
            }
            Self::MediaHovered(_) => write!(f, "UI::MediaHovered"),
            Self::MediaUnhovered(_) => write!(f, "UI::MediaUnhovered"),
            Self::NavigateBack => write!(f, "UI::NavigateBack"),
            Self::UpdateSearchQuery(_) => write!(f, "UI::UpdateSearchQuery"),
            Self::ExecuteSearch => write!(f, "UI::ExecuteSearch"),
            Self::ShowLibraryMenu => write!(f, "UI::ShowLibraryMenu"),
            Self::ShowAllLibrariesMenu => write!(f, "UI::ShowAllLibrariesMenu"),
            Self::ShowProfile => write!(f, "UI::ShowProfile"),
            Self::ShowUserProfile => write!(f, "UI::ShowUserProfile"),
            Self::ShowUserPreferences => write!(f, "UI::ShowUserPreferences"),
            Self::ShowUserSecurity => write!(f, "UI::ShowUserSecurity"),
            Self::ShowDeviceManagement => write!(f, "UI::ShowDeviceManagement"),
            Self::BackToSettings => write!(f, "UI::BackToSettings"),
            Self::Logout => write!(f, "UI::Logout"),
            Self::ShowChangePassword => write!(f, "UI::ShowChangePassword"),
            Self::UpdatePasswordCurrent(_) => write!(f, "UI::UpdatePasswordCurrent"),
            Self::UpdatePasswordNew(_) => write!(f, "UI::UpdatePasswordNew"),
            Self::UpdatePasswordConfirm(_) => write!(f, "UI::UpdatePasswordConfirm"),
            Self::TogglePasswordVisibility => write!(f, "UI::TogglePasswordVisibility"),
            Self::SubmitPasswordChange => write!(f, "UI::SubmitPasswordChange"),
            Self::PasswordChangeResult(_) => write!(f, "UI::PasswordChangeResult"),
            Self::CancelPasswordChange => write!(f, "UI::CancelPasswordChange"),
            Self::ShowSetPin => write!(f, "UI::ShowSetPin"),
            Self::ShowChangePin => write!(f, "UI::ShowChangePin"),
            Self::UpdatePinCurrent(_) => write!(f, "UI::UpdatePinCurrent"),
            Self::UpdatePinNew(_) => write!(f, "UI::UpdatePinNew"),
            Self::UpdatePinConfirm(_) => write!(f, "UI::UpdatePinConfirm"),
            Self::SubmitPinChange => write!(f, "UI::SubmitPinChange"),
            Self::PinChangeResult(_) => write!(f, "UI::PinChangeResult"),
            Self::CancelPinChange => write!(f, "UI::CancelPinChange"),
            Self::LoadDevices => write!(f, "UI::LoadDevices"),
            Self::DevicesLoaded(result) => match result {
                Ok(devices) => write!(f, "UI::DevicesLoaded(Ok: {} devices)", devices.len()),
                Err(e) => write!(f, "UI::DevicesLoaded(Err: {})", e),
            },
            Self::RevokeDevice(device_id) => write!(f, "UI::RevokeDevice({})", device_id),
            Self::DeviceRevoked(result) => match result {
                Ok(device_id) => write!(f, "UI::DeviceRevoked(Ok: {})", device_id),
                Err(e) => write!(f, "UI::DeviceRevoked(Err: {})", e),
            },
            Self::RefreshDevices => write!(f, "UI::RefreshDevices"),
            Self::ToggleAutoLogin(_) => write!(f, "UI::ToggleAutoLogin"),
            Self::AutoLoginToggled(_) => write!(f, "UI::AutoLoginToggled"),
            Self::CarouselNavigation(carousel_message) => {
                write!(f, "UI::CarouselNavigation({:?})", carousel_message)
            }
            Self::UpdateTransitions => write!(f, "UI::UpdateTransitions"),
            Self::UpdateBackdropHandle(handle) => {
                write!(f, "UI::UpdateBackdropHandle({:?})", handle)
            }
            Self::RefreshViewModels => write!(f, "UI::RefreshViewModels"),
            Self::UpdateViewModelFilters => write!(f, "UI::UpdateViewModelFilters"),
            Self::CheckMediaStoreRefresh => write!(f, "UI::CheckMediaStoreRefresh"),
            Self::QueueVisibleDetailsForFetch => write!(f, "UI::QueueVisibleDetailsForFetch"),
            Self::ToggleFullscreen => write!(f, "UI::ToggleFullscreen"),
            Self::SelectLibrary(uuid) => write!(f, "UI::SelectLibrary({:?})", uuid),
            Self::PlayMediaWithId(media_id) => {
                write!(f, "UI::PlayMediaWithId({:?})", media_id)
            }
            Message::PlaySeriesNextEpisode(series_id) => {
                write!(f, "PlaySeriesNextEpisode({:?})", series_id)
            }
            Self::UpdateLibraryFormType(_) => write!(f, "UI::UpdateLibraryFormType()"),
            Self::UpdateLibraryFormPaths(_) => write!(f, "UI::UpdateLibraryFormPaths()"),
            Self::UpdateLibraryFormScanInterval(_) => {
                write!(f, "UI::UpdateLibraryFormScanInterval()")
            }
            Self::ToggleLibraryFormEnabled => write!(f, "UI::ToggleLibraryFormEnabled"),
            Self::ToggleLibraryFormStartScan => write!(f, "UI::ToggleLibraryFormStartScan"),
            Self::SubmitLibraryForm => write!(f, "UI::SubmitLibraryForm"),
            Self::PauseLibraryScan(library_id, scan_id) => {
                write!(f, "UI::PauseLibraryScan({}, {})", library_id, scan_id)
            }
            Self::ResumeLibraryScan(library_id, scan_id) => {
                write!(f, "UI::ResumeLibraryScan({}, {})", library_id, scan_id)
            }
            Self::CancelLibraryScan(library_id, scan_id) => {
                write!(f, "UI::CancelLibraryScan({}, {})", library_id, scan_id)
            }
            Self::FetchScanMetrics => write!(f, "UI::FetchScanMetrics"),
            Self::ResetLibrary(id) => write!(f, "UI::ResetLibrary({})", id),
            Self::NoOp => write!(f, "UI::NoOp"),
        }
    }
}

/// UI domain events
#[derive(Clone, Debug)]
pub enum UIEvent {
    WindowResized(Size),
    ScrollPositionChanged,
    SearchExecuted(String),
}
