use crate::common::messages::{CrossDomainEvent, DomainUpdateResult};

use crate::state_refactored::State;
use ferrex_core::player_prelude::LibraryID;
use iced::Task;

pub fn handle_select_library(
    state: &mut State,
    library_id: Option<LibraryID>,
) -> DomainUpdateResult {
    // Update library selection
    let previous_library_id = state.domains.library.state.current_library_id;
    state.domains.library.state.current_library_id = library_id;

    if library_id.is_none() {
        log::info!("Selected 'All' libraries - showing all content");
        // Emit LibrarySelectAll event to notify other domains
        DomainUpdateResult::with_events(
            Task::none(),
            vec![CrossDomainEvent::LibrarySelectAll],
        )
    } else {
        log::info!("Selected library: {}", library_id.unwrap());
        // Emit cross-domain event to notify other domains about the library change
        // This will allow UI domain to update ViewModels
        DomainUpdateResult::with_events(
            Task::none(),
            vec![CrossDomainEvent::LibraryChanged(library_id.unwrap())],
        )
    }
}

// Legacy handle_library_selected removed - using reference-based API now
