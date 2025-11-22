use iced::Task;

use crate::{
    common::messages::{CrossDomainEvent, DomainMessage},
    domains::ui::{
        messages::UiMessage, settings_ui::SettingsUiMessage, types::ViewState,
    },
    state::State,
};

pub fn handle_clear_database(state: &mut State) -> Task<DomainMessage> {
    log::info!("Clearing all database contents");
    state.domains.ui.state.show_clear_database_confirm = false; // Hide confirmation dialog
    let server_url = state.server_url.clone();
    Task::perform(
        async move {
            let client = reqwest::Client::new();
            let url = format!("{}/maintenance/clear-database", server_url);

            match client.post(&url).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        Ok(())
                    } else {
                        Err(format!("Server error: {}", response.status()))
                    }
                }
                Err(e) => Err(format!("Request failed: {}", e)),
            }
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
            state.domains.library.state.library_media_cache.clear();

            // Reset scan state
            state.loading = false;
            state.domains.library.state.active_scans.clear();
            state.domains.library.state.latest_progress.clear();

            // Clear detail view data
            state.domains.media.state.current_season_details = None;

            state.domains.ui.state.expanded_shows.clear();
            state.domains.ui.state.show_seasons_carousel = None;
            state.domains.ui.state.season_episodes_carousel = None;

            // Clear UI state
            state.domains.ui.state.hovered_media_id = None;
            state.domains.ui.state.error_message = None;

            // Reset TabManager tabs
            //state.tab_manager.clear();

            // Reset AllViewModel - it will automatically reflect empty MediaStore
            //state.all_view_model.set_library_filter(None);

            // Reset view to library (in case user was in detail view)
            state.domains.ui.state.view = ViewState::Library;

            log::info!("All local state cleared and reset");

            // Emit cross-domain event to trigger library refresh
            Task::done(DomainMessage::Event(CrossDomainEvent::DatabaseCleared))
        }
        Err(e) => {
            log::error!("Failed to clear database: {}", e);
            state.domains.ui.state.error_message =
                Some(format!("Failed to clear database: {}", e));
            Task::none()
        }
    }
}
