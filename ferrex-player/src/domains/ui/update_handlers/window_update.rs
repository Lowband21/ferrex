use iced::{Size, Task};

use crate::{domains::ui::messages::Message, state_refactored::State};

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_window_resized(state: &mut State, size: Size) -> Task<Message> {
    log::debug!("Window resized to: {}x{}", size.width, size.height);

    // Grid state handling moved to ViewModels

    state.window_size = size;

    // Update all tab grids with new window width
    // This only updates column count - the scrollable widget will report actual viewport dimensions
    for tab_id in state.tab_manager.tab_ids() {
        if let Some(tab) = state.tab_manager.get_tab_mut(tab_id)
            && let Some(grid_state) = tab.grid_state_mut()
        {
            // Use resize() which only updates columns based on width
            // The scrollable widget will report actual viewport dimensions via TabGridScrolled
            grid_state.resize(size.width);
        }
    }

    // TODO: This is cumbersome, fix it
    let uuid = state
        .domains
        .library
        .state
        .current_library_id
        .map(|library_id| library_id.as_uuid());

    // Update depth regions for the current view with new window size
    state
        .domains
        .ui
        .state
        .background_shader_state
        .update_depth_lines(
            &state.domains.ui.state.view,
            size.width,
            size.height,
            uuid,
        );

    Task::none()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_window_moved(
    state: &mut State,
    position: Option<iced::Point>,
) -> Task<Message> {
    // Store the window position for later use (e.g., when spawning MPV)
    if let Some(position) = position {
        log::info!("Window moved to: ({}, {})", position.x, position.y);
        state.window_position = Some(position);
    }

    Task::none()
}
