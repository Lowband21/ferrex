use crate::{messages::ui::Message, state::State};
use iced::Task;

pub fn handle_update_search_query(state: &mut State, query: String) -> Task<Message> {
    state.search_query = query;
    Task::none()
}

pub fn handle_execute_search(state: &mut State) -> Task<Message> {
    // TODO: Implement search functionality
    log::info!("Search query: {}", state.search_query);
    Task::none()
}
