use crate::{
    domains::{
        auth::types::AuthenticationFlow,
        library::{LibrariesLoadState, messages::Message},
        ui::{
            update_handlers::{
                emit_initial_all_tab_snapshots_combined, init_all_tab_view,
            },
            utils::bump_keep_alive,
        },
    },
    infra::{repository::repository::MediaRepo, services::api::ApiService},
    state::State,
};

use ferrex_core::api::routes::v1;

use iced::Task;
use rkyv::util::AlignedVec;
use std::sync::Arc;
use std::time::Instant;

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

                    // Initialize All-tab (curated + per-library) and emit initial snapshots
                    init_all_tab_view(state);
                    emit_initial_all_tab_snapshots_combined(state);
                    // Keep UI active briefly to ensure initial poster loads/animations are processed
                    bump_keep_alive(state);

                    // Refresh the All tab
                    state.tab_manager.refresh_active_tab();

                    // Mark load succeeded for the current session
                    let user_id = match &state.domains.auth.state.auth_flow {
                        AuthenticationFlow::Authenticated { user, .. } => {
                            Some(user.id)
                        }
                        _ => None,
                    };
                    state.domains.library.state.load_state =
                        LibrariesLoadState::Succeeded {
                            user_id,
                            server_url: state.server_url.clone(),
                        };

                    state.loading = false;
                    Task::none()
                }
                Err(e) => {
                    log::error!("Failed to create MediaRepo: {:?}", e);
                    state.domains.library.state.load_state =
                        LibrariesLoadState::Failed {
                            last_error: format!(
                                "Failed to create MediaRepo: {:?}",
                                e
                            ),
                        };
                    state.loading = false;
                    Task::none()
                }
            }
        }
        Err(e) => {
            log::error!("Failed to load libraries: {}", e);
            state.domains.library.state.load_state =
                LibrariesLoadState::Failed { last_error: e };
            state.loading = false;
            Task::none()
        }
    }
}
