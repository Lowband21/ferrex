use iced::{Point, Task, window};

use crate::common::messages::{DomainMessage, DomainUpdateResult};
use crate::domains::search::types::SearchMode;
use crate::domains::ui::messages as ui;
use crate::domains::ui::windows::WindowKind;
use crate::infra::constants::layout;
use crate::state::State;

fn search_window_size() -> iced::Size {
    iced::Size::new(layout::search::WINDOW_WIDTH, layout::search::WINDOW_HEIGHT)
}

fn search_window_position(state: &State) -> window::Position {
    if let Some(origin) = state.window_position {
        let width = state.window_size.width;
        let x =
            origin.x + (width - layout::search::WINDOW_WIDTH).max(0.0) / 2.0;
        let y = origin.y
            + layout::header::HEIGHT
            + layout::search::WINDOW_VERTICAL_OFFSET;
        window::Position::Specific(Point::new(x, y))
    } else {
        window::Position::Centered
    }
}

fn search_window_settings(state: &State) -> window::Settings {
    window::Settings {
        size: search_window_size(),
        position: search_window_position(state),
        resizable: false,
        decorations: true,
        transparent: true,
        level: window::Level::AlwaysOnTop,
        exit_on_close_request: false,
        ..Default::default()
    }
}

pub fn open_search(
    state: &mut State,
    seed: Option<String>,
) -> DomainUpdateResult {
    state.domains.search.state.set_mode(SearchMode::Dropdown);

    if let Some(existing_id) = state.windows.get(WindowKind::Search) {
        state.search_window_id = Some(existing_id);
        let mut tasks: Vec<Task<DomainMessage>> = Vec::new();

        if let Some(seed) = seed {
            tasks.push(
                super::super::update_handlers::search_updates::update_search_query(state, seed)
                    .task,
            );
        }

        tasks.push(window::gain_focus(existing_id));
        tasks.push(super::focus::focus_search_window_input());

        return DomainUpdateResult::task(Task::batch(tasks));
    }

    let mut tasks: Vec<Task<DomainMessage>> = Vec::new();

    if let Some(seed) = seed {
        tasks.push(
            super::super::update_handlers::search_updates::update_search_query(
                state, seed,
            )
            .task,
        );
    }

    let (id, open) = window::open(search_window_settings(state));
    state.windows.set(WindowKind::Search, id);
    state.search_window_id = Some(id);

    tasks.push(open.map(|opened| {
        DomainMessage::Ui(ui::UiMessage::SearchWindowOpened(opened))
    }));

    DomainUpdateResult::task(Task::batch(tasks))
}

pub fn on_search_opened(
    _state: &mut State,
    id: window::Id,
) -> DomainUpdateResult {
    let focus_input = super::focus::focus_search_window_input();
    let focus_window = window::gain_focus(id);
    let set_top = window::set_level(id, window::Level::AlwaysOnTop);

    DomainUpdateResult::task(Task::batch([set_top, focus_window, focus_input]))
}

pub fn focus_search(state: &State) -> DomainUpdateResult {
    if let Some(id) = state.search_window_id {
        DomainUpdateResult::task(Task::batch([
            window::gain_focus(id),
            super::focus::focus_search_window_input(),
        ]))
    } else {
        DomainUpdateResult::task(Task::none())
    }
}

pub fn focus_search_input(state: &State) -> DomainUpdateResult {
    if state.search_window_id.is_some() {
        DomainUpdateResult::task(super::focus::focus_search_window_input())
    } else {
        DomainUpdateResult::task(Task::none())
    }
}

pub fn close_search(state: &mut State) -> DomainUpdateResult {
    if let Some(id) = state.search_window_id.take() {
        state.domains.search.state.set_mode(SearchMode::Dropdown);
        let mut tasks: Vec<Task<DomainMessage>> = Vec::new();
        tasks.push(window::close(id));
        push_main_focus(&mut tasks, state);
        DomainUpdateResult::task(Task::batch(tasks))
    } else {
        DomainUpdateResult::task(Task::none())
    }
}

pub fn on_raw_window_closed(
    state: &mut State,
    id: window::Id,
) -> DomainUpdateResult {
    let mut tasks: Vec<Task<DomainMessage>> = Vec::new();
    if let Some(kind) = state.windows.remove_by_id(id) {
        if matches!(kind, WindowKind::Main) {
            return DomainUpdateResult::task(iced::exit());
        }
        if matches!(kind, WindowKind::Search) {
            state.search_window_id = None;
            push_main_focus(&mut tasks, state);
        }
    }
    if tasks.is_empty() {
        DomainUpdateResult::task(Task::none())
    } else {
        DomainUpdateResult::task(Task::batch(tasks))
    }
}

/// Queue a single focus command for the main window.
fn push_main_focus(tasks: &mut Vec<Task<DomainMessage>>, state: &State) {
    if let Some(main_id) = state.windows.get(WindowKind::Main) {
        tasks.push(window::gain_focus(main_id));
    }
}
