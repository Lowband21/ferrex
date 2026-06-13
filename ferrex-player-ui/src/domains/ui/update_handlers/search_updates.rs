use crate::{
    common::messages::{DomainMessage, DomainUpdateResult},
    domains::{search::SearchMessage, ui::tabs::TabState},
    state::State,
};

use iced::{
    Task,
    widget::{operation::scroll_to, scrollable},
};

/// Apply a UI-originated query change to the search domain while maintaining UI scroll state.
pub fn update_search_query(
    state: &mut State,
    query: String,
) -> DomainUpdateResult {
    state.domains.ui.state.search_query = query.clone();

    let scroll_restore_task =
        match state.tab_manager.get_tab(state.tab_manager.active_tab_id()) {
            Some(tab) => {
                if let TabState::Library(lib_state) = tab {
                    let scroll_position = lib_state.grid_state.scroll_position;
                    let scrollable_id =
                        lib_state.grid_state.scrollable_id.clone();

                    scroll_to::<DomainMessage>(
                        scrollable_id,
                        scrollable::AbsoluteOffset {
                            x: 0.0,
                            y: scroll_position,
                        },
                    )
                } else {
                    Task::none()
                }
            }
            None => Task::none(),
        };

    DomainUpdateResult::task(Task::batch([
        Task::done(DomainMessage::Search(SearchMessage::UpdateQuery(query))),
        scroll_restore_task,
    ]))
}
