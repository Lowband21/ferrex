//! UI-agnostic library update logic.
//!
//! Reducers in this module mutate `LibraryDomainState` and produce domain tasks
//! without depending on the desktop player's root state or view layer.

use crate::{
    LibrariesLoadState,
    messages::LibraryMessage,
    update_handlers::library_loaded::{
        LibrariesLoadedContext, fetch_libraries_list, handle_libraries_loaded,
    },
};
use ferrex_player_api::services::api::ApiService;
use ferrex_player_foundation::domain::{DomainTask, DomainUpdateResult};
use std::sync::Arc;

/// App-shell hooks required by the extracted library state machine.
pub trait LibraryUpdateContext: LibrariesLoadedContext {
    type AppMessage: Send + 'static;

    fn library_message(message: LibraryMessage) -> Self::AppMessage;
    fn is_authenticated(&self) -> bool;
    fn api_service(&self) -> Arc<dyn ApiService>;
    fn set_libraries_for_navigation(
        &mut self,
        libraries: &[ferrex_core::player_prelude::Library],
    );

    fn fetch_libraries_bootstrap_task(
        &self,
        api_service: Arc<dyn ApiService>,
        libraries: Vec<ferrex_core::player_prelude::Library>,
    ) -> DomainTask<Self::AppMessage> {
        DomainTask::perform(
            crate::update_handlers::library_loaded::fetch_libraries_bootstrap(
                api_service,
                libraries,
            ),
            |result| {
                Self::library_message(LibraryMessage::LibrariesLoaded(
                    result.map_err(|err| err.to_string()),
                ))
            },
        )
    }
}

/// Context-based library update entry point for state-machine transitions that
/// no longer need to import the final desktop root `State`.
pub fn update_library<C>(
    context: &mut C,
    message: LibraryMessage,
) -> DomainUpdateResult<DomainTask<C::AppMessage>, ()>
where
    C: LibraryUpdateContext + 'static,
{
    match message {
        LibraryMessage::LoadLibraries => {
            if !context.is_authenticated() {
                log::info!(
                    "[Library] Ignoring LoadLibraries: user not authenticated yet"
                );
                return DomainUpdateResult::task(DomainTask::none());
            }

            let current_user_id = context.session_user_id();
            let current_server = context.server_url().to_owned();
            let api = context.api_service();
            let load_state = context.library_state_mut().load_state.clone();

            let task = match load_state {
                LibrariesLoadState::NotStarted
                | LibrariesLoadState::Failed { .. } => {
                    context.library_state_mut().load_state =
                        LibrariesLoadState::InProgress;
                    DomainTask::perform(fetch_libraries_list(api), |result| {
                        C::library_message(LibraryMessage::LibrariesListLoaded(
                            result.map_err(|e| format!("{e:#}")),
                        ))
                    })
                }
                LibrariesLoadState::InProgress => DomainTask::none(),
                LibrariesLoadState::Succeeded {
                    user_id,
                    server_url,
                } => {
                    let same_user = user_id.is_some()
                        && current_user_id.is_some()
                        && user_id == current_user_id;
                    let same_server = server_url == current_server;
                    if same_user && same_server {
                        DomainTask::none()
                    } else {
                        context.library_state_mut().load_state =
                            LibrariesLoadState::InProgress;
                        DomainTask::perform(
                            fetch_libraries_list(api),
                            |result| {
                                C::library_message(
                                    LibraryMessage::LibrariesListLoaded(
                                        result.map_err(|e| format!("{e:#}")),
                                    ),
                                )
                            },
                        )
                    }
                }
            };

            DomainUpdateResult::task(task)
        }
        LibraryMessage::LibrariesListLoaded(result) => match result {
            Ok(libraries) => {
                context.set_libraries_for_navigation(&libraries);
                let api = context.api_service();
                let task =
                    context.fetch_libraries_bootstrap_task(api, libraries);
                DomainUpdateResult::task(task)
            }
            Err(error) => {
                context.library_state_mut().load_state =
                    LibrariesLoadState::Failed { last_error: error };
                context.mark_loading(false);
                DomainUpdateResult::task(DomainTask::none())
            }
        },
        LibraryMessage::LibrariesLoaded(result) => {
            let task = handle_libraries_loaded(context, result)
                .map(C::library_message);
            DomainUpdateResult::task(task)
        }
        _ => DomainUpdateResult::task(DomainTask::none()),
    }
}
