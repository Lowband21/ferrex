use iced::{widget::scrollable, Size, Task};

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
        if let Some(tab) = state.tab_manager.get_tab_mut(tab_id) {
            if let Some(grid_state) = tab.grid_state_mut() {
                // Use resize() which only updates columns based on width
                // The scrollable widget will report actual viewport dimensions via TabGridScrolled
                grid_state.resize(size.width);
            }
        }
    }

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
            state.domains.library.state.current_library_id,
        );

    Task::none()
}
