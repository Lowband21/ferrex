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
    LibraryID, MediaFile, MediaID, User, UserPermissions,
};
use iced::Task;

/// Result of a domain update operation
/// Contains both messages to be processed and events to be broadcast
#[derive(Debug)]
pub struct DomainUpdate {
    /// Messages to be processed by the domain or other domains
    pub messages: Vec<DomainMessage>,
    /// Cross-domain events to be broadcast to all domains
    pub events: Vec<CrossDomainEvent>,
}

impl DomainUpdate {
    /// Create an empty update (no messages or events)
    pub fn none() -> Self {
        Self {
            messages: Vec::new(),
            events: Vec::new(),
        }
    }

    /// Create an update with a single message
    pub fn message(msg: impl Into<DomainMessage>) -> Self {
        Self {
            messages: vec![msg.into()],
            events: Vec::new(),
        }
    }

    /// Create an update with a single event
    pub fn event(event: CrossDomainEvent) -> Self {
        Self {
            messages: Vec::new(),
            events: vec![event],
        }
    }

    /// Create an update with messages and events
    pub fn with(
        messages: Vec<DomainMessage>,
        events: Vec<CrossDomainEvent>,
    ) -> Self {
        Self { messages, events }
    }

    /// Add a message to this update
    pub fn add_message(mut self, msg: impl Into<DomainMessage>) -> Self {
        self.messages.push(msg.into());
        self
    }

    /// Add an event to this update
    pub fn add_event(mut self, event: CrossDomainEvent) -> Self {
        self.events.push(event);
        self
    }

    /// Check if this update contains any messages or events
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty() && self.events.is_empty()
    }
}

/// Result of a domain update that includes both a task and events to emit
pub struct DomainUpdateResult {
    /// The task to execute (may produce more messages)
    pub task: Task<DomainMessage>,
    /// Events to broadcast to other domains immediately
    pub events: Vec<CrossDomainEvent>,
}

impl DomainUpdateResult {
    /// Create a result with just a task
    pub fn task(task: Task<DomainMessage>) -> Self {
        Self {
            task,
            events: Vec::new(),
        }
    }

    /// Create a result with task and events
    pub fn with_events(
        task: Task<DomainMessage>,
        events: Vec<CrossDomainEvent>,
    ) -> Self {
        Self { task, events }
    }

    /// Add an event to this result
    pub fn add_event(mut self, event: CrossDomainEvent) -> Self {
        self.events.push(event);
        self
    }
}

/// The main domain message router
#[derive(Clone)]
pub enum DomainMessage {
    /// Authentication domain
    Auth(auth::messages::Message),

    /// Library management domain
    Library(library::messages::Message),

    /// Media playback domain
    Media(media::messages::Message),

    /// Player domain
    Player(player::messages::Message),

    /// UI/View domain
    Ui(ui::messages::Message),

    /// Metadata fetching domain
    Metadata(metadata::messages::Message),

    /// Streaming/Transcoding domain
    Streaming(streaming::messages::Message),

    /// Settings domain
    Settings(settings::messages::Message),

    /// User management domain
    UserManagement(user_management::messages::Message),

    /// Search domain
    Search(search::messages::Message),

    /// Focus orchestration
    Focus(FocusMessage),

    /// Cross-domain coordination messages
    NoOp,
    Tick,
    ClearError,
    Event(CrossDomainEvent), // Cross-domain event for coordination
}

// Automatic routing from domain messages
impl From<auth::messages::Message> for DomainMessage {
    fn from(msg: auth::messages::Message) -> Self {
        DomainMessage::Auth(msg)
    }
}

impl From<library::messages::Message> for DomainMessage {
    fn from(msg: library::messages::Message) -> Self {
        DomainMessage::Library(msg)
    }
}

impl From<media::messages::Message> for DomainMessage {
    fn from(msg: media::messages::Message) -> Self {
        DomainMessage::Media(msg)
    }
}

impl From<player::messages::Message> for DomainMessage {
    fn from(msg: player::messages::Message) -> Self {
        DomainMessage::Player(msg)
    }
}

impl From<ui::messages::Message> for DomainMessage {
    fn from(msg: ui::messages::Message) -> Self {
        DomainMessage::Ui(msg)
    }
}

impl From<metadata::messages::Message> for DomainMessage {
    fn from(msg: metadata::messages::Message) -> Self {
        DomainMessage::Metadata(msg)
    }
}

impl From<streaming::messages::Message> for DomainMessage {
    fn from(msg: streaming::messages::Message) -> Self {
        DomainMessage::Streaming(msg)
    }
}

impl From<settings::messages::Message> for DomainMessage {
    fn from(msg: settings::messages::Message) -> Self {
        DomainMessage::Settings(msg)
    }
}

impl From<user_management::messages::Message> for DomainMessage {
    fn from(msg: user_management::messages::Message) -> Self {
        DomainMessage::UserManagement(msg)
    }
}

impl From<search::messages::Message> for DomainMessage {
    fn from(msg: search::messages::Message) -> Self {
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
    LibrarySelected(LibraryID),
    LibrarySelectAll, // Select all libraries (show all content)
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
    #[deprecated(note = "Transcoding is now handled within streaming domain")]
    RequestTranscoding(MediaFile),
    #[deprecated(note = "Transcoding is now handled within streaming domain")]
    TranscodingReady(url::Url), // Legacy: Streaming notifies player that stream is ready

    // UI events

    // Window management events
    HideWindow, // Hide the application window (e.g., for external MPV)
    RestoreWindow(bool), // Restore window with fullscreen state
    SetWindowMode(iced::window::Mode), // Set specific window mode

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
    LibraryChanged(LibraryID), // Library selection changed

    // NOTE: Device management events moved to direct Settings messages in Task 2.9

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
