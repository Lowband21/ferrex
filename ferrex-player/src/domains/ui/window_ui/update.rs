use crate::{
    common::messages::{DomainMessage, DomainUpdateResult},
    domains::ui::{update_handlers::window_update, window_ui::WindowUiMessage},
    state::State,
};

pub fn update_window_ui(
    state: &mut State,
    message: WindowUiMessage,
) -> DomainUpdateResult {
    match message {
        WindowUiMessage::WindowResized(size) => DomainUpdateResult::task(
            window_update::handle_window_resized(state, size)
                .map(DomainMessage::Ui),
        ),
        WindowUiMessage::WindowMoved(position) => DomainUpdateResult::task(
            window_update::handle_window_moved(state, position)
                .map(DomainMessage::Ui),
        ),
    }
}
