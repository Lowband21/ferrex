use crate::{messages::library::Message, state::State};
use iced::Task;

/// Handles RefreshLibrary message
pub fn handle_refresh_library(state: &mut State) -> Task<Message> {
    state.loading = true;

    // Clear loading posters set to allow retry
    state.loading_posters.clear();

    // Refresh current library's media references
    if let Some(library_id) = &state.current_library_id {
        // Refresh specific library
        state.load_library_media_references(*library_id)
    } else {
        // No specific library selected - refresh all enabled libraries
        let mut tasks = Vec::new();
        for library in state.libraries.clone().iter().filter(|lib| lib.enabled) {
            // TODO: Fix this clone
            tasks.push(state.load_library_media_references(library.id));
        }
        if tasks.is_empty() {
            state.loading = false;
            Task::none()
        } else {
            Task::batch(tasks)
        }
    }
}
