use iced::Task;

use crate::{
    common::messages::DomainMessage,
    domains::auth::messages::AuthMessage,
    domains::ui::{
        messages::UiMessage, settings_ui::SettingsUiMessage, types::ViewState,
    },
    state::State,
};
use ferrex_core::api::types::admin::ResetDatabaseRequest;

pub fn handle_clear_database(state: &mut State) -> Task<DomainMessage> {
    log::info!("Clearing all database contents");
    state.domains.ui.state.library_maintenance_confirmation = None;
    let api = state.api_service.clone();
    Task::perform(
        async move {
            api.reset_database(ResetDatabaseRequest::clear_all_data())
                .await
                .map(|_| ())
                .map_err(|error| error.to_string())
        },
        |result| {
            DomainMessage::Ui(UiMessage::Settings(
                SettingsUiMessage::DatabaseCleared(result),
            ))
        },
    )
}

pub fn handle_database_cleared(
    state: &mut State,
    result: Result<(), String>,
) -> Task<DomainMessage> {
    state.domains.ui.state.library_maintenance_in_flight = None;
    match result {
        Ok(()) => {
            log::info!("Database cleared successfully");

            // Clear MediaRepo (new single source of truth)
            {
                let mut repo_lock = state.media_repo.write();
                if let Some(repo) = repo_lock.as_mut() {
                    repo.clear();
                }
                // Clear the repo entirely
                *repo_lock = None;
            }

            // Clear library state
            state.domains.library.state.library_form_data = None;
            state.domains.library.state.library_form_errors.clear();
            state.domains.library.state.library_form_success = None;
            state.domains.library.state.library_media_cache.clear();
            state.domains.library.state.libraries.clear();
            state.domains.library.state.load_state =
                crate::domains::library::LibrariesLoadState::NotStarted;

            // Reset scan state
            state.loading = false;
            state.domains.library.state.clear_scan_tracking();

            // Clear detail view data
            state.domains.media.state.current_season_details = None;

            state.domains.ui.state.expanded_shows.clear();
            state.domains.ui.state.show_seasons_carousel = None;
            state.domains.ui.state.season_episodes_carousel = None;

            // Clear UI state
            state.domains.ui.state.hovered_media_id = None;
            state.domains.ui.state.error_message = None;
            state.domains.ui.state.current_library_id = None;
            state.domains.ui.state.scope =
                crate::domains::ui::shell_ui::Scope::Home;

            // Reset TabManager tabs
            //state.tab_manager.clear();

            // Reset AllViewModel - it will automatically reflect empty MediaStore
            //state.all_view_model.set_library_filter(None);

            // Reset view to library (in case user was in detail view)
            state.domains.ui.state.view = ViewState::Library;
            state.is_authenticated = false;
            state.domains.auth.state.is_authenticated = false;
            state.domains.auth.state.user_permissions = None;

            log::info!("All local state cleared and reset");

            // The full wipe includes users and sessions. Clear persisted local
            // credentials as well so the next screen is a clean setup/login flow.
            Task::done(DomainMessage::Auth(AuthMessage::ResetLocalAuthState))
        }
        Err(e) => {
            log::error!("Failed to clear database: {}", e);
            let message = format!("Failed to clear all data: {}", e);
            state.domains.ui.state.error_message = Some(message.clone());
            state.domains.library.state.library_form_errors.clear();
            state
                .domains
                .library
                .state
                .library_form_errors
                .push(message);
            Task::none()
        }
    }
}
