use iced::{Task, window};

use crate::{
    common::messages::{DomainMessage, DomainUpdateResult},
    domains::{
        search::types::{SearchMode, SearchPresentation},
        ui::{
            update_handlers::search_updates,
            windows::{
                PlayerOverlayWindowState, WindowKind,
                focus::focus_active_search_input,
            },
        },
    },
    state::State,
};

fn overlay_host_window(state: &State) -> Option<window::Id> {
    if state.windows.player_overlay_state() == PlayerOverlayWindowState::Active
    {
        state.windows.get(WindowKind::PlayerOverlay)
    } else {
        state.windows.get(WindowKind::Main)
    }
}

/// Whether playback currently owns the application's single visible window.
///
/// The request-scoped shell handoff outlives the hidden Iced donor during
/// native-window fallback, so it is the durable guard against opening or
/// refocusing a detached Search `NSWindow` while mpv's root remains visible.
pub(crate) fn native_playback_requires_in_root_search(state: &State) -> bool {
    state.windows.shell_hidden_for_playback().is_some()
}

/// Whether Search may use its ordinary detached-window presentation.
///
/// During playback every Search entry point must stay inside mpv's root. This
/// predicate is shared by the reducer and view so a stale message cannot open
/// a window after the affordance has disappeared.
pub(crate) fn detached_search_allowed(state: &State) -> bool {
    !native_playback_requires_in_root_search(state)
}

/// Retire the registered detached Search host synchronously.
///
/// Removing both registries before queueing `window::close` makes a delayed
/// raw-close notification stale by construction, so it cannot hide a newer
/// in-root Search overlay.
fn retire_detached_window(state: &mut State) -> Option<window::Id> {
    let registered = state.windows.get(WindowKind::Search);
    let id = state.search_window_id.take().or(registered)?;
    if registered == Some(id) {
        state.windows.remove_by_id(id);
    }
    Some(id)
}

/// Retire a detached Search window before opening a native playback root.
///
/// This intentionally does not focus the retained main window: the caller is
/// about to hide that shell and transfer focus to the in-root player view.
pub(crate) fn close_detached_for_native_playback(
    state: &mut State,
) -> Task<DomainMessage> {
    let Some(id) = retire_detached_window(state) else {
        return Task::none();
    };

    state.domains.search.state.presentation = SearchPresentation::Hidden;
    state.domains.search.state.escape_pending = false;
    state.domains.search.state.tenfoot_keyboard.close();
    window::close(id)
}

pub fn open_overlay(
    state: &mut State,
    seed: Option<String>,
) -> DomainUpdateResult {
    let detached_to_close = if native_playback_requires_in_root_search(state) {
        retire_detached_window(state)
    } else {
        None
    };

    if detached_to_close.is_none() && state.search_window_id.is_some() {
        state.domains.search.state.presentation =
            SearchPresentation::DetachedWindow;
        state.domains.search.state.tenfoot_keyboard.close();
        return crate::domains::ui::windows::controller::focus_search(state);
    }

    state.domains.search.state.presentation = SearchPresentation::Overlay;

    if state.interface_mode.is_tenfoot() {
        state.domains.search.state.set_mode(SearchMode::FullScreen);
        state.domains.search.state.tenfoot_keyboard.open();
        if state.domains.search.state.selected_index.is_none()
            && !state.domains.search.state.results.is_empty()
        {
            state.domains.search.state.selected_index = Some(0);
        }
    }

    let mut tasks: Vec<Task<DomainMessage>> = Vec::new();
    let mut events = Vec::new();

    if let Some(id) = detached_to_close {
        tasks.push(window::close(id));
    }

    if let Some(seed) = seed {
        let update = search_updates::update_search_query(state, seed);
        tasks.push(update.task);
        events.extend(update.events);
    }

    if let Some(host_id) = overlay_host_window(state) {
        tasks.push(window::gain_focus(host_id));
    }

    tasks.push(focus_active_search_input(state));

    DomainUpdateResult::with_events(Task::batch(tasks), events)
}

pub fn pop_out(state: &mut State) -> DomainUpdateResult {
    if detached_search_allowed(state) {
        crate::domains::ui::windows::controller::open_search(state, None)
    } else {
        open_overlay(state, None)
    }
}

