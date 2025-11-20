use iced::{widget::scrollable, Size, Task};

use crate::{
    messages::ui::Message,
    state::{State, ViewMode},
};

pub fn handle_window_resized(state: &mut State, size: Size) -> Task<Message> {
    log::debug!("Window resized to: {}x{}", size.width, size.height);

    // Grid state handling moved to ViewModels

    state.window_size = size;

    // Update ViewModels with new window size
    state
        .movies_view_model
        .update_window_size(size.width, size.height);
    state
        .tv_view_model
        .update_window_size(size.width, size.height);

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
        .background_shader_state
        .update_depth_lines(&state.view, size.width, size.height);

    if tasks.is_empty() {
        Task::none()
    } else {
        Task::batch(tasks)
    }
}
