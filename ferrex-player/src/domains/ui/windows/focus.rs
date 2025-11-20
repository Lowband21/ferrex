use iced::Task;
use iced::widget::Id;
use iced::widget::operation::focus;

use crate::common::messages::DomainMessage;
use crate::state::State;

/// Identifier for the dedicated floating search window input.
pub const SEARCH_WINDOW_INPUT_ID: &str = "search-window-input";

/// Creates a task that focuses whichever search input should be active for the
/// current UI state. This keeps downstream call sites agnostic of the concrete widget
/// identifiers and makes it easier to extend focus rules later on.
pub fn focus_active_search_input(state: &State) -> Task<DomainMessage> {
    if state.search_window_id.is_some() {
        focus::<DomainMessage>(Id::new(SEARCH_WINDOW_INPUT_ID))
    } else {
        Task::none()
    }
}

/// Creates a task that focuses the dedicated search window input.
pub fn focus_search_window_input() -> Task<DomainMessage> {
    focus::<DomainMessage>(Id::new(SEARCH_WINDOW_INPUT_ID))
}
