use crate::{
    domains::media::models::{SeasonDetails, TvShowDetails},
    domains::metadata::messages::Message,
    domains::ui::views::carousel::CarouselState,
    state_refactored::State,
};
use iced::Task;

/// Handle TV show loaded event
pub fn handle_tv_show_loaded(
    state: &mut State,
    show_name: String,
    result: Result<TvShowDetails, String>,
) -> Task<Message> {
    match result {
        Ok(details) => {
            log::info!("TV show details loaded for: {}", show_name);

            // Image loading is now handled by UnifiedImageService through image_for widget
            // No need to explicitly load posters here

            // Create carousel state for seasons with season card dimensions
            state.domains.ui.state.show_seasons_carousel =
                Some(CarouselState::new_with_dimensions(
                    details.seasons.len(),
                    200.0, // Season card width (Medium size)
                    15.0,  // Spacing
                ));
            if let Some(carousel) = &mut state.domains.ui.state.show_seasons_carousel {
                let available_width = state.window_size.width - 80.0;
                carousel.update_items_per_page(available_width);
            }

            Task::none()
        }
        Err(e) => {
            log::error!("Failed to load TV show details: {}", e);
            state.domains.ui.state.error_message = Some(format!("Failed to load show details: {}", e));
            Task::none()
        }
    }
}

/// Handle season loaded event
pub fn handle_season_loaded(
    state: &mut State,
    show_name: String,
    season_num: u32,
    result: Result<SeasonDetails, String>,
) -> Task<Message> {
    match result {
        Ok(details) => {
            log::info!("Season {} details loaded for: {}", season_num, show_name);

            // Image loading is now handled by UnifiedImageService through image_for widget
            // No need to explicitly load posters here
            let ui_state = &mut state.domains.ui.state;
            let media_state = &mut state.domains.media.state;

            media_state.current_season_details = Some(details.clone());
            // Create carousel state for episodes - use episode_count
            ui_state.season_episodes_carousel =
                Some(CarouselState::new(details.episode_count as usize));
            if let Some(carousel) = &mut ui_state.season_episodes_carousel {
                let available_width = state.window_size.width - 80.0;
                carousel.update_items_per_page(available_width);
            }

            Task::none()
        }
        Err(e) => {
            log::error!("Failed to load season details: {}", e);
            state.domains.ui.state.error_message = Some(format!("Failed to load season details: {}", e));
            Task::none()
        }
    }
}
