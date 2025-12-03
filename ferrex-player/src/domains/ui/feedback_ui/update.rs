use std::time::Duration;

use iced::Task;

use crate::{
    common::messages::DomainUpdateResult,
    domains::ui::feedback_ui::FeedbackMessage, state::State,
};

/// Default timeout for toast notifications
const TOAST_TIMEOUT: Duration = Duration::from_secs(3);

pub fn update_feedback_ui(
    state: &mut State,
    message: FeedbackMessage,
) -> DomainUpdateResult {
    match message {
        FeedbackMessage::ClearError => {
            state.domains.ui.state.error_message = None;
            DomainUpdateResult::task(Task::none())
        }
        FeedbackMessage::ShowToast(notification) => {
            state
                .domains
                .ui
                .state
                .toast_manager
                .push(notification, TOAST_TIMEOUT);

            DomainUpdateResult::task(Task::none())
        }
        FeedbackMessage::DismissToast(id) => {
            state.domains.ui.state.toast_manager.dismiss(id);
            DomainUpdateResult::task(Task::none())
        }
        FeedbackMessage::ToastTick => {
            state.domains.ui.state.toast_manager.cleanup_expired();
            DomainUpdateResult::task(Task::none())
        }
    }
}
