pub mod update;

use std::time::Instant;

use crate::{
    domains::ui::{messages::UiMessage, motion_controller::MotionMessage},
    infra::shader_widgets::poster::PosterInstanceKey,
};
use iced::widget::scrollable;

pub use update::update_interaction_ui;

#[derive(Clone)]
pub enum InteractionMessage {
    // Scrolling
    TabGridScrolled(scrollable::Viewport), // Unified scroll message for tab system
    DetailViewScrolled(scrollable::Viewport), // Scroll events in detail views

    // Home view vertical scroll + focus navigation
    HomeScrolled(scrollable::Viewport),
    HomeFocusNext,
    HomeFocusPrev,
    /// Frame-synchronized tick with timestamp from window::frames()
    HomeFocusTick(Instant),

    // Kinetic grid scrolling (arrow keys)
    KineticScroll(MotionMessage),

    // Mouse tracking for focus gating
    MouseMoved,
    MediaHovered(PosterInstanceKey),
    MediaUnhovered(PosterInstanceKey),
}

impl From<InteractionMessage> for UiMessage {
    fn from(msg: InteractionMessage) -> Self {
        UiMessage::Interaction(msg)
    }
}

impl InteractionMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::TabGridScrolled(_) => "UI::TabGridScrolled",
            Self::DetailViewScrolled(_) => "UI::DetailViewScrolled",

            Self::HomeScrolled(_) => "UI::HomeViewScrolled",
            Self::HomeFocusNext => "UI::HomeFocusNext",
            Self::HomeFocusPrev => "UI::HomeFocusPrev",
            Self::HomeFocusTick(_) => "UI::HomeFocusTick",

            Self::KineticScroll(_) => "UI::KineticScroll",

            Self::MouseMoved => "UI::MouseMoved",
            Self::MediaHovered(_) => "UI::MediaHovered",
            Self::MediaUnhovered(_) => "UI::MediaUnhovered",
        }
    }
}

impl std::fmt::Debug for InteractionMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TabGridScrolled(viewport) => {
                write!(f, "UI::TabGridScrolled({:?})", viewport)
            }
            Self::DetailViewScrolled(viewport) => {
                write!(f, "UI::DetailViewScrolled({:?})", viewport)
            }
            Self::HomeScrolled(viewport) => {
                write!(f, "UI::AllViewScrolled({:?})", viewport)
            }
            Self::HomeFocusNext => write!(f, "UI::AllFocusNext"),
            Self::HomeFocusPrev => write!(f, "UI::AllFocusPrev"),
            Self::HomeFocusTick(_) => write!(f, "UI::AllFocusTick"),
            Self::KineticScroll(_) => write!(f, "UI::KineticScroll"),
            Self::MouseMoved => write!(f, "UI::MouseMoved"),
            Self::MediaHovered(key) => {
                write!(f, "UI::MediaHovered({:?})", key.media_id)
            }
            Self::MediaUnhovered(key) => {
                write!(f, "UI::MediaUnhovered({:?})", key.media_id)
            }
        }
    }
}
