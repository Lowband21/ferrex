pub mod cross_domain;

// Message types are now defined in their respective domains
use crate::common::focus::FocusMessage;
use crate::domains::auth;
use crate::domains::library;
use crate::domains::media;
use crate::domains::metadata;
use crate::domains::player;
use crate::domains::settings;
use crate::domains::streaming;
use crate::domains::ui;

use crate::domains::search;
use crate::domains::user_management;

use ferrex_core::player_prelude::{
    LibraryId, MediaFile, MediaID, User, UserPermissions,
};
use iced::Task;

/// Result of a domain update operation.
///
/// This temporary alias preserves existing `ferrex-player` imports while the
/// generic helper lives in `ferrex-player-foundation`.
pub type DomainUpdate = ferrex_player_foundation::domain::DomainUpdate<
    DomainMessage,
    CrossDomainEvent,
>;

/// Result of a domain update that includes both a task and events to emit.
///
/// This temporary alias preserves existing `ferrex-player` imports while the
/// generic helper lives in `ferrex-player-foundation`.
pub type DomainUpdateResult =
    ferrex_player_foundation::domain::DomainUpdateResult<
        Task<DomainMessage>,
        CrossDomainEvent,
    >;

/// The main domain message router
#[derive(Clone)]
pub enum DomainMessage {
    /// Authentication domain
    Auth(auth::messages::AuthMessage),

    /// Library management domain
    Library(library::messages::LibraryMessage),

    /// Media playback domain
    Media(media::messages::MediaMessage),

    /// Player domain
    Player(player::messages::PlayerMessage),

    /// UI/View domain
    Ui(ui::messages::UiMessage),

    /// Metadata fetching domain
    Metadata(metadata::messages::MetadataMessage),

    /// Streaming/Transcoding domain
    Streaming(streaming::messages::StreamingMessage),

    /// Settings domain
    Settings(settings::messages::SettingsMessage),

    /// User management domain
    UserManagement(user_management::messages::UserManagementMessage),

    /// Search domain
    Search(search::messages::SearchMessage),

    /// Focus orchestration
    Focus(FocusMessage),

    /// Cross-domain coordination messages
    NoOp,
    Tick,
    ClearError,
    Event(CrossDomainEvent), // Cross-domain event for coordination
}

// Automatic routing from domain messages
impl From<auth::messages::AuthMessage> for DomainMessage {
    fn from(msg: auth::messages::AuthMessage) -> Self {
        DomainMessage::Auth(msg)
    }
}

impl From<library::messages::LibraryMessage> for DomainMessage {
    fn from(msg: library::messages::LibraryMessage) -> Self {
        DomainMessage::Library(msg)
    }
}

impl From<media::messages::MediaMessage> for DomainMessage {
    fn from(msg: media::messages::MediaMessage) -> Self {
        DomainMessage::Media(msg)
    }
}

impl From<player::messages::PlayerMessage> for DomainMessage {
    fn from(msg: player::messages::PlayerMessage) -> Self {
        DomainMessage::Player(msg)
    }
}

impl From<ui::messages::UiMessage> for DomainMessage {
    fn from(msg: ui::messages::UiMessage) -> Self {
        DomainMessage::Ui(msg)
    }
}

impl From<metadata::messages::MetadataMessage> for DomainMessage {
    fn from(msg: metadata::messages::MetadataMessage) -> Self {
        DomainMessage::Metadata(msg)
    }
}

impl From<streaming::messages::StreamingMessage> for DomainMessage {
    fn from(msg: streaming::messages::StreamingMessage) -> Self {
        DomainMessage::Streaming(msg)
    }
}

impl From<settings::messages::SettingsMessage> for DomainMessage {
    fn from(msg: settings::messages::SettingsMessage) -> Self {
        DomainMessage::Settings(msg)
    }
}

impl From<user_management::messages::UserManagementMessage> for DomainMessage {
    fn from(msg: user_management::messages::UserManagementMessage) -> Self {
        DomainMessage::UserManagement(msg)
    }
}

impl From<search::messages::SearchMessage> for DomainMessage {
    fn from(msg: search::messages::SearchMessage) -> Self {
        DomainMessage::Search(msg)
    }
}

impl From<FocusMessage> for DomainMessage {
    fn from(msg: FocusMessage) -> Self {
        DomainMessage::Focus(msg)
    }
}

impl DomainMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Auth(msg) => msg.name(),
            Self::Library(msg) => msg.name(),
            Self::Media(msg) => msg.name(),
            Self::Player(_) => "Player", // PlayerMessage doesn't have name() method yet
            Self::Ui(msg) => msg.name(),
            Self::Metadata(msg) => msg.name(),
            Self::Streaming(msg) => msg.name(),
            Self::Settings(msg) => msg.name(),
            Self::UserManagement(msg) => msg.name(),
            Self::Search(msg) => msg.as_str(),
            Self::Focus(msg) => msg.name(),
            Self::NoOp => "DomainMessage::NoOp",
            Self::Tick => "DomainMessage::Tick",
            Self::ClearError => "DomainMessage::ClearError",
            Self::Event(_) => "DomainMessage::Event",
        }
    }
}

