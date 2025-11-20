use crate::{
    messages::metadata::Message,
    models::{SeasonDetails, TvShowDetails},
    state::State,
    views::carousel::CarouselState,
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
            state.show_seasons_carousel = Some(CarouselState::new_with_dimensions(
                details.seasons.len(),
                200.0, // Season card width (Medium size)
                15.0,  // Spacing
            ));
            if let Some(carousel) = &mut state.show_seasons_carousel {
                let available_width = state.window_size.width - 80.0;
                carousel.update_items_per_page(available_width);
            }

            Task::none()
        }
        Err(e) => {
            log::error!("Failed to load TV show details: {}", e);
            state.error_message = Some(format!("Failed to load show details: {}", e));
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

            state.current_season_details = Some(details.clone());
            // Create carousel state for episodes - use episode_count
            state.season_episodes_carousel =
                Some(CarouselState::new(details.episode_count as usize));
            if let Some(carousel) = &mut state.season_episodes_carousel {
                let available_width = state.window_size.width - 80.0;
                carousel.update_items_per_page(available_width);
            }

            Task::none()
        }
        Err(e) => {
            log::error!("Failed to load season details: {}", e);
            state.error_message = Some(format!("Failed to load season details: {}", e));
            Task::none()
        }
    }
}