pub fn close(state: &mut State) -> DomainUpdateResult {
    state.domains.search.state.presentation = SearchPresentation::Hidden;
    state.domains.search.state.escape_pending = false;
    state.domains.search.state.tenfoot_keyboard.close();

    if state.search_window_id.is_some() {
        crate::domains::ui::windows::controller::close_search(state)
    } else {
        let mut tasks: Vec<Task<DomainMessage>> = Vec::new();
        if let Some(host_id) = overlay_host_window(state) {
            tasks.push(window::gain_focus(host_id));
        }
        let task = if tasks.is_empty() {
            Task::none()
        } else {
            Task::batch(tasks)
        };
        DomainUpdateResult::task(task)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrex_player_playback::messages::PlaybackRequestId;
    use iced_runtime::{
        Action, futures::futures::StreamExt, task,
        window::Action as WindowAction,
    };

    fn active_player_overlay_state() -> (State, window::Id, window::Id) {
        let mut state = State::default();
        let main = window::Id::unique();
        let overlay = window::Id::unique();
        let request = PlaybackRequestId::new(1);
        state.windows.set(WindowKind::Main, main);
        state.windows.set(WindowKind::PlayerOverlay, overlay);
        assert!(state.windows.begin_player_overlay_launch(request));
        assert!(state.windows.activate_player_overlay(request));
        assert!(state.windows.finish_player_overlay_activation(request));
        (state, main, overlay)
    }

    async fn assert_gain_focus(
        task_to_inspect: Task<DomainMessage>,
        expected: window::Id,
    ) {
        let mut actions = task::into_stream(task_to_inspect)
            .expect("search focus should emit a task");
        let mut focused = None;
        while let Some(action) = actions.next().await {
            if let Action::Window(WindowAction::GainFocus(id)) = action {
                focused = Some(id);
            }
        }
        assert_eq!(focused, Some(expected));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn search_open_and_close_focus_active_player_host_not_hidden_main() {
        let (mut state, main, overlay) = active_player_overlay_state();
        assert_eq!(overlay_host_window(&state), Some(overlay));
        assert_ne!(overlay_host_window(&state), Some(main));

        let opened = open_overlay(&mut state, None);
        assert_eq!(
            state.domains.search.state.presentation,
            SearchPresentation::Overlay
        );
        assert_gain_focus(opened.task, overlay).await;

        let closed = close(&mut state);
        assert_eq!(
            state.domains.search.state.presentation,
            SearchPresentation::Hidden
        );
        assert_gain_focus(closed.task, overlay).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn search_overlay_uses_main_for_ordinary_shell_surface() {
        let mut state = State::default();
        let main = window::Id::unique();
        let hidden_donor = window::Id::unique();
        state.windows.set(WindowKind::Main, main);
        state.windows.set(WindowKind::PlayerOverlay, hidden_donor);

        assert_eq!(
            state.windows.player_overlay_state(),
            PlayerOverlayWindowState::Hidden
        );
        assert_eq!(overlay_host_window(&state), Some(main));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn pop_out_during_native_playback_stays_inside_player_root() {
        let (mut state, _main, overlay) = active_player_overlay_state();

        assert!(!detached_search_allowed(&state));
        let result = pop_out(&mut state);

        assert_eq!(
            state.domains.search.state.presentation,
            SearchPresentation::Overlay
        );
        assert_eq!(state.search_window_id, None);
        assert_eq!(state.windows.get(WindowKind::Search), None);
        assert_gain_focus(result.task, overlay).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn in_root_search_retires_detached_host_before_refocusing_player() {
        let (mut state, _main, overlay) = active_player_overlay_state();
        let detached = window::Id::unique();
        state.windows.set(WindowKind::Search, detached);
        state.search_window_id = Some(detached);
        state.domains.search.state.presentation =
            SearchPresentation::DetachedWindow;

        let result = open_overlay(&mut state, None);

        assert_eq!(
            state.domains.search.state.presentation,
            SearchPresentation::Overlay
        );
        assert_eq!(state.search_window_id, None);
        assert_eq!(state.windows.get(WindowKind::Search), None);

        let mut actions = task::into_stream(result.task)
            .expect("rehome must close Search and focus the player root");
        let mut saw_close = false;
        let mut saw_focus = false;
        while let Some(action) = actions.next().await {
            match action {
                Action::Window(WindowAction::Close(id)) if id == detached => {
                    saw_close = true;
                }
                Action::Window(WindowAction::GainFocus(id))
                    if id == overlay =>
                {
                    saw_focus = true;
                }
                _ => {}
            }
        }
        assert!(saw_close);
        assert!(saw_focus);
    }
}
