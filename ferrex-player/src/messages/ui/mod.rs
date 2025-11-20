pub mod subscriptions;

use crate::{
    api_types::MediaReference,
    state::{SortBy, ViewMode},
    views::carousel::CarouselMessage,
};
use iced::widget::scrollable;
use iced::Size;

#[derive(Clone)]
pub enum Message {
    // View mode control
    SetViewMode(ViewMode), // Switch between All/Movies/TV Shows
    ViewDetails(crate::media_library::MediaFile),
    ViewMovieDetails(crate::api_types::MovieReference),
    ViewTvShow(ferrex_core::SeriesID), // series_id
    ViewSeason(ferrex_core::SeriesID, ferrex_core::SeasonID), // series_id, season_id
    ViewEpisode(ferrex_core::EpisodeID), // episode_id

    // Sorting
    SetSortBy(SortBy), // Change sort field
    ToggleSortOrder,   // Toggle ascending/descending

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
    MoviesGridScrolled(scrollable::Viewport),
    TvShowsGridScrolled(scrollable::Viewport),
    CheckScrollStopped,                       // Check if scrolling has stopped
    RecalculateGridsAfterResize,              // Recalculate grid states after window resize
    DetailViewScrolled(scrollable::Viewport), // Scroll events in detail views

    // Window events
    WindowResized(Size),

    // Hover events
    MediaHovered(String),   // media_id
    MediaUnhovered(String), // media_id being unhovered

    // Header navigation
    NavigateHome,
    BackToLibrary, // Navigate back to library/home view
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
    
    // Device management
    LoadDevices,
    DevicesLoaded(Result<Vec<crate::views::settings::device_management::UserDevice>, String>),
    RevokeDevice(String), // device_id
    DeviceRevoked(Result<String, String>), // device_id or error
    RefreshDevices,
    
    // User preferences
    ToggleAutoLogin(bool),
    AutoLoginToggled(Result<bool, String>),  // Proxy for Auth::Logout

    // Carousel navigation
    CarouselNavigation(CarouselMessage),

    // Animation and transition messages
    UpdateTransitions, // Update color and backdrop transitions

    // Backdrop control
    ToggleBackdropAspectMode,
    UpdateBackdropHandle(iced::widget::image::Handle),

    // View model refresh
    RefreshViewModels,
    QueueVisibleDetailsForFetch, // Queue visible items for background detail fetching

    // Cross-domain proxy messages
    ToggleFullscreen,                  // Proxy for Media::ToggleFullscreen
    ToggleScanProgress,                // Proxy for Library::ToggleScanProgress
    SelectLibrary(Option<uuid::Uuid>), // Proxy for Library::SelectLibrary
    PlayMediaWithId(
        crate::media_library::MediaFile,
        ferrex_core::api_types::MediaId,
    ), // Proxy for Media::PlayMediaWithId

    // TV Show loading
    TvShowLoaded(String, Result<crate::models::TvShowDetails, String>), // series_id, result

    // Library aggregation
    AggregateAllLibraries, // Signal to aggregate all libraries

    // Library management proxies
    ShowLibraryForm(Option<crate::media_library::Library>), // Proxy for Library::ShowLibraryForm
    HideLibraryForm,                                        // Proxy for Library::HideLibraryForm
    ScanLibrary_(uuid::Uuid),                               // Proxy for Library::ScanLibrary_
    DeleteLibrary(uuid::Uuid),                              // Proxy for Library::DeleteLibrary
    UpdateLibraryFormName(String), // Proxy for Library::UpdateLibraryFormName
    UpdateLibraryFormType(String), // Proxy for Library::UpdateLibraryFormType
    UpdateLibraryFormPaths(String), // Proxy for Library::UpdateLibraryFormPaths
    UpdateLibraryFormScanInterval(String), // Proxy for Library::UpdateLibraryFormScanInterval
    ToggleLibraryFormEnabled,      // Proxy for Library::ToggleLibraryFormEnabled
    SubmitLibraryForm,             // Proxy for Library::SubmitLibraryForm

