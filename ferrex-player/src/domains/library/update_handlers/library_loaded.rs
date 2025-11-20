use crate::common::messages::DomainMessage;
use crate::domains::library::messages::Message;
use crate::state_refactored::State;
use ferrex_core::library::Library;
use iced::Task;
use uuid::Uuid;

/// Handles LibrariesLoaded message
pub fn handle_libraries_loaded(
    state: &mut State,
    result: Result<Vec<Library>, String>,
) -> Task<Message> {
    let mut tasks: Vec<Task<Message>> = vec![];
    match result {
        Ok(libraries) => {
            log::info!("Loaded {} libraries", libraries.len());
            state.domains.library.state.libraries = libraries;

            // Clear loading flag after libraries are loaded
            state.loading = false;

            // BatchMetadataFetcher initialization handled at higher level

            if state.domains.library.state.libraries.is_empty() {
                // No libraries configured - don't try to load media
                log::info!("No libraries configured, skipping media loading");
                // Loading state handled at higher level
                return Task::none();
            }

            // Load all enabled libraries in the background
            let library_ids: Vec<Uuid> = state
                .domains
                .library
                .state
                .libraries
                .iter()
                .filter(|lib| lib.enabled)
                .map(|library| library.id)
                .collect();

            if !library_ids.is_empty() {
                log::info!(
                    "Loading {} enabled libraries in background",
                    library_ids.len()
                );

                // View mode handling moved to higher level
                if state.domains.library.state.current_library_id.is_none() {
                    log::info!("Starting with 'All' view to aggregate all libraries");
                }

                // Load media references for all enabled libraries
                for library_id in library_ids {
                    tasks.push(state.load_library_media_references(library_id));
                }
            }

            // Error handling moved to higher level
        }
        Err(e) => {
            log::error!("Failed to load libraries: {}", e);
            // Clear loading flag even on error
            state.loading = false;
            // Error handling and loading state moved to higher level
            return Task::none();
        }
    }
    Task::batch(tasks)
}

pub fn handle_load_libraries(server_url: String) -> Task<Message> {
    Task::perform(
        crate::domains::media::library::fetch_libraries(server_url),
        |result| match result {
            Ok(libraries) => Message::LibrariesLoaded(Ok(libraries)),
            Err(e) => Message::LibrariesLoaded(Err(e.to_string())),
        },
    )
}
