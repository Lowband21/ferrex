pub mod update;

use crate::domains::ui::{messages::UiMessage, tabs::TabId};
use ferrex_core::player_prelude::{
    EpisodeID, LibraryId, MovieID, SeasonID, SeriesID,
};
use iced::window;

pub use update::update_shell_ui;

/// Represents the current scope of content being displayed
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scope {
    /// Home (curated view)
    Home,
    /// Single library view
    Library(LibraryId),
}

impl Scope {
    /// Convert scope to corresponding tab ID
    pub fn to_tab_id(&self) -> TabId {
        match self {
            Scope::Home => TabId::Home,
            Scope::Library(id) => TabId::Library(*id),
        }
    }

    /// Get library id from current library scope if present
    pub fn lib_id(&self) -> Option<LibraryId> {
        match self {
            Scope::Home => None,
            Scope::Library(lib_id) => Some(*lib_id),
        }
    }
}

#[derive(Clone)]
pub enum UiShellMessage {
    // Unified scope selection
    SelectScope(Scope),

    // View navigation
    SelectLibraryAndMode(LibraryId),
    ViewMovieDetails(MovieID),
    ViewTvShow(SeriesID),
    ViewSeason(SeriesID, SeasonID),
    ViewEpisode(EpisodeID),

    // Header navigation
    NavigateHome,
    NavigateBack,

    // Main window lifecycle notifications
    MainWindowOpened(window::Id),
    MainWindowFocused,
    MainWindowUnfocused,
    RawWindowClosed(window::Id),

    // Search window and query management
    UpdateSearchQuery(String),
    BeginSearchFromKeyboard(String),
    ExecuteSearch,
    OpenSearchWindow,
    OpenSearchWindowWithSeed(String),
    SearchWindowOpened(window::Id),
    FocusSearchWindow,
    FocusSearchInput,
    CloseSearchWindow,

    // Cross-domain controls
    ToggleFullscreen,
}

impl From<UiShellMessage> for UiMessage {
    fn from(msg: UiShellMessage) -> Self {
        UiMessage::Shell(msg)
    }
}

impl UiShellMessage {
    pub fn name(&self) -> &'static str {
        match self {
            // Unified scope selection
            Self::SelectScope(_) => "UI::SelectScope",

            // View navigation
            Self::SelectLibraryAndMode(_) => "UI::SelectLibraryAndMode",
            Self::ViewMovieDetails(_) => "UI::ViewMovieDetails",
            Self::ViewTvShow(_) => "UI::ViewTvShow",
            Self::ViewSeason(_, _) => "UI::ViewSeason",
            Self::ViewEpisode(_) => "UI::ViewEpisode",

            // Header navigation
            Self::NavigateHome => "UI::NavigateHome",
            Self::NavigateBack => "UI::NavigateBack",

            // Main window lifecycle notifications
            Self::MainWindowOpened(_) => "UI::MainWindowOpened",
            Self::MainWindowFocused => "UI::MainWindowFocused",
            Self::MainWindowUnfocused => "UI::MainWindowUnfocused",
            Self::RawWindowClosed(_) => "UI::RawWindowClosed",

            // Search window and query management
            Self::UpdateSearchQuery(_) => "UI::UpdateSearchQuery",
            Self::BeginSearchFromKeyboard(_) => "UI::BeginSearchFromKeyboard",
            Self::ExecuteSearch => "UI::ExecuteSearch",
            Self::OpenSearchWindow => "UI::OpenSearchWindow",
            Self::OpenSearchWindowWithSeed(_) => "UI::OpenSearchWindowWithSeed",
            Self::SearchWindowOpened(_) => "UI::SearchWindowOpened",
            Self::FocusSearchWindow => "UI::FocusSearchWindow",
            Self::FocusSearchInput => "UI::FocusSearchInput",
            Self::CloseSearchWindow => "UI::CloseSearchWindow",
            Self::ToggleFullscreen => "UI::ToggleFullscreen",
        }
    }
}

impl std::fmt::Debug for UiShellMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UiShellMessage::SelectScope(scope) => {
                write!(f, "UI::SelectScope({:?})", scope)
            }
            UiShellMessage::SelectLibraryAndMode(id) => {
                write!(f, "UI::SelectLibraryAndMode({})", id)
            }
            UiShellMessage::ViewMovieDetails(movie) => {
                write!(f, "UI::ViewMovieDetails({:?})", movie)
            }
            UiShellMessage::ViewTvShow(id) => {
                write!(f, "UI::ViewTvShow({})", id)
            }
            UiShellMessage::ViewSeason(series_id, season_id) => {
                write!(f, "UI::ViewSeason({}, {})", series_id, season_id)
            }
            UiShellMessage::ViewEpisode(id) => {
                write!(f, "UI::ViewEpisode({})", id)
            }
            UiShellMessage::NavigateHome => write!(f, "UI::NavigateHome"),
            UiShellMessage::NavigateBack => write!(f, "UI::NavigateBack"),
            UiShellMessage::MainWindowOpened(id) => {
                write!(f, "UI::MainWindowOpened({:?})", id)
            }
            UiShellMessage::MainWindowFocused => {
                write!(f, "UI::MainWindowFocused")
            }
            UiShellMessage::MainWindowUnfocused => {
                write!(f, "UI::MainWindowUnfocused")
            }
            UiShellMessage::RawWindowClosed(id) => {
                write!(f, "UI::RawWindowClosed({:?})", id)
            }
            UiShellMessage::UpdateSearchQuery(_) => {
                write!(f, "UI::UpdateSearchQuery")
            }
            UiShellMessage::BeginSearchFromKeyboard(_) => {
                write!(f, "UI::BeginSearchFromKeyboard")
            }
            UiShellMessage::ExecuteSearch => write!(f, "UI::ExecuteSearch"),
            UiShellMessage::OpenSearchWindow => {
                write!(f, "UI::OpenSearchWindow")
            }
            UiShellMessage::OpenSearchWindowWithSeed(_) => {
                write!(f, "UI::OpenSearchWindowWithSeed")
            }
            UiShellMessage::SearchWindowOpened(id) => {
                write!(f, "UI::SearchWindowOpened({:?})", id)
            }
            UiShellMessage::FocusSearchWindow => {
                write!(f, "UI::FocusSearchWindow")
            }
            UiShellMessage::FocusSearchInput => {
                write!(f, "UI::FocusSearchInput")
            }
            UiShellMessage::CloseSearchWindow => {
                write!(f, "UI::CloseSearchWindow")
            }
            UiShellMessage::ToggleFullscreen => {
                write!(f, "UI::ToggleFullscreen")
            }
        }
    }
}
