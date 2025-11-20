pub mod auth;
pub mod cross_domain;
pub mod library;
pub mod media;
pub mod metadata;
pub mod settings;
pub mod streaming;
pub mod ui;
pub mod user_management;

use iced::Task;

/// The main domain message router
#[derive(Clone)]
pub enum DomainMessage {
    /// Authentication domain
    Auth(auth::Message),

    /// Library management domain
    Library(library::Message),

    /// Media playback domain
    Media(media::Message),

    /// UI/View domain
    Ui(ui::Message),

    /// Metadata fetching domain
    Metadata(metadata::Message),

    /// Streaming/Transcoding domain
    Streaming(streaming::Message),
    
    /// Settings domain
    Settings(settings::Message),

    /// User management domain
    UserManagement(user_management::Message),

    /// Cross-domain coordination messages
    NoOp,
    Tick,
    ClearError,
    Event(CrossDomainEvent), // Cross-domain event for coordination
}

// Automatic routing from domain messages
impl From<auth::Message> for DomainMessage {
    fn from(msg: auth::Message) -> Self {
        DomainMessage::Auth(msg)
    }
}

impl From<library::Message> for DomainMessage {
    fn from(msg: library::Message) -> Self {
        DomainMessage::Library(msg)
    }
}

impl From<media::Message> for DomainMessage {
    fn from(msg: media::Message) -> Self {
        DomainMessage::Media(msg)
    }
}

impl From<ui::Message> for DomainMessage {
    fn from(msg: ui::Message) -> Self {
        DomainMessage::Ui(msg)
    }
}

impl From<metadata::Message> for DomainMessage {
    fn from(msg: metadata::Message) -> Self {
        DomainMessage::Metadata(msg)
    }
}

impl From<streaming::Message> for DomainMessage {
    fn from(msg: streaming::Message) -> Self {
        DomainMessage::Streaming(msg)
    }
}

impl From<settings::Message> for DomainMessage {
    fn from(msg: settings::Message) -> Self {
        DomainMessage::Settings(msg)
    }
}

impl From<user_management::Message> for DomainMessage {
    fn from(msg: user_management::Message) -> Self {
        DomainMessage::UserManagement(msg)
    }
}

impl DomainMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Auth(msg) => msg.name(),
            Self::Library(msg) => msg.name(),
            Self::Media(msg) => msg.name(),
            Self::Ui(msg) => msg.name(),
            Self::Metadata(msg) => msg.name(),
            Self::Streaming(msg) => msg.name(),
            Self::Settings(msg) => msg.name(),
            Self::UserManagement(msg) => msg.name(),
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
            Self::Library(msg) => write!(f, "DomainMessage::Library({:?})", msg),
            Self::Media(msg) => write!(f, "DomainMessage::Media({:?})", msg),
            Self::Ui(msg) => write!(f, "DomainMessage::Ui({:?})", msg),
            Self::Metadata(msg) => write!(f, "DomainMessage::Metadata({:?})", msg),
            Self::Streaming(msg) => write!(f, "DomainMessage::Streaming({:?})", msg),
            Self::Settings(msg) => write!(f, "DomainMessage::Settings({:?})", msg),
            Self::UserManagement(msg) => write!(f, "DomainMessage::UserManagement({:?})", msg),
            Self::NoOp => write!(f, "DomainMessage::NoOp"),
            Self::Tick => write!(f, "DomainMessage::Tick"),
            Self::ClearError => write!(f, "DomainMessage::ClearError"),
            Self::Event(event) => write!(f, "DomainMessage::Event({:?})", event),
        }
    }
}

/// Cross-domain event bus for coordination
#[derive(Clone, Debug)]
pub enum CrossDomainEvent {
    // Auth events
    UserAuthenticated(ferrex_core::user::User, ferrex_core::rbac::UserPermissions),
    UserLoggedOut,
    AuthenticationComplete, // Signals auth flow is complete and app should proceed
    AuthConfigurationChanged, // Auth settings/configuration was changed
    AuthCommandRequested(crate::messages::auth::AuthCommand), // Request to execute auth command
    AuthCommandCompleted(crate::messages::auth::AuthCommand, crate::messages::auth::AuthCommandResult), // Auth command execution completed

    // Library events
    LibraryUpdated,
    MediaListChanged,
    LibrarySelected(uuid::Uuid),
    RequestLibraryRefresh,     // Request to refresh library list
    LibraryToggleScanProgress, // Toggle scan progress visibility

    // Library management events
    LibraryShowForm(Option<crate::media_library::Library>),
    LibraryHideForm,
    LibraryScan(uuid::Uuid),
    LibraryDelete(uuid::Uuid),
    LibraryFormUpdateName(String),
    LibraryFormUpdateType(String),
    LibraryFormUpdatePaths(String),
    LibraryFormUpdateScanInterval(String),
    LibraryFormToggleEnabled,
    LibraryFormSubmit,

    // Media events
    MediaStartedPlaying(crate::media_library::MediaFile),
    MediaStopped,
    MediaPaused,
    VideoReadyToPlay,      // Video is ready to be loaded and played
    MediaToggleFullscreen, // Toggle fullscreen mode
    MediaPlayWithId(
        crate::media_library::MediaFile,
        ferrex_core::api_types::MediaId,
    ), // Play media with tracking ID

    // UI events
    ViewChanged(crate::state::ViewMode),
    WindowResized(iced::Size),
    DatabaseCleared, // Database was cleared, refresh needed
    NavigateHome,    // Navigate back to home/library view

    // Metadata events
    MetadataUpdated(ferrex_core::MediaId),
    BatchMetadataReady(Vec<crate::api_types::MediaReference>),
}

/// Event handler that domains can implement
pub trait DomainEventHandler {
    type Message;

    fn handle_event(&self, event: &CrossDomainEvent) -> Option<Task<Self::Message>>;
}
