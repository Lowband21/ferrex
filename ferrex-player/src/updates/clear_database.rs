use iced::Task;

use crate::{
    messages::{ui::Message, CrossDomainEvent},
    state::{State, ViewMode, ViewState},
};

pub fn handle_clear_database(state: &mut State) -> Task<Message> {
    log::info!("Clearing all database contents");
    state.show_clear_database_confirm = false; // Hide confirmation dialog
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
        Message::DatabaseCleared,
    )
}

pub fn handle_database_cleared(state: &mut State, result: Result<(), String>) -> Task<Message> {
    match result {
        Ok(()) => {
            log::info!("Database cleared successfully");

            // Clear MediaStore (single source of truth)
            if let Ok(mut store) = state.media_store.write() {
                store.clear();
            }

            // Clear library data
            state.libraries.clear();
            state.current_library_id = None;
            state.library_form_data = None;
            state.library_form_errors.clear();
            state.library_media_cache.clear();

            // Reset scan state
            state.scanning = false;
            state.loading = false;
            state.active_scan_id = None;
            state.scan_progress = None;
            state.show_scan_progress = false;

            // Clear detail view data
            state.current_season_details = None;
            state.expanded_shows.clear();
            state.show_seasons_carousel = None;
            state.season_episodes_carousel = None;

            // Clear UI state
            state.hovered_media_id = None;
            state.error_message = None;

            // Reset scroll positions
            state.movies_scroll_position = None;
            state.tv_shows_scroll_position = None;
            state.last_scroll_position = 0.0;
            state.scroll_velocity = 0.0;
            state.fast_scrolling = false;
            state.scroll_stopped_time = None;
            state.scroll_samples.clear();

            // Reset ViewModels - they will automatically reflect empty MediaStore
            state.all_view_model.set_library_filter(None);
            state.movies_view_model.set_library_filter(None);
            state.tv_view_model.set_library_filter(None);

            // Reset view to library (in case user was in detail view)
            state.view = ViewState::Library;
            state.view_mode = ViewMode::All;

            log::info!("All local state cleared and reset");

            // Emit cross-domain event to trigger library refresh
            Task::done(Message::_EmitCrossDomainEvent(
                CrossDomainEvent::DatabaseCleared,
            ))
        }
        Err(e) => {
            log::error!("Failed to clear database: {}", e);
            state.error_message = Some(format!("Failed to clear database: {}", e));
            Task::none()
        }
    }
}
