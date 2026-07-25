use iced::{Point, Task, window};

use crate::{
    common::messages::{DomainMessage, DomainUpdateResult},
    domains::{
        player::messages::PlayerMessage,
        search::types::SearchPresentation,
        ui::{
            shell_ui::UiShellMessage,
            windows::{PlayerOverlayWindowState, WindowKind},
        },
    },
    infra::constants::layout,
    state::State,
};

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

fn player_overlay_window_settings(state: &State) -> window::Settings {
    window::Settings {
        size: state.window_size,
        position: state
            .window_position
            .map(window::Position::Specific)
            .unwrap_or(window::Position::Centered),
        // The host must exist before any native relationship is attached, but
        // must not flash or claim a second visible player identity first.
        visible: false,
        // Native-root presenters are the sole geometry authority after attach.
        resizable: false,
        decorations: false,
        transparent: true,
        level: window::Level::Normal,
        exit_on_close_request: false,
        ..Default::default()
    }
}

/// Allocate the dedicated controls overlay in a hidden state.
///
/// Showing it is a separate operation that must only happen after a presenter
/// confirms native attachment.
pub fn open_player_overlay(state: &mut State) -> DomainUpdateResult {
    if let Some(id) = state.windows.get(WindowKind::PlayerOverlay) {
        return if state.windows.player_overlay_state()
            == PlayerOverlayWindowState::Active
        {
            DomainUpdateResult::task(window::gain_focus(id))
        } else {
            DomainUpdateResult::task(Task::none())
        };
    }

    let (id, open) = window::open(player_overlay_window_settings(state));
    state.windows.set(WindowKind::PlayerOverlay, id);
    state.windows.set_player_overlay_size(id, state.window_size);
    DomainUpdateResult::task(open.map(|opened| {
        DomainMessage::Ui(UiShellMessage::PlayerOverlayOpened(opened).into())
    }))
}

/// Keep a newly allocated overlay hidden while native attachment is pending.
pub fn on_player_overlay_opened(
    state: &mut State,
    id: window::Id,
) -> DomainUpdateResult {
    if state.windows.get(WindowKind::PlayerOverlay) == Some(id) {
        log::debug!(
            "native player overlay handoff: hidden controls host allocated"
        );
        DomainUpdateResult::task(Task::none())
    } else {
        // A superseded asynchronous open result must not leave an orphan
        // controls window visible later.
        DomainUpdateResult::task(window::close(id))
    }
}

/// Reveal the overlay only after native attachment has been confirmed.
pub fn activate_player_overlay(state: &mut State) -> DomainUpdateResult {
    let Some(_overlay_id) = state.windows.get(WindowKind::PlayerOverlay) else {
        return DomainUpdateResult::task(Task::none());
    };
    if !state.windows.activate_player_overlay() {
        return DomainUpdateResult::task(Task::none());
    }
    log::debug!(
        "native player overlay handoff: presenter attached and positioned; queuing retained main-window hide"
    );

    // The platform presenter already positioned the hidden overlay against
    // mpv's content rectangle. Hide the retained main window first, then let a
    // follow-up update reveal that exact native window without overwriting its
    // geometry from stale main-window state.
    let handoff: Task<DomainMessage> = state
        .windows
        .get(WindowKind::Main)
        .map(|main_id| window::set_mode(main_id, window::Mode::Hidden))
        .unwrap_or_else(Task::none)
        .chain(Task::done(DomainMessage::Ui(
            UiShellMessage::PlayerOverlayHandoffReady.into(),
        )));

    DomainUpdateResult::task(handoff)
}

/// Reveal the native-positioned overlay after the retained main window has
/// completed its hide command.
pub fn finish_player_overlay_activation(
    state: &mut State,
) -> DomainUpdateResult {
    let Some(overlay_id) = state.windows.get(WindowKind::PlayerOverlay) else {
        return DomainUpdateResult::task(Task::none());
    };
    if state.windows.player_overlay_state()
        != PlayerOverlayWindowState::Activating
    {
        return DomainUpdateResult::task(Task::none());
    }
    log::debug!(
        "native player overlay handoff: retained main-window hide completed"
    );

    let presenter_visible = state
        .domains
        .player
        .state
        .video_opt
        .as_mut()
        .is_some_and(|video| video.set_native_presenter_host_visible(true));
    if !presenter_visible {
        return dismiss_player_overlay(state);
    }
    if !state.windows.finish_player_overlay_activation() {
        return dismiss_player_overlay(state);
    }
    log::debug!(
        "native player overlay handoff: presenter host visible; overlay focus requested"
    );

    DomainUpdateResult::task(window::gain_focus(overlay_id))
}

/// Retain an observable, pointer-free confirmation that the platform delivered
/// focus after the native host became visible.
pub fn on_player_overlay_focused(state: &State) -> DomainUpdateResult {
    if state.windows.player_overlay_state() == PlayerOverlayWindowState::Active
    {
        log::debug!("native player overlay handoff: overlay focus confirmed");
    }
    DomainUpdateResult::task(Task::none())
}

fn restore_main_window(state: &State) -> Task<DomainMessage> {
    let Some(main_id) = state.windows.get(WindowKind::Main) else {
        return Task::none();
    };

    let mut restore: Task<DomainMessage> =
        window::resize(main_id, state.window_size);
    if let Some(position) = state.window_position {
        restore = restore.chain(window::move_to(main_id, position));
    }
    let mode = if state.is_fullscreen {
        window::Mode::Fullscreen
    } else {
        window::Mode::Windowed
    };
    restore
        .chain(window::set_mode(main_id, mode))
        .chain(window::gain_focus(main_id))
}

