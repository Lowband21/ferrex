use iced::Task;

use crate::{
    common::messages::DomainUpdateResult,
    domains::ui::feedback_ui::FeedbackMessage, state::State,
};

pub fn update_feedback_ui(
    state: &mut State,
    message: FeedbackMessage,
) -> DomainUpdateResult {
    match message {
        FeedbackMessage::ClearError => {
            state.domains.ui.state.error_message = None;
            DomainUpdateResult::task(Task::none())
        }
    }
}
