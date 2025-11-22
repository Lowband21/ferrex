pub mod subscriptions;

use crate::domains::ui::{
    background_ui::BackgroundMessage, feedback_ui::FeedbackMessage,
    header_ui::HeaderMessage, interaction_ui::InteractionMessage,
    library_ui::LibraryUiMessage, menu::PosterMenuMessage,
    playback_ui::PlaybackMessage, settings_ui::SettingsUiMessage,
    shell_ui::UiShellMessage, view_model_ui::ViewModelMessage,
    views::virtual_carousel::VirtualCarouselMessage,
    window_ui::WindowUiMessage,
};
use iced::Size;

#[derive(Clone)]
pub enum UiMessage {
    Shell(UiShellMessage),
    Interaction(InteractionMessage),

    Library(LibraryUiMessage),
    Settings(SettingsUiMessage),
    ViewModels(ViewModelMessage),
    Header(HeaderMessage),
    Playback(PlaybackMessage),
    Feedback(FeedbackMessage),

    // Virtual carousel events (new module)
    VirtualCarousel(VirtualCarouselMessage),
    // Poster menu subdomain
    PosterMenu(PosterMenuMessage),
    // Window lifecycle and movement events
    Window(WindowUiMessage),
    // Background and transition updates
    Background(BackgroundMessage),

    // No-op variant for UI elements that are not yet implemented
    NoOp,
}

impl UiMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Shell(msg) => msg.name(),
            Self::Interaction(msg) => msg.name(),
            Self::Library(msg) => msg.name(),
            Self::Settings(_) => "UI::Settings",
            Self::ViewModels(msg) => msg.name(),
            Self::Header(msg) => msg.name(),
            Self::Playback(msg) => msg.name(),
            Self::Feedback(msg) => msg.name(),

            Self::VirtualCarousel(_) => "UI::VirtualCarousel",
            Self::PosterMenu(_) => "UI::PosterMenu",
            Self::Window(msg) => msg.name(),
            Self::Background(msg) => msg.name(),

            Self::NoOp => "UI::NoOp",
        }
    }
}

impl std::fmt::Debug for UiMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shell(msg) => write!(f, "UI::Shell({:?})", msg),
            Self::Interaction(msg) => write!(f, "UI::Interaction({:?})", msg),
            Self::Library(msg) => write!(f, "UI::Library({:?})", msg),
            Self::Settings(msg) => write!(f, "UI::Settings({:?})", msg),
            Self::ViewModels(msg) => write!(f, "UI::ViewModels({:?})", msg),
            Self::Header(msg) => write!(f, "UI::Header({:?})", msg),
            Self::Playback(msg) => write!(f, "UI::Playback({:?})", msg),
            Self::Feedback(msg) => write!(f, "UI::Feedback({:?})", msg),

            Self::VirtualCarousel(msg) => {
                write!(f, "UI::VirtualCarousel({:?})", msg)
            }
            Self::PosterMenu(msg) => write!(f, "UI::PosterMenu({:?})", msg),
            Self::Window(msg) => write!(f, "UI::Window({:?})", msg),
            Self::Background(msg) => write!(f, "UI::Background({:?})", msg),
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
