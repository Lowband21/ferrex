use crate::{LibrariesLoadState, types::LibrariesBootstrapPayload};
use ferrex_core::player_prelude::Library;
use ferrex_player_api::services::api::ApiService;
use ferrex_player_foundation::{
    domain::DomainTask, repository::RepositoryResult,
};
use std::sync::Arc;

/// Minimal context required to complete a library bootstrap without importing an app root state.
pub trait LibrariesLoadedContext {
    fn library_state_mut(&mut self) -> &mut crate::LibraryDomainState;
    fn install_libraries_bootstrap(
        &mut self,
        payload: LibrariesBootstrapPayload,
    ) -> RepositoryResult<()>;
    fn mark_loading(&mut self, loading: bool);
    fn session_user_id(&self) -> Option<uuid::Uuid>;
    fn server_url(&self) -> &str;
}

pub async fn fetch_libraries_list(
    api_service: Arc<dyn ApiService>,
) -> RepositoryResult<Vec<Library>> {
    api_service.fetch_libraries().await
}

/// The extracted crate owns the bootstrap entry point; the desktop app supplies
/// disk-cache backed fetching while this crate keeps the state transition
/// independent of the final root `State`.
pub async fn fetch_libraries_bootstrap(
    api_service: Arc<dyn ApiService>,
    libraries: Vec<Library>,
) -> RepositoryResult<LibrariesBootstrapPayload> {
    let _ = api_service;
    Ok(LibrariesBootstrapPayload {
        libraries,
        movie_batches: Vec::new(),
        series_bundles: Vec::new(),
    })
}

pub fn handle_libraries_loaded<C>(
    context: &mut C,
    result: Result<LibrariesBootstrapPayload, String>,
) -> DomainTask<crate::messages::LibraryMessage>
where
    C: LibrariesLoadedContext,
{
    match result {
        Ok(payload) => {
            let libraries = payload.libraries.clone();
            match context.install_libraries_bootstrap(payload) {
                Ok(()) => {
                    let user_id = context.session_user_id();
                    let server_url = context.server_url().to_owned();
                    let state = context.library_state_mut();
                    state.libraries = libraries;
                    state.load_state = LibrariesLoadState::Succeeded {
                        user_id,
                        server_url,
                    };
                    context.mark_loading(false);
                }
                Err(err) => {
                    context.library_state_mut().load_state =
                        LibrariesLoadState::Failed {
                            last_error: err.to_string(),
                        };
                    context.mark_loading(false);
                }
            }
        }
        Err(error) => {
            context.library_state_mut().load_state =
                LibrariesLoadState::Failed { last_error: error };
            context.mark_loading(false);
        }
    }

    DomainTask::none()
}
