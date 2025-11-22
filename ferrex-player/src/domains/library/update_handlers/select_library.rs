use crate::common::messages::{CrossDomainEvent, DomainUpdateResult};

use crate::state::State;
use ferrex_core::player_prelude::LibraryId;
use iced::Task;

pub fn handle_select_library(
    state: &mut State,
    library_id: Option<LibraryId>,
) -> DomainUpdateResult {
    // Update scope based on library selection
    state.domains.ui.state.scope = match library_id {
        Some(id) => crate::domains::ui::shell_ui::Scope::Library(id),
        None => crate::domains::ui::shell_ui::Scope::Home,
    };

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
