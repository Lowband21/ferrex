use iced::Task;
use iced::widget::{operation::scroll_to, scrollable::AbsoluteOffset};

use super::messages::{Direction, MotionMessage};
use crate::domains::ui::messages::Message;
use crate::domains::ui::tabs::TabState;
use crate::domains::ui::types::{DisplayMode, ViewState};
use crate::state::State;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn update(state: &mut State, msg: MotionMessage) -> Task<Message> {
    match msg {
        MotionMessage::Start(dir) => handle_start(state, dir),
        MotionMessage::Stop(dir) => handle_stop(state, dir),
        MotionMessage::Tick => handle_tick(state),
        MotionMessage::SetBoost(active) => handle_boost(state, active),
    }
}

fn handle_start(state: &mut State, dir: Direction) -> Task<Message> {
    // Limit to library grid context
    if !matches!(state.domains.ui.state.view, ViewState::Library) {
        return Task::none();
    }
    if !matches!(state.domains.ui.state.display_mode, DisplayMode::Library) {
        return Task::none();
    }
    if !matches!(state.tab_manager.active_tab(), TabState::Library(_)) {
        return Task::none();
    }

    let d = match dir {
        Direction::Up => -1,
        Direction::Down => 1,
    };
    state.domains.ui.state.motion_controller.start(d);
    Task::none()
}

fn handle_stop(state: &mut State, dir: Direction) -> Task<Message> {
    let d = match dir {
        Direction::Up => -1,
        Direction::Down => 1,
    };
    state.domains.ui.state.motion_controller.stop_holding(d);
    Task::none()
}

fn handle_boost(state: &mut State, active: bool) -> Task<Message> {
    state.domains.ui.state.motion_controller.set_boost(active);
    Task::none()
}

fn handle_tick(state: &mut State) -> Task<Message> {
    // Only operate in Library tab with a grid
    let TabState::Library(lib_state) = state.tab_manager.active_tab() else {
        return Task::none();
    };

    let grid = &lib_state.grid_state;

    // Compute max scroll
    if grid.columns == 0 || grid.row_height <= 0.0 {
        return Task::none();
    }
    let total_rows = grid.total_items.div_ceil(grid.columns);
    let content_height = total_rows as f32 * grid.row_height;
    let viewport_h = grid.viewport_height.max(1.0);
    let max_scroll = if content_height > viewport_h {
        content_height - viewport_h
    } else {
        0.0
    };

    let current = grid.scroll_position;
    let next = state.domains.ui.state.motion_controller.tick(
        current,
        grid.row_height.max(1.0),
        max_scroll,
    );

    let Some(offset_y) = next else {
        return Task::none();
    };

    let id = grid.scrollable_id.clone();
    scroll_to::<Message>(
        id,
        AbsoluteOffset {
            x: 0.0,
            y: offset_y,
        },
    )
}
