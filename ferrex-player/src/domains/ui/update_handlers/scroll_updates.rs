use std::time::{Duration, Instant};

use crate::{
    domains::ui::messages::Message,
    infrastructure::performance_config::scrolling::{FAST_SCROLL_THRESHOLD, SCROLL_STOP_DEBOUNCE_MS},
    state_refactored::State,
};
use iced::{widget::scrollable::Viewport, Task};

pub fn handle_movies_grid_scrolled(state: &mut State, viewport: Viewport) -> Task<Message> {
    // NEW ARCHITECTURE: Update scroll position in ViewModel's grid state
    let mut grid_state = state.movies_view_model.grid_state().clone();
    grid_state.update_scroll(viewport);
    state.movies_view_model.update_grid_state(grid_state);

    // Calculate scroll velocity
    let current_position = viewport.absolute_offset().y;
    let now = Instant::now();

    // Add current sample to the queue
    state.domains.ui.state.scroll_samples.push_back((now, current_position));

    // Keep only the last 5 samples
    while state.domains.ui.state.scroll_samples.len() > 5 {
        state.domains.ui.state.scroll_samples.pop_front();
    }

    // Calculate velocity if we have at least 2 samples
    if state.domains.ui.state.scroll_samples.len() >= 2 {
        let oldest = state.domains.ui.state.scroll_samples.front().unwrap();
        let newest = state.domains.ui.state.scroll_samples.back().unwrap();

        let time_delta = newest.0.duration_since(oldest.0).as_secs_f32();
        if time_delta > 0.0 {
            let position_delta = newest.1 - oldest.1;
            state.domains.ui.state.scroll_velocity = (position_delta / time_delta).abs();

            // Determine if we're fast scrolling
            let was_fast_scrolling = state.domains.ui.state.fast_scrolling;
            state.domains.ui.state.fast_scrolling = state.domains.ui.state.scroll_velocity > FAST_SCROLL_THRESHOLD;

            // Log state changes
            if was_fast_scrolling != state.domains.ui.state.fast_scrolling {
                log::info!(
                    "Scroll mode changed: {} (velocity: {:.0} px/s)",
                    if state.domains.ui.state.fast_scrolling {
                        "FAST"
                    } else {
                        "NORMAL"
                    },
                    state.domains.ui.state.scroll_velocity
                );
            }

            // Reset scroll stopped time when actively scrolling
            state.domains.ui.state.scroll_stopped_time = None;
        }
    }

    state.domains.ui.state.last_scroll_position = current_position;
    state.domains.ui.state.last_scroll_time = Some(now);

    // Visibility is already updated above with the grid state update

    // Metadata reprioritization no longer needed - batch fetching handles all items

    // Schedule a check for scroll stop
    Task::perform(
        async move {
            tokio::time::sleep(Duration::from_millis(SCROLL_STOP_DEBOUNCE_MS)).await;
        },
        |_| Message::CheckScrollStopped,
    )
}

pub fn handle_tv_shows_grid_scrolled(state: &mut State, viewport: Viewport) -> Task<Message> {
    // NEW ARCHITECTURE: Update scroll position in ViewModel's grid state
    let mut grid_state = state.tv_view_model.grid_state().clone();
    grid_state.update_scroll(viewport);
    state.tv_view_model.update_grid_state(grid_state);

    // Use same velocity tracking as movies grid
    let current_position = viewport.absolute_offset().y;
    let now = Instant::now();

    state.domains.ui.state.scroll_samples.push_back((now, current_position));
    while state.domains.ui.state.scroll_samples.len() > 5 {
        state.domains.ui.state.scroll_samples.pop_front();
    }

    if state.domains.ui.state.scroll_samples.len() >= 2 {
        let oldest = state.domains.ui.state.scroll_samples.front().unwrap();
        let newest = state.domains.ui.state.scroll_samples.back().unwrap();

        let time_delta = newest.0.duration_since(oldest.0).as_secs_f32();
        if time_delta > 0.0 {
            let position_delta = newest.1 - oldest.1;
            state.domains.ui.state.scroll_velocity = (position_delta / time_delta).abs();

            let was_fast_scrolling = state.domains.ui.state.fast_scrolling;
            state.domains.ui.state.fast_scrolling = state.domains.ui.state.scroll_velocity > FAST_SCROLL_THRESHOLD;

            if was_fast_scrolling != state.domains.ui.state.fast_scrolling {
                log::info!(
                    "TV scroll mode changed: {} (velocity: {:.0} px/s)",
                    if state.domains.ui.state.fast_scrolling {
                        "FAST"
                    } else {
                        "NORMAL"
                    },
                    state.domains.ui.state.scroll_velocity
                );
            }

            state.domains.ui.state.scroll_stopped_time = None;
        }
    }

    state.domains.ui.state.last_scroll_position = current_position;

    // Visibility is already updated above with the grid state update

    // Metadata reprioritization no longer needed - batch fetching handles all items
    state.domains.ui.state.last_scroll_time = Some(now);

    Task::perform(
        async move {
            tokio::time::sleep(Duration::from_millis(SCROLL_STOP_DEBOUNCE_MS)).await;
        },
        |_| Message::CheckScrollStopped,
    )
}

pub fn handle_detail_view_scrolled(state: &mut State, viewport: Viewport) -> Task<Message> {
    // Update scroll offset for fixed backdrop
    let scroll_offset = viewport.absolute_offset().y;
    log::debug!("DetailViewScrolled: Updating background shader scroll offset to {}", scroll_offset);
    state.domains.ui.state.background_shader_state.scroll_offset = scroll_offset;

    // Update depth lines to move with the scrolled content
    state.domains.ui.state.background_shader_state.update_depth_lines(
        &state.domains.ui.state.view,
        state.window_size.width,
        state.window_size.height,
        state.domains.library.state.current_library_id,
    );

    Task::none()
}
