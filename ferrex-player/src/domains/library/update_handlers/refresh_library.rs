use crate::domains::library::messages::LibraryMessage;
use crate::state::State;
use iced::Task;

/// Handles RefreshLibrary message
pub fn handle_refresh_library(state: &mut State) -> Task<LibraryMessage> {
    // Loading state handled at higher level

    // Refresh current library's media references
    if let Some(_library_id) = &state.domains.library.state.current_library_id {
        // Media loading handled at higher level
        Task::none()
    } else {
        // No specific library selected - refresh all enabled libraries
        // Media loading handled at higher level
        Task::none()
    }
}
