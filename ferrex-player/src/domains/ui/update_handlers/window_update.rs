use iced::{widget::scrollable, Size, Task};

use crate::{domains::ui::messages::Message, state_refactored::State};

pub fn handle_window_resized(state: &mut State, size: Size) -> Task<Message> {
    log::debug!("Window resized to: {}x{}", size.width, size.height);

    // Grid state handling moved to ViewModels

    state.window_size = size;

    // Update all tab grids with new window dimensions
    // This ensures proper grid layout after window resize
    for tab_id in state.tab_manager.tab_ids() {
        if let Some(tab) = state.tab_manager.get_tab_mut(tab_id) {
            if let Some(grid_state) = tab.grid_state_mut() {
                // Update viewport dimensions
                grid_state.viewport_width = size.width;
                grid_state.viewport_height = size.height;
                
                // Recalculate columns based on new width
                grid_state.update_columns(size.width);
                
                // Recalculate visible range for virtual scrolling
                grid_state.calculate_visible_range();
                
                log::debug!(
                    "Updated grid for tab {:?}: {}x{}, {} columns",
                    tab_id, 
                    size.width, 
                    size.height,
                    grid_state.columns
                );
            }
        }
    }

    // Trigger scroll updates to get actual viewport dimensions from scrollable widgets
    let mut tasks = Vec::new();

    // Scroll position restoration handled by ViewModels

    // Schedule a delayed recalculation to ensure scrollable widgets have updated
    let recalc_task = Task::perform(
        async {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        },
        |_| Message::RecalculateGridsAfterResize,
    );

    tasks.push(recalc_task);

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

    if tasks.is_empty() {
        Task::none()
    } else {
        Task::batch(tasks)
    }
}