    // Internal cross-domain coordination
    #[doc(hidden)]
    _EmitCrossDomainEvent(crate::messages::CrossDomainEvent),

    // No-op variant for UI elements that are not yet implemented
    NoOp,
}

impl Message {
    pub fn name(&self) -> &'static str {
        match self {
            // View mode control
            Self::SetViewMode(_) => "UI::SetViewMode",
            Self::ViewDetails(_) => "UI::ViewDetails",
            Self::ViewMovieDetails(_) => "UI::ViewMovieDetails",
            Self::ViewTvShow(_) => "UI::ViewTvShow",
            Self::ViewSeason(_, _) => "UI::ViewSeason",
            Self::ViewEpisode(_) => "UI::ViewEpisode",

            // Sorting
            Self::SetSortBy(_) => "UI::SetSortBy",
            Self::ToggleSortOrder => "UI::ToggleSortOrder",

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
            Self::MoviesGridScrolled(_) => "UI::MoviesGridScrolled",
            Self::TvShowsGridScrolled(_) => "UI::TvShowsGridScrolled",
            Self::CheckScrollStopped => "UI::CheckScrollStopped",
            Self::RecalculateGridsAfterResize => "UI::RecalculateGridsAfterResize",
            Self::DetailViewScrolled(_) => "UI::DetailViewScrolled",

            // Window events
            Self::WindowResized(_) => "UI::WindowResized",

            // Hover events
            Self::MediaHovered(_) => "UI::MediaHovered",
            Self::MediaUnhovered(_) => "UI::MediaUnhovered",

            // Header navigation
            Self::NavigateHome => "UI::NavigateHome",
            Self::BackToLibrary => "UI::BackToLibrary",
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

            // View model refresh
            Self::RefreshViewModels => "UI::RefreshViewModels",
            Self::QueueVisibleDetailsForFetch => "UI::QueueVisibleDetailsForFetch",

            // Cross-domain proxy messages
            Self::ToggleFullscreen => "UI::ToggleFullscreen",
            Self::ToggleScanProgress => "UI::ToggleScanProgress",
            Self::SelectLibrary(_) => "UI::SelectLibrary",
            Self::PlayMediaWithId(_, _) => "UI::PlayMediaWithId",

            // TV Show loading
            Self::TvShowLoaded(_, _) => "UI::TvShowLoaded",

            // Library aggregation
            Self::AggregateAllLibraries => "UI::AggregateAllLibraries",

            // Library management proxies
            Self::ShowLibraryForm(_) => "UI::ShowLibraryForm",
            Self::HideLibraryForm => "UI::HideLibraryForm",
            Self::ScanLibrary_(_) => "UI::ScanLibrary_",
            Self::DeleteLibrary(_) => "UI::DeleteLibrary",
            Self::UpdateLibraryFormName(_) => "UI::UpdateLibraryFormName",
            Self::UpdateLibraryFormType(_) => "UI::UpdateLibraryFormType",
            Self::UpdateLibraryFormPaths(_) => "UI::UpdateLibraryFormPaths",
            Self::UpdateLibraryFormScanInterval(_) => "UI::UpdateLibraryFormScanInterval",
            Self::ToggleLibraryFormEnabled => "UI::ToggleLibraryFormEnabled",
            Self::SubmitLibraryForm => "UI::SubmitLibraryForm",

            // Internal
            Self::_EmitCrossDomainEvent(_) => "UI::_EmitCrossDomainEvent",

            // No-op
            Self::NoOp => "UI::NoOp",
        }
    }
}

impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SetViewMode(mode) => write!(f, "UI::SetViewMode({:?})", mode),
            Self::ViewDetails(file) => write!(f, "UI::ViewDetails({})", file.display_title()),
            Self::ViewMovieDetails(movie) => {
                write!(f, "UI::ViewMovieDetails({})", movie.title.as_str())
            }
            Self::ViewTvShow(id) => write!(f, "UI::ViewTvShow({})", id),
            Self::ViewSeason(series_id, season_id) => {
                write!(f, "UI::ViewSeason({}, {})", series_id, season_id)
            }
            Self::ViewEpisode(id) => write!(f, "UI::ViewEpisode({})", id),
            Self::SetSortBy(sort) => write!(f, "UI::SetSortBy({:?})", sort),
            Self::ToggleSortOrder => write!(f, "UI::ToggleSortOrder"),
            Self::ShowAdminDashboard => write!(f, "UI::ShowAdminDashboard"),
            Self::HideAdminDashboard => write!(f, "UI::HideAdminDashboard"),
            Self::ShowLibraryManagement => write!(f, "UI::ShowLibraryManagement"),
            Self::HideLibraryManagement => write!(f, "UI::HideLibraryManagement"),
            Self::NavigateHome => write!(f, "UI::NavigateHome"),
            Self::WindowResized(size) => write!(f, "UI::WindowResized({:?})", size),
            Self::ToggleBackdropAspectMode => write!(f, "UI::ToggleBackdropAspectMode"),
            Self::TvShowLoaded(series_id, result) => match result {
                Ok(_) => write!(f, "UI::TvShowLoaded({}, Ok)", series_id),
                Err(e) => write!(f, "UI::TvShowLoaded({}, Err: {})", series_id, e),
            },
            Self::AggregateAllLibraries => write!(f, "UI::AggregateAllLibraries"),
            Self::ShowLibraryForm(lib) => {
                if let Some(l) = lib {
                    write!(f, "UI::ShowLibraryForm(Some: {})", l.name)
                } else {
                    write!(f, "UI::ShowLibraryForm(None)")
                }
            }
            Self::HideLibraryForm => write!(f, "UI::HideLibraryForm"),
            Self::ScanLibrary_(id) => write!(f, "UI::ScanLibrary_({})", id),
            Self::DeleteLibrary(id) => write!(f, "UI::DeleteLibrary({})", id),
            Self::UpdateLibraryFormName(name) => write!(f, "UI::UpdateLibraryFormName({})", name),
            Self::UpdateLibraryFormType(t) => write!(f, "UI::UpdateLibraryFormType({:?})", t),
            Self::UpdateLibraryFormPaths(paths) => {
                write!(f, "UI::UpdateLibraryFormPaths({} paths)", paths.len())
            }
            Self::UpdateLibraryFormScanInterval(i) => {
                write!(f, "UI::UpdateLibraryFormScanInterval({})", i)
            }
            Self::ToggleLibraryFormEnabled => write!(f, "UI::ToggleLibraryFormEnabled"),
            Self::SubmitLibraryForm => write!(f, "UI::SubmitLibraryForm"),
            Self::_EmitCrossDomainEvent(event) => {
                write!(f, "UI::_EmitCrossDomainEvent({:?})", event)
            }
            Self::NoOp => write!(f, "UI::NoOp"),
            Message::SetViewMode(view_mode) => write!(f, "UI::SetViewMode({:?})", view_mode),
            Message::ViewDetails(media_file) => write!(f, "UI::ViewDetails({:?})", media_file),
            Message::ViewMovieDetails(movie_reference) => {
                write!(f, "UI::ViewMovieDetails({:?})", movie_reference)
            }
            Message::ViewTvShow(series_id) => write!(f, "UI::ViewTvShow({:?})", series_id),
            Message::ViewSeason(series_id, season_id) => {
                write!(f, "UI::ViewSeason({:?}, {:?})", series_id, season_id)
            }
            Message::ViewEpisode(episode_id) => write!(f, "UI::ViewEpisode({:?})", episode_id),
            Message::SetSortBy(sort_by) => write!(f, "UI::SetSortBy({:?})", sort_by),
            Message::ToggleSortOrder => write!(f, "UI::ToggleSortOrder"),
            Message::ShowAdminDashboard => write!(f, "UI::ShowAdminDashboard"),
            Message::HideAdminDashboard => write!(f, "UI::HideAdminDashboard"),
            Message::ShowLibraryManagement => write!(f, "UI::ShowLibraryManagement"),
            Message::HideLibraryManagement => write!(f, "UI::HideLibraryManagement"),
            Message::ShowClearDatabaseConfirm => write!(f, "UI::ShowClearDatabaseConfirm"),
            Message::HideClearDatabaseConfirm => write!(f, "UI::HideClearDatabaseConfirm"),
            Message::ClearDatabase => write!(f, "UI::ClearDatabase"),
            Message::DatabaseCleared(_) => write!(f, "UI::DatabaseCleared"),
            Message::ClearError => write!(f, "UI::ClearError"),
            Message::MoviesGridScrolled(viewport) => {
                write!(f, "UI::MoviesGridScrolled({:?})", viewport)
            }
            Message::TvShowsGridScrolled(viewport) => {
                write!(f, "UI::TvShowsGridScrolled({:?})", viewport)
            }
            Message::CheckScrollStopped => write!(f, "UI::CheckScrollStopped"),
            Message::RecalculateGridsAfterResize => write!(f, "UI::RecalculateGridsAfterResize"),
            Message::DetailViewScrolled(viewport) => {
                write!(f, "UI::DetailViewScrolled({:?})", viewport)
            }
            Message::WindowResized(size) => write!(f, "UI::WindowResized({:?})", size),
            Message::MediaHovered(_) => write!(f, "UI::MediaHovered"),
            Message::MediaUnhovered(_) => write!(f, "UI::MediaUnhovered"),
            Message::NavigateHome => write!(f, "UI::NavigateHome"),
            Message::BackToLibrary => write!(f, "UI::BackToLibrary"),
            Message::UpdateSearchQuery(_) => write!(f, "UI::UpdateSearchQuery"),
            Message::ExecuteSearch => write!(f, "UI::ExecuteSearch"),
            Message::ShowLibraryMenu => write!(f, "UI::ShowLibraryMenu"),
            Message::ShowAllLibrariesMenu => write!(f, "UI::ShowAllLibrariesMenu"),
            Message::ShowProfile => write!(f, "UI::ShowProfile"),
            Message::ShowUserProfile => write!(f, "UI::ShowUserProfile"),
            Message::ShowUserPreferences => write!(f, "UI::ShowUserPreferences"),
            Message::ShowUserSecurity => write!(f, "UI::ShowUserSecurity"),
            Message::ShowDeviceManagement => write!(f, "UI::ShowDeviceManagement"),
            Message::BackToSettings => write!(f, "UI::BackToSettings"),
            Message::Logout => write!(f, "UI::Logout"),
            
            // Security settings
            Message::ShowChangePassword => write!(f, "UI::ShowChangePassword"),
            Message::UpdatePasswordCurrent(_) => write!(f, "UI::UpdatePasswordCurrent"),
            Message::UpdatePasswordNew(_) => write!(f, "UI::UpdatePasswordNew"),
            Message::UpdatePasswordConfirm(_) => write!(f, "UI::UpdatePasswordConfirm"),
            Message::TogglePasswordVisibility => write!(f, "UI::TogglePasswordVisibility"),
            Message::SubmitPasswordChange => write!(f, "UI::SubmitPasswordChange"),
            Message::PasswordChangeResult(_) => write!(f, "UI::PasswordChangeResult"),
            Message::CancelPasswordChange => write!(f, "UI::CancelPasswordChange"),
            
            Message::ShowSetPin => write!(f, "UI::ShowSetPin"),
            Message::ShowChangePin => write!(f, "UI::ShowChangePin"),
            Message::UpdatePinCurrent(_) => write!(f, "UI::UpdatePinCurrent"),
            Message::UpdatePinNew(_) => write!(f, "UI::UpdatePinNew"),
            Message::UpdatePinConfirm(_) => write!(f, "UI::UpdatePinConfirm"),
            Message::SubmitPinChange => write!(f, "UI::SubmitPinChange"),
            Message::PinChangeResult(_) => write!(f, "UI::PinChangeResult"),
            Message::CancelPinChange => write!(f, "UI::CancelPinChange"),
            
            // Device management
            Message::LoadDevices => write!(f, "UI::LoadDevices"),
            Message::DevicesLoaded(result) => match result {
                Ok(devices) => write!(f, "UI::DevicesLoaded(Ok: {} devices)", devices.len()),
                Err(e) => write!(f, "UI::DevicesLoaded(Err: {})", e),
            },
            Message::RevokeDevice(device_id) => write!(f, "UI::RevokeDevice({})", device_id),
            Message::DeviceRevoked(result) => match result {
                Ok(device_id) => write!(f, "UI::DeviceRevoked(Ok: {})", device_id),
                Err(e) => write!(f, "UI::DeviceRevoked(Err: {})", e),
            },
            Message::RefreshDevices => write!(f, "UI::RefreshDevices"),
            
            // User preferences
            Message::ToggleAutoLogin(_) => write!(f, "UI::ToggleAutoLogin"),
            Message::AutoLoginToggled(_) => write!(f, "UI::AutoLoginToggled"),
            Message::CarouselNavigation(carousel_message) => {
                write!(f, "UI::CarouselNavigation({:?})", carousel_message)
            }
            Message::UpdateTransitions => write!(f, "UI::UpdateTransitions"),
            Message::ToggleBackdropAspectMode => write!(f, "UI::ToggleBackdropAspectMode"),
            Message::UpdateBackdropHandle(handle) => {
                write!(f, "UI::UpdateBackdropHandle({:?})", handle)
            }
            Message::RefreshViewModels => write!(f, "UI::RefreshViewModels"),
            Message::QueueVisibleDetailsForFetch => write!(f, "UI::QueueVisibleDetailsForFetch"),
            Message::ToggleFullscreen => write!(f, "UI::ToggleFullscreen"),
            Message::ToggleScanProgress => write!(f, "UI::ToggleScanProgress"),
            Message::SelectLibrary(uuid) => write!(f, "UI::SelectLibrary({:?})", uuid),
            Message::PlayMediaWithId(media_file, media_id) => {
                write!(f, "UI::PlayMediaWithId({:?}, {:?})", media_file, media_id)
            }
            Message::TvShowLoaded(_, tv_show_details) => {
                write!(f, "UI::TvShowLoaded({:?})", tv_show_details)
            }
            Message::AggregateAllLibraries => write!(f, "UI::AggregateAllLibraries"),
            Message::ShowLibraryForm(library) => write!(f, "UI::ShowLibraryForm({:?})", library),
            Message::HideLibraryForm => write!(f, "UI::HideLibraryForm"),
            Message::ScanLibrary_(uuid) => write!(f, "UI::ScanLibrary_({:?})", uuid),
            Message::DeleteLibrary(uuid) => write!(f, "UI::DeleteLibrary({:?})", uuid),
            Message::UpdateLibraryFormName(_) => write!(f, "UI::UpdateLibraryFormName()"),
            Message::UpdateLibraryFormType(_) => write!(f, "UI::UpdateLibraryFormType()"),
            Message::UpdateLibraryFormPaths(_) => write!(f, "UI::UpdateLibraryFormPaths()"),
            Message::UpdateLibraryFormScanInterval(_) => {
                write!(f, "UI::UpdateLibraryFormScanInterval()")
            }
            Message::ToggleLibraryFormEnabled => write!(f, "UI::ToggleLibraryFormEnabled"),
            Message::SubmitLibraryForm => write!(f, "UI::SubmitLibraryForm"),
            Message::_EmitCrossDomainEvent(cross_domain_event) => {
                write!(f, "UI::_EmitCrossDomainEvent({:?})", cross_domain_event)
            }
            Message::NoOp => write!(f, "UI::NoOp"),
        }
    }
}

/// UI domain events
#[derive(Clone, Debug)]
pub enum UIEvent {
    ViewChanged(ViewMode),
    WindowResized(Size),
    ScrollPositionChanged,
    SearchExecuted(String),
}