impl std::fmt::Debug for DomainMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auth(msg) => write!(f, "DomainMessage::Auth({:?})", msg),
            Self::Library(msg) => {
                write!(f, "DomainMessage::Library({:?})", msg)
            }
            Self::Media(msg) => write!(f, "DomainMessage::Media({:?})", msg),
            Self::Player(msg) => write!(f, "DomainMessage::Player({:?})", msg),
            Self::Ui(msg) => write!(f, "DomainMessage::Ui({:?})", msg),
            Self::Metadata(msg) => {
                write!(f, "DomainMessage::Metadata({:?})", msg)
            }
            Self::Streaming(msg) => {
                write!(f, "DomainMessage::Streaming({:?})", msg)
            }
            Self::Settings(msg) => {
                write!(f, "DomainMessage::Settings({:?})", msg)
            }
            Self::UserManagement(msg) => {
                write!(f, "DomainMessage::UserManagement({:?})", msg)
            }
            Self::Search(msg) => write!(f, "DomainMessage::Search({:?})", msg),
            Self::Focus(msg) => write!(f, "DomainMessage::Focus({:?})", msg),
            Self::NoOp => write!(f, "DomainMessage::NoOp"),
            Self::Tick => write!(f, "DomainMessage::Tick"),
            Self::ClearError => write!(f, "DomainMessage::ClearError"),
            Self::Event(event) => {
                write!(f, "DomainMessage::Event({:?})", event)
            }
        }
    }
}

/// Cross-domain event bus for coordination
#[derive(Clone, Debug)]
pub enum CrossDomainEvent {
    // Auth events
    UserAuthenticated(User, UserPermissions),
    UserLoggedOut,
    AuthenticationComplete, // Signals auth flow is complete and app should proceed
    AuthConfigurationChanged, // Auth settings/configuration was changed
    AuthCommandRequested(crate::domains::auth::messages::AuthCommand), // Request to execute auth command
    AuthCommandCompleted(
        crate::domains::auth::messages::AuthCommand,
        crate::domains::auth::messages::AuthCommandResult,
    ), // Auth command execution completed

    // Library events
    LibraryUpdated,
    MediaListChanged,
    LibrarySelected(LibraryId),
    LibrarySelectHome, // Select all libraries (show all content)
    RequestLibraryRefresh, // Request to refresh library list
    // NOTE: Library management events moved to direct messages in Task 2.5

    // Media events
    MediaStartedPlaying(MediaFile),
    MediaStopped,
    MediaPaused,
    MediaToggleFullscreen, // Toggle fullscreen mode
    MediaPlayWithId(MediaFile, MediaID), // Play media with tracking ID

    // Player coordination events
    MediaStarted(MediaID), // Player notifies media domain of started playback

    // Window management events
    HideWindow, // Hide the application window (e.g., for external MPV)
    RestoreWindow(bool), // Restore window with fullscreen state
    SetWindowMode(iced::window::Mode), // Set specific window mode
    /// Hide the retained main window, then continue opening the already
    /// resolved integrated playback source.
    BeginIntegratedPlayback {
        request: ferrex_player_playback::messages::PlaybackRequestId,
    },
    BeginExternalPlayback {
        request: ferrex_player_playback::messages::PlaybackRequestId,
    },
    ExternalPlaybackLaunchFailed {
        request: ferrex_player_playback::messages::PlaybackRequestId,
    },
    NativePresenterAttached {
        request: ferrex_player_playback::messages::PlaybackRequestId,
    },
    NativePresenterUnavailable {
        request: ferrex_player_playback::messages::PlaybackRequestId,
        effective_target: ferrex_player_playback::contract::PlaybackTarget,
    },
    /// Playback teardown completed; dismiss any dedicated native-player host
    /// and restore the retained main window.
    PlaybackExited {
        request: Option<ferrex_player_playback::messages::PlaybackRequestId>,
    },

    WindowResized(iced::Size),
    DatabaseCleared, // Database was cleared, refresh needed
    // NOTE: Navigation events moved to direct UI messages in Task 2.3

    // Metadata events
    MetadataUpdated(MediaID),
    BatchMetadataReady(Vec<crate::infra::api_types::Media>),
    RequestBatchMetadataFetch(
        Vec<(uuid::Uuid, Vec<crate::infra::api_types::Media>)>,
    ), // Request batch metadata fetching
    MediaLoaded, // Media has been loaded and is ready

    // Additional library events
    LibraryChanged(LibraryId), // Library selection changed
    // Children updates for series/season detail views
    SeriesChildrenChanged(ferrex_core::player_prelude::SeriesID),
    SeasonChildrenChanged(ferrex_core::player_prelude::SeasonID),

    // Cleanup events for logout
    ClearMediaStore,      // Clear media store data
    ClearLibraries,       // Clear libraries and current_library_id
    ClearCurrentShowData, // Clear current show UI state (season_details, carousels)

    // ViewModels update events
    RequestViewModelRefresh, // Request UI domain to refresh all ViewModels

    // Search events
    SearchInProgress(bool), // Search is in progress (multi-consumer: UI loading state)
    // NOTE: Search command events moved to direct Search messages in Task 2.10
    NavigateToMedia(crate::infra::api_types::Media), // Navigate to selected media (UI event)
    RequestMediaDetails(crate::infra::api_types::Media), // Request details for media

    // Generic no-op event
    NoOp,
}

/// Event handler that domains can implement
pub trait DomainEventHandler {
    type Message;

    fn handle_event(
        &self,
        event: &CrossDomainEvent,
    ) -> Option<Task<Self::Message>>;
}
