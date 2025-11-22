pub mod update;

use crate::domains::ui::messages::UiMessage;

pub use update::update_feedback_ui;

#[derive(Clone)]
pub enum FeedbackMessage {
    ClearError,
}

impl From<FeedbackMessage> for UiMessage {
    fn from(msg: FeedbackMessage) -> Self {
        UiMessage::Feedback(msg)
    }
}

impl FeedbackMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::ClearError => "UI::ClearError",
        }
    }
}

impl std::fmt::Debug for FeedbackMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClearError => write!(f, "UI::ClearError"),
        }
    }
}
