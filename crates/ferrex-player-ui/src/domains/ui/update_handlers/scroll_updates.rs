use crate::{
    domains::ui::{messages::UiMessage, tabs::TabState},
    state::State,
};

use iced::{Task, widget::scrollable::Viewport};
use std::time::Instant;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_detail_view_scrolled(
    state: &mut State,
    viewport: Viewport,
) -> Task<UiMessage> {
    // Update scroll offset for fixed backdrop
    let scroll_offset = viewport.absolute_offset().y;
    log::debug!(
        "DetailViewScrolled: Updating background shader scroll offset to {}",
        scroll_offset
    );
    state
        .domains
        .ui
        .state
        .background_shader_state
        .set_horizontal_scroll_px(0.0);
    state
        .domains
        .ui
        .state
        .background_shader_state
        .set_vertical_scroll_px(scroll_offset);

    // TODO: This is cumbersome, fix it
    let uuid = state
        .domains
        .ui
        .state
        .scope
        .lib_id()
        .map(|library_id| library_id.to_uuid());

    // Update depth lines to move with the scrolled content
    state
        .domains
        .ui
        .state
        .background_shader_state
        .update_depth_lines(
            &state.domains.ui.state.view,
            state.window_size.width,
            state.window_size.height,
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
pub fn handle_tab_grid_scrolled(
    state: &mut State,
    viewport: Viewport,
) -> Task<UiMessage> {
    // Keep the background shader visually attached to the scrolled grid content.
    // This prevents a “static wallpaper” effect under fast scrolling.
    let scroll_y = viewport.absolute_offset().y;
    state
        .domains
        .ui
        .state
        .background_shader_state
        .set_horizontal_scroll_px(0.0);
    state
        .domains
        .ui
        .state
        .background_shader_state
        .set_vertical_scroll_px(scroll_y);
    log::trace!("TabGridScrolled: background scroll_offset={scroll_y}");

    // Get the active tab and update its scroll state
    let active_tab_id = state.tab_manager.active_tab_id();

    // Update scroll position in the active tab's grid state (for virtual scrolling)
    if let Some(tab) = state.tab_manager.get_tab_mut(active_tab_id) {
        match tab {
            TabState::Library(lib_state) => {
                // Update the grid state for virtual scrolling calculations
                lib_state.update_scroll(viewport);
            }
            TabState::Home(_all_state) => {
                // All tab uses carousel, not virtual grid - no grid state to update
            }
        }
    }

    // Abort kinetic motion when reaching scroll bounds
    if state.domains.ui.state.motion_controller.is_active()
        && let Some(TabState::Library(lib_state)) =
            state.tab_manager.get_tab(active_tab_id)
    {
        let grid = &lib_state.grid_state;
        let scroll_y = viewport.absolute_offset().y;

        // Compute max scroll
        let total_rows = grid.total_items.div_ceil(grid.columns.max(1));
        let content_height = total_rows as f32 * grid.row_height;
        let viewport_h = viewport.bounds().height.max(1.0);
        let max_scroll = if content_height > viewport_h {
            content_height - viewport_h
        } else {
            0.0
        };

        let at_top = scroll_y <= 0.5;
        let at_bottom = scroll_y >= max_scroll - 0.5;
        let dir = state.domains.ui.state.motion_controller.direction();

        // Stop motion if scrolling into a bound
        if (at_top && dir < 0) || (at_bottom && dir > 0) {
            state.domains.ui.state.motion_controller.abort();
        }
    }

    // Save scroll position to ScrollPositionManager for persistence
    let scroll_state =
        crate::domains::ui::scroll_manager::ScrollState::from_viewport(
            viewport,
        );
    state
        .domains
        .ui
        .state
        .scroll_manager
        .save_tab_scroll(&active_tab_id, scroll_state);

    log::trace!(
        "Tab {:?} scroll saved to manager at position {}",
        active_tab_id,
        viewport.absolute_offset().y
    );

    crate::domains::ui::update_handlers::scroll_prefetch::maybe_run_grid_scroll_prefetch(
        state,
        Instant::now(),
    );

    Task::none()
}
