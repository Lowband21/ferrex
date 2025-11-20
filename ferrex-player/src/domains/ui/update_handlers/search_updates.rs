use crate::common::messages::{DomainMessage, DomainUpdateResult};
use crate::domains::search::{messages as search_messages, types::SearchMode};
use crate::state::State;
use iced::Task;
use iced::widget::operation::scroll_to;

/// Apply a UI-originated query change to the search domain while maintaining UI scroll state.
pub fn update_search_query(state: &mut State, query: String) -> DomainUpdateResult {
    state.domains.ui.state.search_query = query.clone();

    let scroll_restore_task =
        if let Some(tab) = state.tab_manager.get_tab(state.tab_manager.active_tab_id()) {
            if let crate::domains::ui::tabs::TabState::Library(lib_state) = tab {
                let scroll_position = lib_state.grid_state.scroll_position;
                let scrollable_id = lib_state.grid_state.scrollable_id.clone();

                scroll_to::<DomainMessage>(
                    scrollable_id,
                    iced::widget::scrollable::AbsoluteOffset {
                        x: 0.0,
                        y: scroll_position,
                    },
                )
            } else {
                Task::none()
            }
        } else {
            Task::none()
        };

    DomainUpdateResult::task(Task::batch([
        Task::done(DomainMessage::Search(
            search_messages::Message::UpdateQuery(query),
        )),
        scroll_restore_task,
    ]))
}

/// Seed a fresh dropdown search from a global keyboard interaction.
pub fn begin_search_from_keyboard(state: &mut State, seed: String) -> DomainUpdateResult {
    if seed.is_empty() {
        return DomainUpdateResult::task(Task::none());
    }

    state.domains.search.state.set_mode(SearchMode::Dropdown);
    state.domains.search.state.clear();
    state.domains.search.state.query = seed.clone();

    update_search_query(state, seed)
}
