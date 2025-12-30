use iced::{Task, window};

use crate::{
    common::messages::{DomainMessage, DomainUpdateResult},
    domains::{
        search::types::SearchPresentation,
        ui::{
            update_handlers::search_updates,
            windows::{WindowKind, focus::focus_active_search_input},
        },
    },
    state::State,
};

pub fn open_overlay(
    state: &mut State,
    seed: Option<String>,
) -> DomainUpdateResult {
    if state.search_window_id.is_some() {
        state.domains.search.state.presentation =
            SearchPresentation::DetachedWindow;
        return crate::domains::ui::windows::controller::focus_search(state);
    }

    state.domains.search.state.presentation = SearchPresentation::Overlay;

    let mut tasks: Vec<Task<DomainMessage>> = Vec::new();
    let mut events = Vec::new();

    if let Some(seed) = seed {
        let update = search_updates::update_search_query(state, seed);
        tasks.push(update.task);
        events.extend(update.events);
    }

    if let Some(main_id) = state.windows.get(WindowKind::Main) {
        tasks.push(window::gain_focus(main_id));
    }

    tasks.push(focus_active_search_input(state));

    DomainUpdateResult::with_events(Task::batch(tasks), events)
}

pub fn pop_out(state: &mut State) -> DomainUpdateResult {
    crate::domains::ui::windows::controller::open_search(state, None)
}

pub fn close(state: &mut State) -> DomainUpdateResult {
    state.domains.search.state.presentation = SearchPresentation::Hidden;
    state.domains.search.state.escape_pending = false;

    if state.search_window_id.is_some() {
        crate::domains::ui::windows::controller::close_search(state)
    } else {
        let mut tasks: Vec<Task<DomainMessage>> = Vec::new();
        if let Some(main_id) = state.windows.get(WindowKind::Main) {
            tasks.push(window::gain_focus(main_id));
        }
        let task = if tasks.is_empty() {
            Task::none()
        } else {
            Task::batch(tasks)
        };
        DomainUpdateResult::task(task)
    }
}
