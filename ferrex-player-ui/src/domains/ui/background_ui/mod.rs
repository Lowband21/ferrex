pub mod update;

use crate::domains::ui::messages::UiMessage;
use iced::widget::image::Handle;

pub use update::update_background_ui;

#[derive(Clone)]
pub enum BackgroundMessage {
    UpdateTransitions,
    ToggleBackdropAspectMode,
    UpdateBackdropHandle(Handle),
}

impl From<BackgroundMessage> for UiMessage {
    fn from(msg: BackgroundMessage) -> Self {
        UiMessage::Background(msg)
    }
}

impl BackgroundMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::UpdateTransitions => "UI::UpdateTransitions",
            Self::ToggleBackdropAspectMode => "UI::ToggleBackdropAspectMode",
            Self::UpdateBackdropHandle(_) => "UI::UpdateBackdropHandle",
        }
    }
}

impl std::fmt::Debug for BackgroundMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UpdateTransitions => write!(f, "UI::UpdateTransitions"),
            Self::ToggleBackdropAspectMode => {
                write!(f, "UI::ToggleBackdropAspectMode")
            }
            Self::UpdateBackdropHandle(handle) => {
                write!(f, "UI::UpdateBackdropHandle({:?})", handle)
            }
        }
    }
}
