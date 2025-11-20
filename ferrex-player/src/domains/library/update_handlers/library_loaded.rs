use std::sync::Arc;
use std::time::Instant;

use crate::domains::library::messages::Message;
use crate::infrastructure::repository::repository::MediaRepo;
use crate::infrastructure::adapters::ApiClientAdapter;
use crate::infrastructure::constants::routes;
use crate::infrastructure::services::api::ApiService;
use crate::state_refactored::State;
use iced::Task;
use rkyv::util::AlignedVec;

/// Fetch all libraries
pub async fn fetch_libraries(api_service: Arc<ApiClientAdapter>) -> anyhow::Result<AlignedVec> {
    let now = Instant::now();
    let bytes: AlignedVec = api_service
        .as_ref()
        .get_rkyv(routes::libraries::GET, None)
        .await?;

    let elapsed = now.elapsed();
    log::info!("################");
    log::info!("Fetched libraries in {:?}", elapsed);
    log::info!("################");

    Ok(bytes)
}

/// Handles LibrariesLoaded message
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_libraries_loaded(
    state: &mut State,
    result: Result<AlignedVec, String>,
) -> Task<Message> {
    // Idempotency guard: skip if repo already populated
    if state
        .media_repo
        .read()
        .as_ref()
        .map(|r| !r.is_empty())
        .unwrap_or(false)
    {
        log::warn!("[Library] Libraries already loaded; ignoring duplicate LibrariesLoaded");
        state.loading = false;
        return Task::none();
    }

    match result {
        Ok(bytes) => {
            // Create and populate MediaRepo with the loaded data
            match MediaRepo::new(bytes) {
                Ok(media_repo) => {
                    let library_count = media_repo.len();

                    // Store MediaRepo in State
                    {
                        let mut repo_lock = state.media_repo.write();
                        *repo_lock = Some(media_repo);
                        log::info!("Populated MediaRepo with {} libraries", library_count);
                    }

                    // Register libraries with TabManager for tab creation
                    if state.domains.library.state.repo_accessor.is_initialized() {
                        state.update_tab_manager_libraries();
                        log::info!(
                            "Registered {} libraries with TabManager",
                            state
                                .domains
                                .library
                                .state
                                .repo_accessor
                                .library_count()
                                .unwrap_or(0)
                        );
                    }

                    // Refresh the All tab
                    state.tab_manager.refresh_active_tab();

                    state.loading = false;
                    Task::none()
                }
                Err(e) => {
                    log::error!("Failed to create MediaRepo: {:?}", e);
                    state.loading = false;
                    Task::none()
                }
            }
        }
        Err(e) => {
            log::error!("Failed to load libraries: {}", e);
            state.loading = false;
            Task::none()
        }
    }
}
