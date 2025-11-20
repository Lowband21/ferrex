use crate::{
    media_library::{self, Library},
    messages::{library::Message, CrossDomainEvent, DomainMessage},
    state::{State, ViewMode},
};
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
            state.libraries = libraries;

            // Ensure BatchMetadataFetcher is initialized before loading media
            if state.batch_metadata_fetcher.is_none() {
                log::warn!("BatchMetadataFetcher not initialized yet, initializing now");
                if let Some(api_client) = &state.api_client {
                    let batch_fetcher = std::sync::Arc::new(
                        crate::batch_metadata_fetcher::BatchMetadataFetcher::new(
                            std::sync::Arc::new(api_client.clone()),
                        ),
                    );
                    state.batch_metadata_fetcher = Some(batch_fetcher);
                    log::info!("[BatchMetadataFetcher] Initialized in LibrariesLoaded handler");
                } else {
                    log::error!("Cannot initialize BatchMetadataFetcher - no ApiClient available");
                }
            }

            if state.libraries.is_empty() {
                // No libraries configured - don't try to load media
                log::info!("No libraries configured, skipping media loading");
                state.loading = false;
                return Task::none();
            }

            // Load all enabled libraries in the background
            let library_ids: Vec<Uuid> = state
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

                // Start with "All" view to show all libraries
                // Don't set current_library_id during startup to prevent clearing data
                if state.current_library_id.is_none() {
                    state.view_mode = ViewMode::All;
                    log::info!("Starting with 'All' view to aggregate all libraries");
                }

                // Load all libraries (they'll be cached for instant switching)
                for id in library_ids {
                    tasks.push(state.load_library_media_references(id));
                }
            }

            state.error_message = None;
        }
        Err(e) => {
            log::error!("Failed to load libraries: {}", e);
            state.error_message = Some(format!("Failed to load libraries: {}", e));

            // Don't fall back to legacy loading - just show the error
            state.loading = false;
            return Task::none();
        }
    }
    Task::batch(tasks)
}

pub fn handle_load_libraries(state: &mut State) -> Task<DomainMessage> {
    let server_url = state.server_url.clone();
    Task::perform(
        media_library::fetch_libraries(server_url),
        |result| match result {
            Ok(libraries) => DomainMessage::Library(Message::LibrariesLoaded(Ok(libraries))),
            Err(e) => DomainMessage::Library(Message::LibrariesLoaded(Err(e.to_string()))),
        },
    )
}
