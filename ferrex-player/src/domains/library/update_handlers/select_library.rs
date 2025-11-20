use crate::common::messages::{CrossDomainEvent, DomainUpdateResult};

use crate::state::State;
use ferrex_core::player_prelude::LibraryID;
use iced::Task;

pub fn handle_select_library(
    state: &mut State,
    library_id: Option<LibraryID>,
) -> DomainUpdateResult {
    // Update library selection
    state.domains.library.state.current_library_id = library_id;

    if let Some(library_id) = library_id {
        log::info!("Selected library: {}", library_id);
        // Emit cross-domain event to notify other domains about the library change
        DomainUpdateResult::with_events(
            Task::none(),
            vec![CrossDomainEvent::LibraryChanged(library_id)],
        )
    } else {
        log::info!("Selected 'All' libraries - showing all content");
        // Do not echo LibrarySelectAll here to avoid feedback loops.
        // The CrossDomain mediator updates library state and broadcasts
        // LibrarySelectAll to interested domains when appropriate.
        DomainUpdateResult::task(Task::none())
    }
}
