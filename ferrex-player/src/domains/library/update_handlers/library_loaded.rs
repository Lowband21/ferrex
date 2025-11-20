use std::sync::Arc;
use std::time::Instant;

use crate::domains::library::messages::Message;
use crate::infrastructure::repository::repository::MediaRepo;
use crate::infrastructure::services::api::ApiService;
use crate::state::State;
use ferrex_core::api_routes::v1;
use iced::Task;
use rkyv::util::AlignedVec;

/// Fetch all libraries
pub async fn fetch_libraries(
    api_service: Arc<dyn ApiService>,
) -> anyhow::Result<AlignedVec> {
    let now = Instant::now();
    let bytes: AlignedVec = api_service
        .as_ref()
        .get_rkyv(v1::libraries::COLLECTION, None)
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
                        log::info!(
                            "Populated MediaRepo with {} libraries",
                            library_count
                        );
                    }

                    // Register libraries with TabManager for tab creation
                    if state
                        .domains
                        .library
                        .state
                        .repo_accessor
                        .is_initialized()
                    {
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
