use std::time::Instant;

use iced::Task;
use iced::widget::{operation::scroll_by, scrollable::AbsoluteOffset};

use super::messages::{Direction, MotionMessage};
use crate::domains::ui::messages::UiMessage;
use crate::domains::ui::shell_ui::Scope;
use crate::domains::ui::tabs::TabState;
use crate::domains::ui::types::ViewState;
use crate::domains::ui::update_handlers::scroll_prefetch;
use crate::domains::ui::utils::bump_keep_alive;
use crate::state::State;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn update(state: &mut State, msg: MotionMessage) -> Task<UiMessage> {
    match msg {
        MotionMessage::Start(dir) => handle_start(state, dir),
        MotionMessage::Stop(dir) => handle_stop(state, dir),
        MotionMessage::Tick(now) => handle_tick(state, now),
        MotionMessage::SetBoost(active) => handle_boost(state, active),
    }
}

fn handle_start(state: &mut State, dir: Direction) -> Task<UiMessage> {
    // Limit to library grid context
    if !matches!(state.domains.ui.state.view, ViewState::Library) {
        return Task::none();
    }
    if !matches!(state.domains.ui.state.scope, Scope::Library(_)) {
        return Task::none();
    }
    if !matches!(state.tab_manager.active_tab(), TabState::Library(_)) {
        return Task::none();
    }

    // Apply current runtime config for grid scrolling
    let cfg = super::config::MotionControllerConfig::from_runtime_config(
        &state.runtime_config,
    );
    state.domains.ui.state.motion_controller.set_config(cfg);

    let d = match dir {
        Direction::Up => -1,
        Direction::Down => 1,
    };
    state.domains.ui.state.motion_controller.start(d);
    Task::none()
}

fn handle_stop(state: &mut State, dir: Direction) -> Task<UiMessage> {
    let d = match dir {
        Direction::Up => -1,
        Direction::Down => 1,
    };
    state.domains.ui.state.motion_controller.stop_holding(d);
    Task::none()
}

fn handle_boost(state: &mut State, active: bool) -> Task<UiMessage> {
    state.domains.ui.state.motion_controller.set_boost(active);
    Task::none()
}

fn handle_tick(state: &mut State, now: Instant) -> Task<UiMessage> {
    // Only operate in Library tab with a grid
    let active_tab_id = state.tab_manager.active_tab_id();
    let Some(delta_y) = ({
        // Borrow the active library tab just long enough to read the row height.
        let Some(TabState::Library(lib_state)) =
            state.tab_manager.get_tab_mut(active_tab_id)
        else {
            return Task::none();
        };

        let grid = &lib_state.grid_state;

        if grid.columns == 0 || grid.row_height <= 0.0 {
            return Task::none();
        }

        state
            .domains
            .ui
            .state
            .motion_controller
            .tick_delta_at(now, grid.row_height.max(1.0))
    }) else {
        return Task::none();
    };

    let (scroll_id, applied_delta_y, new_y, hit_bound) = {
        let Some(TabState::Library(lib_state)) =
            state.tab_manager.get_tab_mut(active_tab_id)
        else {
            return Task::none();
        };

        let grid = &mut lib_state.grid_state;

        // Compute scroll bounds based on the last known viewport height.
        let total_rows = grid.total_items.div_ceil(grid.columns.max(1));
        let content_height = total_rows as f32 * grid.row_height;
        let viewport_h = grid.viewport_height.max(1.0);
        let max_scroll = if content_height > viewport_h {
            content_height - viewport_h
        } else {
            0.0
        };

        let old_y = grid.scroll_position;
        let new_y = (old_y + delta_y).clamp(0.0, max_scroll);
        let applied_delta_y = new_y - old_y;

        if applied_delta_y.abs() > f32::EPSILON {
            grid.scroll_position = new_y;
            grid.calculate_visible_range();
        }

        (
            grid.scrollable_id.clone(),
            applied_delta_y,
            new_y,
            applied_delta_y.abs() <= f32::EPSILON,
        )
    };

    if hit_bound {
        state.domains.ui.state.motion_controller.abort();
        return Task::none();
    }

    // Keep the app rendering during keyboard-driven scrolling, and keep the background shader
    // deterministically anchored to the same scroll position we apply to the scrollable widget.
    //
    // Note: Iced scroll operations (`scroll_by`/`scroll_to`) do not publish `on_scroll` viewports,
    // so without updating our own state we would end up with a “static wallpaper” effect.
    bump_keep_alive(state);
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
        .set_vertical_scroll_px(new_y);

    // Programmatic scroll operations do not emit `on_scroll` viewports in Iced.
    // Run the same debounced prefetch/snapshot logic used by pointer-driven scroll.
    scroll_prefetch::maybe_run_grid_scroll_prefetch(state, now);

    scroll_by::<UiMessage>(
        scroll_id,
        AbsoluteOffset {
            x: 0.0,
            y: applied_delta_y,
        },
    )
}