fn prepare_player_overlay_close(id: window::Id) {
    let result = ferrex_player_playback::native_video_slot::
        prepare_iced_native_host_close(id);
    log::debug!(
        "Prepared player overlay close: detached_slots={}, released_host={}",
        result.detached_slots,
        result.released_host
    );
}

fn close_player_overlay_with_policy(
    state: &mut State,
    stop_playback: bool,
) -> DomainUpdateResult {
    let Some(id) = state.windows.get(WindowKind::PlayerOverlay) else {
        return DomainUpdateResult::task(Task::none());
    };
    let previous = state.windows.begin_player_overlay_close();
    if previous == PlayerOverlayWindowState::Closing {
        return DomainUpdateResult::task(Task::none());
    }

    // Presenter detach and raw-handle release happen synchronously in this
    // update, before the native close action can be processed.
    prepare_player_overlay_close(id);

    let mut close: Task<DomainMessage> = window::close(id);
    let handoff_started = matches!(
        previous,
        PlayerOverlayWindowState::Activating | PlayerOverlayWindowState::Active
    );
    if handoff_started {
        close = close.chain(restore_main_window(state));
    }

    let mut tasks = vec![close];
    if stop_playback && handoff_started {
        tasks.push(Task::done(DomainMessage::Player(PlayerMessage::Stop)));
    }
    DomainUpdateResult::task(Task::batch(tasks))
}

/// Close a user-visible player overlay, stop playback, and restore the main
/// window geometry/focus. Presenter fallback can use
/// [`dismiss_player_overlay`] without stopping the replacement backend.
pub fn close_player_overlay(state: &mut State) -> DomainUpdateResult {
    close_player_overlay_with_policy(state, true)
}

/// Remove the overlay while allowing playback fallback to continue.
pub fn dismiss_player_overlay(state: &mut State) -> DomainUpdateResult {
    close_player_overlay_with_policy(state, false)
}

pub fn open_search(
    state: &mut State,
    seed: Option<String>,
) -> DomainUpdateResult {
    state.domains.search.state.presentation =
        SearchPresentation::DetachedWindow;

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
        DomainMessage::Ui(UiShellMessage::SearchDetachedOpened(opened).into())
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
    if state.domains.search.state.presentation.is_open() {
        DomainUpdateResult::task(super::focus::focus_active_search_input(state))
    } else {
        DomainUpdateResult::task(Task::none())
    }
}

pub fn close_search(state: &mut State) -> DomainUpdateResult {
    if let Some(id) = state.search_window_id.take() {
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
    let overlay_state = state.windows.player_overlay_state();
    if let Some(kind) = state.windows.remove_by_id(id) {
        if matches!(kind, WindowKind::Main) {
            return DomainUpdateResult::task(iced::exit());
        }
        if matches!(kind, WindowKind::Search) {
            state.search_window_id = None;
            state.domains.search.state.presentation =
                SearchPresentation::Hidden;
            push_main_focus(&mut tasks, state);
        }
        if matches!(kind, WindowKind::PlayerOverlay) {
            // Normally this is idempotent because the ClosePlayerOverlay
            // update performed teardown before queueing `window::close`.
            prepare_player_overlay_close(id);
            if matches!(
                overlay_state,
                PlayerOverlayWindowState::Activating
                    | PlayerOverlayWindowState::Active
            ) {
                // Defensive path for a platform that bypassed CloseRequested.
                tasks.push(Task::done(DomainMessage::Player(
                    PlayerMessage::Stop,
                )));
                tasks.push(restore_main_window(state));
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn player_overlay_is_transparent_undecorated_and_initially_hidden() {
        let state = State::default();
        let settings = player_overlay_window_settings(&state);

        assert!(!settings.visible);
        assert!(settings.transparent);
        assert!(!settings.decorations);
        assert!(!settings.resizable);
        assert!(!settings.exit_on_close_request);
        assert_eq!(settings.level, window::Level::Normal);
        assert_eq!(settings.size, state.window_size);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn opening_overlay_records_hidden_lifecycle_before_task_completion() {
        let mut state = State::default();
        let result = open_player_overlay(&mut state);

        assert!(state.windows.get(WindowKind::PlayerOverlay).is_some());
        assert_eq!(
            state.windows.player_overlay_state(),
            PlayerOverlayWindowState::Hidden
        );
        drop(result);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn playback_exit_dismisses_active_overlay_without_mutating_player() {
        let mut state = State::default();
        let main = window::Id::unique();
        let overlay = window::Id::unique();
        state.windows.set(WindowKind::Main, main);
        state.windows.set(WindowKind::PlayerOverlay, overlay);
        assert!(state.windows.activate_player_overlay());
        assert!(state.windows.finish_player_overlay_activation());
        state.domains.player.state.last_valid_position = 42.5;

        let result = dismiss_player_overlay(&mut state);

        assert_eq!(
            state.windows.player_overlay_state(),
            PlayerOverlayWindowState::Closing
        );
        assert_eq!(state.domains.player.state.last_valid_position, 42.5);
        assert_eq!(state.windows.get(WindowKind::Main), Some(main));
        assert_eq!(state.windows.get(WindowKind::PlayerOverlay), Some(overlay));
        drop(result);
    }
}
