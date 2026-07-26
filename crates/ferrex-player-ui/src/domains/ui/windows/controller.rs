use ferrex_player_playback::messages::PlaybackRequestId;
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

fn focus_active_player_overlay(id: window::Id) -> Task<DomainMessage> {
    // Callers reach this helper only after native attachment is confirmed and
    // the overlay lifecycle is Active. On macOS the vendored winit backend
    // detects the foreign-hosted view and makes it first responder in its
    // actual host; it never orders or activates the hidden donor NSWindow.
    window::gain_focus(id)
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
            DomainUpdateResult::task(focus_active_player_overlay(id))
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

/// Hide the retained shell before opening a resolved integrated source.
///
/// `Task::chain` is the synchronization boundary: libmpv backend creation only
/// happens in the follow-up message after Iced has completed the main-window
/// hide action. URL authorization remains outside this transition, so the app
/// stays visible while the asynchronous source request is pending.
pub fn begin_integrated_playback(
    state: &mut State,
    request: PlaybackRequestId,
) -> DomainUpdateResult {
    if !state
        .domains
        .player
        .state
        .is_resolved_playback_request(request)
    {
        log::debug!(
            "ignoring stale integrated shell handoff for request {:?}",
            request
        );
        return DomainUpdateResult::task(Task::none());
    }

    if state.windows.player_overlay_state() == PlayerOverlayWindowState::Closing
    {
        state.windows.defer_player_overlay_launch(request);
        log::debug!(
            "deferring integrated shell handoff until the closing donor is gone"
        );
        return DomainUpdateResult::task(Task::none());
    }

    // A detached Search window would remain visible after the retained main
    // shell is hidden, creating a second externally manageable window beside
    // mpv's root. Retire it before any backend-open continuation can run.
    let close_detached_search =
        crate::domains::ui::search_surface::close_detached_for_native_playback(
            state,
        );

    let allocate = if state.windows.get(WindowKind::PlayerOverlay).is_none() {
        open_player_overlay(state).task
    } else {
        Task::none()
    };

    if !state.windows.begin_player_overlay_launch(request) {
        log::debug!(
            "native player overlay launch already pending or unavailable"
        );
        return DomainUpdateResult::task(Task::none());
    }

    log::debug!(
        "native player overlay handoff: hiding retained main window before backend open"
    );
    let open = Task::done(DomainMessage::Player(
        PlayerMessage::OpenResolvedStreamSource { request },
    ));
    let hide_then_open =
        if let Some(main_id) = state.windows.get(WindowKind::Main) {
            window::set_mode(main_id, window::Mode::Hidden).chain(open)
        } else {
            log::warn!(
                "native player overlay handoff: main window is not registered"
            );
            open
        };

    DomainUpdateResult::task(
        close_detached_search.chain(allocate).chain(hide_then_open),
    )
}

fn continue_external_playback_handoff(
    state: &mut State,
    request: PlaybackRequestId,
) -> DomainUpdateResult {
    if !state
        .domains
        .player
        .state
        .is_resolved_playback_request(request)
        || !state
            .domains
            .player
            .state
            .is_external_playback_intent(request)
    {
        log::debug!(
            "retiring stale external shell handoff for request {:?}",
            request
        );
        return if state.windows.finish_shell_handoff(Some(request)) {
            DomainUpdateResult::task(restore_main_window(state))
        } else {
            DomainUpdateResult::task(Task::none())
        };
    }

    let spawn = Task::done(DomainMessage::Player(
        PlayerMessage::OpenExternalStreamSource { request },
    ));
    let hide_then_spawn = if let Some(main_id) =
        state.windows.get(WindowKind::Main)
    {
        window::set_mode(main_id, window::Mode::Hidden).chain(spawn)
    } else {
        log::warn!("external playback handoff: main window is not registered");
        spawn
    };
    DomainUpdateResult::task(hide_then_spawn)
}

/// Transfer shell ownership to an external request, then ensure an integrated
/// donor is positively closed before the retained shell hide and process spawn.
pub fn begin_external_playback(
    state: &mut State,
    request: PlaybackRequestId,
) -> DomainUpdateResult {
    if !state
        .domains
        .player
        .state
        .is_resolved_playback_request(request)
        || !state
            .domains
            .player
            .state
            .is_external_playback_intent(request)
    {
        log::debug!(
            "ignoring stale external shell handoff for request {:?}",
            request
        );
        return DomainUpdateResult::task(Task::none());
    }

    state.windows.begin_external_shell_handoff(request);
    if let Some(id) = state.windows.get(WindowKind::PlayerOverlay) {
        state.windows.defer_external_playback_launch(request);
        let previous = state.windows.begin_player_overlay_close();
        if previous == PlayerOverlayWindowState::Closing {
            return DomainUpdateResult::task(Task::none());
        }
        prepare_player_overlay_close(id);
        log::debug!(
            "external playback handoff: waiting for integrated donor raw close"
        );
        return DomainUpdateResult::task(window::close(id));
    }

    continue_external_playback_handoff(state, request)
}

/// Restore the matching retained shell before permitting internal fallback.
/// A stale failure may restore its old owner, but can never launch its backend.
pub fn recover_external_playback_launch(
    state: &mut State,
    request: PlaybackRequestId,
) -> DomainUpdateResult {
    if !state.windows.finish_shell_handoff(Some(request)) {
        log::debug!(
            "ignoring stale external launch failure for request {:?}",
            request
        );
        return DomainUpdateResult::task(Task::none());
    }

    let current = state
        .domains
        .player
        .state
        .is_resolved_playback_request(request)
        && state
            .domains
            .player
            .state
            .is_external_playback_intent(request);
    let restore = restore_main_window(state);
    if current {
        DomainUpdateResult::task(restore.chain(Task::done(
            DomainMessage::Player(
                PlayerMessage::ResumeInternalPlaybackAfterExternalLaunchFailure {
                    request,
                },
            ),
        )))
    } else {
        DomainUpdateResult::task(restore)
    }
}

/// Reveal the overlay only after native attachment has been confirmed.
pub fn activate_player_overlay(
    state: &mut State,
    request: PlaybackRequestId,
) -> DomainUpdateResult {
    if state.domains.player.state.session_playback_request != Some(request) {
        log::debug!(
            "ignoring stale native-presenter attachment for request {:?}",
            request
        );
        return DomainUpdateResult::task(Task::none());
    }
    let Some(_overlay_id) = state.windows.get(WindowKind::PlayerOverlay) else {
        return DomainUpdateResult::task(Task::none());
    };
    if !state.windows.activate_player_overlay(request) {
        return DomainUpdateResult::task(Task::none());
    }
    log::debug!(
        "native player overlay handoff: presenter attached and positioned; defensively reasserting retained main-window hide"
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
            UiShellMessage::PlayerOverlayHandoffReady { request }.into(),
        )));

    DomainUpdateResult::task(handoff)
}

/// Reveal the native-positioned overlay after the retained main window has
/// completed its hide command.
pub fn finish_player_overlay_activation(
    state: &mut State,
    request: PlaybackRequestId,
) -> DomainUpdateResult {
    if state.domains.player.state.root_shutdown_blocks_launch() {
        log::debug!(
            "deferring stale native-presenter reveal while root teardown is unresolved"
        );
        return DomainUpdateResult::task(Task::none());
    }
    let Some(overlay_id) = state.windows.get(WindowKind::PlayerOverlay) else {
        return DomainUpdateResult::task(Task::none());
    };
    if state.windows.player_overlay_state()
        != PlayerOverlayWindowState::Activating
        || state.windows.player_overlay_launch_request() != Some(request)
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
        return dismiss_player_overlay_for_effective_target(state, request);
    }
    if !state.windows.finish_player_overlay_activation(request) {
        return dismiss_player_overlay_for_effective_target(state, request);
    }
    // Attachment is now complete and the lifecycle is Active, so the
    // canonical focus task is safe to queue. The vendored macOS backend routes
    // a foreign-hosted view to its actual host's first responder without
    // ordering the hidden donor window.
    log::debug!(
        "native player overlay handoff: presenter host visible; overlay focus requested"
    );
    DomainUpdateResult::task(focus_active_player_overlay(overlay_id))
}

/// Retain an observable, pointer-free confirmation that the platform delivered
/// focus after the native host became visible.
pub fn on_player_overlay_focused(state: &mut State) -> DomainUpdateResult {
    let Some(id) = state.windows.get(WindowKind::PlayerOverlay) else {
        return DomainUpdateResult::task(Task::none());
    };
    state.windows.record_focus(id);
    if state.windows.player_overlay_state() == PlayerOverlayWindowState::Active
    {
        log::debug!("native player overlay handoff: overlay focus confirmed");
    }
    DomainUpdateResult::task(Task::none())
}

/// Clear actual-host focus when the platform reports that the player root lost
/// it. A stale unfocus cannot clear focus from a newer registered window.
pub fn on_player_overlay_unfocused(state: &mut State) -> DomainUpdateResult {
    let Some(id) = state.windows.get(WindowKind::PlayerOverlay) else {
        return DomainUpdateResult::task(Task::none());
    };
    if state.windows.record_unfocus(id) {
        log::debug!("native player overlay handoff: overlay focus released");
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

/// Close a user-visible controls donor and request playback teardown.
///
/// The retained shell is deliberately not restored here. Playback first
/// withdraws mpv's native root synchronously, then emits the request-scoped
/// `PlaybackExited` event that owns shell restoration.
pub fn close_player_overlay(
    state: &mut State,
    request: PlaybackRequestId,
) -> DomainUpdateResult {
    if state.windows.shell_hidden_for_playback() != Some(request) {
        log::debug!(
            "ignoring stale native root close for request {:?}",
            request
        );
        return DomainUpdateResult::task(Task::none());
    }
    let Some(id) = state.windows.get(WindowKind::PlayerOverlay) else {
        // A native-root CloseRequested can already be queued when a fallback
        // donor close publishes RawWindowClosed and removes its subscription.
        // The request-scoped message still owns this shell, so preserve the
        // root-close intent even though no donor remains to close.
        return DomainUpdateResult::task(Task::done(DomainMessage::Player(
            PlayerMessage::Stop,
        )));
    };
    let previous = state.windows.begin_player_overlay_close();
    if previous == PlayerOverlayWindowState::Closing {
        // `ClosePlayerOverlay` is sourced from an actual native close request,
        // not from our programmatic donor-close action. A presenter refresh can
        // race that queued request and already mark the donor Closing while
        // attempting native-window fallback. Preserve the user's root-close
        // intent by still stopping playback; otherwise fallback verification
        // can wait forever on the root that is already closing.
        return DomainUpdateResult::task(Task::done(DomainMessage::Player(
            PlayerMessage::Stop,
        )));
    }
    prepare_player_overlay_close(id);

    let mut tasks = vec![window::close(id)];
    if matches!(
        previous,
        PlayerOverlayWindowState::Launching
            | PlayerOverlayWindowState::Activating
            | PlayerOverlayWindowState::Active
    ) {
        tasks.push(Task::done(DomainMessage::Player(PlayerMessage::Stop)));
    }
    DomainUpdateResult::task(Task::batch(tasks))
}

/// Remove the overlay after playback has exited or failed, restoring the
/// retained application shell only for the request that owns its hide.
pub fn dismiss_player_overlay(
    state: &mut State,
    request: Option<PlaybackRequestId>,
) -> DomainUpdateResult {
    if request.is_none()
        && (state.windows.shell_hidden_for_playback().is_some()
            || state.windows.player_overlay_launch_request().is_some())
    {
        log::debug!(
            "ignoring requestless stale exit while a playback request owns the shell"
        );
        return DomainUpdateResult::task(Task::none());
    }
    let owner_matches = request.is_none()
        || state.windows.shell_hidden_for_playback() == request;
    let overlay_matches = request.is_none()
        || state.windows.player_overlay_launch_request() == request;
    if !owner_matches && !overlay_matches {
        log::debug!(
            "ignoring stale playback-exit shell disposition {:?}",
            request
        );
        return DomainUpdateResult::task(Task::none());
    }

    let restore = state.windows.finish_shell_handoff(request);
    let close = state.windows.get(WindowKind::PlayerOverlay).and_then(|id| {
        let previous = state.windows.begin_player_overlay_close();
        if previous == PlayerOverlayWindowState::Closing {
            None
        } else {
            prepare_player_overlay_close(id);
            Some(window::close(id))
        }
    });

    let task = match (close, restore) {
        (Some(close), true) => close.chain(restore_main_window(state)),
        (Some(close), false) => close,
        (None, true) => restore_main_window(state),
        (None, false) => Task::none(),
    };
    DomainUpdateResult::task(task)
}

/// Remove the controls donor while a normal native-mpv fallback keeps playing.
///
/// The retained Ferrex shell must remain hidden: mpv's fallback root is still
/// the sole visible/manageable playback window. Restoring the shell here would
/// create two externally visible windows.
pub fn dismiss_player_overlay_for_native_fallback(
    state: &mut State,
    request: PlaybackRequestId,
) -> DomainUpdateResult {
    if state.windows.shell_hidden_for_playback() != Some(request)
        && state.windows.player_overlay_launch_request() != Some(request)
    {
        log::debug!(
            "ignoring stale native-window fallback disposition for request {:?}",
            request
        );
        return DomainUpdateResult::task(Task::none());
    }
    let Some(id) = state.windows.get(WindowKind::PlayerOverlay) else {
        return DomainUpdateResult::task(Task::none());
    };
    let previous = state.windows.begin_player_overlay_close();
    if previous == PlayerOverlayWindowState::Closing {
        return DomainUpdateResult::task(Task::none());
    }
    prepare_player_overlay_close(id);
    DomainUpdateResult::task(window::close(id))
}

fn dismiss_player_overlay_for_effective_target(
    state: &mut State,
    request: PlaybackRequestId,
) -> DomainUpdateResult {
    let native_root_continues = state
        .domains
        .player
        .state
        .video_opt
        .as_ref()
        .is_some_and(|video| {
            state.domains.player.state.session_playback_request
                == Some(request)
                && video.snapshot().target
                    == ferrex_player_playback::contract::PlaybackTarget::MPV_NATIVE_WINDOW
        });
    if native_root_continues {
        dismiss_player_overlay_for_native_fallback(state, request)
    } else {
        dismiss_player_overlay(state, Some(request))
    }
}

pub fn open_search(
    state: &mut State,
    seed: Option<String>,
) -> DomainUpdateResult {
    if !crate::domains::ui::search_surface::detached_search_allowed(state) {
        return crate::domains::ui::search_surface::open_overlay(state, seed);
    }

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
    state: &mut State,
    id: window::Id,
) -> DomainUpdateResult {
    let registered = state.windows.get(WindowKind::Search);
    let is_current_detached = state.search_window_id == Some(id)
        && registered == Some(id)
        && state.domains.search.state.presentation
            == SearchPresentation::DetachedWindow
        && crate::domains::ui::search_surface::detached_search_allowed(state);
    if !is_current_detached {
        // The open task can complete after playback has claimed the shell or
        // after Search has moved back in-root. Retire only this exact stale
        // host and never overwrite the newer presentation/focus state.
        if state.search_window_id == Some(id) {
            state.search_window_id = None;
        }
        if registered == Some(id) {
            state.windows.remove_by_id(id);
        }
        return DomainUpdateResult::task(window::close(id));
    }

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

pub fn focus_search_input(state: &mut State) -> DomainUpdateResult {
    if state.domains.search.state.presentation
        == SearchPresentation::DetachedWindow
        && let Some(id) = state.search_window_id
    {
        state.windows.record_focus(id);
    }
    if state.domains.search.state.presentation.is_open() {
        DomainUpdateResult::task(super::focus::focus_active_search_input(state))
    } else {
        DomainUpdateResult::task(Task::none())
    }
}

pub fn close_search(state: &mut State) -> DomainUpdateResult {
    if let Some(id) = state.search_window_id.take() {
        if state.windows.get(WindowKind::Search) == Some(id) {
            state.windows.remove_by_id(id);
        }
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
    let search_was_current = state.search_window_id == Some(id);
    if let Some(kind) = state.windows.remove_by_id(id) {
        if matches!(kind, WindowKind::Main) {
            return DomainUpdateResult::task(iced::exit());
        }
        if matches!(kind, WindowKind::Search) && search_was_current {
            state.search_window_id = None;
            state.domains.search.state.presentation =
                SearchPresentation::Hidden;
            if crate::domains::ui::search_surface::detached_search_allowed(
                state,
            ) {
                push_main_focus(&mut tasks, state);
            }
        }
        if matches!(kind, WindowKind::PlayerOverlay) {
            // Normally this is idempotent because the ClosePlayerOverlay
            // update performed teardown before queueing `window::close`.
            prepare_player_overlay_close(id);
            if matches!(
                overlay_state,
                PlayerOverlayWindowState::Launching
                    | PlayerOverlayWindowState::Activating
                    | PlayerOverlayWindowState::Active
            ) {
                // Defensive path for a platform that bypassed CloseRequested.
                tasks.push(Task::done(DomainMessage::Player(
                    PlayerMessage::Stop,
                )));
            }
            if let Some(request) =
                state.windows.take_deferred_external_playback_launch()
            {
                tasks.push(
                    continue_external_playback_handoff(state, request).task,
                );
            } else if let Some(request) =
                state.windows.take_deferred_player_overlay_launch()
            {
                tasks.push(begin_integrated_playback(state, request).task);
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
    use iced_runtime::{
        Action, futures::futures::StreamExt, task,
        window::Action as WindowAction,
    };

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
    async fn active_overlay_focus_uses_canonical_window_action() {
        let donor = window::Id::unique();
        let mut actions = task::into_stream(focus_active_player_overlay(donor))
            .expect("active overlay focus should emit an action");
        assert!(matches!(
            actions.next().await,
            Some(Action::Window(WindowAction::GainFocus(id))) if id == donor
        ));
        assert!(actions.next().await.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn existing_overlay_refocus_requires_active_lifecycle() {
        let mut state = State::default();
        let overlay = window::Id::unique();
        let request = PlaybackRequestId::new(1);
        state.windows.set(WindowKind::PlayerOverlay, overlay);

        assert!(
            task::into_stream(open_player_overlay(&mut state).task).is_none()
        );
        assert!(state.windows.begin_player_overlay_launch(request));
        assert!(state.windows.activate_player_overlay(request));
        assert!(
            task::into_stream(open_player_overlay(&mut state).task).is_none()
        );

        assert!(state.windows.finish_player_overlay_activation(request));
        let mut actions =
            task::into_stream(open_player_overlay(&mut state).task)
                .expect("active overlay refocus should emit an action");
        assert!(matches!(
            actions.next().await,
            Some(Action::Window(WindowAction::GainFocus(id))) if id == overlay
        ));
        assert!(actions.next().await.is_none());
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
    async fn integrated_open_hides_main_before_emitting_backend_continuation() {
        let mut state = State::default();
        let request =
            state.domains.player.state.begin_playback_request().unwrap();
        assert!(state.domains.player.state.resolve_playback_request(request));
        let main = window::Id::unique();
        let overlay = window::Id::unique();
        state.windows.set(WindowKind::Main, main);
        state.windows.set(WindowKind::PlayerOverlay, overlay);

        let result = begin_integrated_playback(&mut state, request);
        assert_eq!(
            state.windows.player_overlay_state(),
            PlayerOverlayWindowState::Launching
        );

        let mut actions = task::into_stream(result.task)
            .expect("integrated launch should emit ordered actions");
        assert!(matches!(
            actions.next().await,
            Some(Action::Window(WindowAction::SetMode(
                id,
                window::Mode::Hidden
            ))) if id == main
        ));
        assert!(matches!(
            actions.next().await,
            Some(Action::Output(DomainMessage::Player(
                PlayerMessage::OpenResolvedStreamSource {
                    request: emitted_request
                }
            ))) if emitted_request == request
        ));
        assert!(actions.next().await.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn integrated_open_closes_detached_search_before_backend_continuation()
     {
        let mut state = State::default();
        let request =
            state.domains.player.state.begin_playback_request().unwrap();
        assert!(state.domains.player.state.resolve_playback_request(request));
        let main = window::Id::unique();
        let overlay = window::Id::unique();
        let search = window::Id::unique();
        state.windows.set(WindowKind::Main, main);
        state.windows.set(WindowKind::PlayerOverlay, overlay);
        state.windows.set(WindowKind::Search, search);
        state.search_window_id = Some(search);
        state.domains.search.state.presentation =
            SearchPresentation::DetachedWindow;

        let result = begin_integrated_playback(&mut state, request);

        assert_eq!(state.search_window_id, None);
        assert_eq!(state.windows.get(WindowKind::Search), None);
        assert_eq!(
            state.domains.search.state.presentation,
            SearchPresentation::Hidden
        );
        let mut actions = task::into_stream(result.task).expect(
            "search close, shell hide, and backend open must be ordered",
        );
        assert!(matches!(
            actions.next().await,
            Some(Action::Window(WindowAction::Close(id))) if id == search
        ));
        assert!(matches!(
            actions.next().await,
            Some(Action::Window(WindowAction::SetMode(
                id,
                window::Mode::Hidden
            ))) if id == main
        ));
        assert!(matches!(
            actions.next().await,
            Some(Action::Output(DomainMessage::Player(
                PlayerMessage::OpenResolvedStreamSource {
                    request: emitted_request
                }
            ))) if emitted_request == request
        ));
        assert!(actions.next().await.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn delayed_search_open_cannot_escape_native_player_root() {
        let mut state = State::default();
        let request =
            state.domains.player.state.begin_playback_request().unwrap();
        assert!(state.domains.player.state.resolve_playback_request(request));
        let main = window::Id::unique();
        let overlay = window::Id::unique();
        let search = window::Id::unique();
        state.windows.set(WindowKind::Main, main);
        state.windows.set(WindowKind::PlayerOverlay, overlay);
        state.windows.set(WindowKind::Search, search);
        state.search_window_id = Some(search);
        state.domains.search.state.presentation =
            SearchPresentation::DetachedWindow;

        drop(begin_integrated_playback(&mut state, request));
        drop(crate::domains::ui::search_surface::open_overlay(
            &mut state, None,
        ));
        let late = on_search_opened(&mut state, search);

        assert_eq!(
            state.domains.search.state.presentation,
            SearchPresentation::Overlay
        );
        assert_eq!(state.search_window_id, None);
        assert_eq!(state.windows.get(WindowKind::Search), None);
        let mut actions = task::into_stream(late.task)
            .expect("late detached host must be closed");
        assert!(matches!(
            actions.next().await,
            Some(Action::Window(WindowAction::Close(id))) if id == search
        ));
        assert!(actions.next().await.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn delayed_search_raw_close_cannot_hide_new_in_root_overlay() {
        let mut state = State::default();
        let request =
            state.domains.player.state.begin_playback_request().unwrap();
        assert!(state.domains.player.state.resolve_playback_request(request));
        let main = window::Id::unique();
        let overlay = window::Id::unique();
        let search = window::Id::unique();
        state.windows.set(WindowKind::Main, main);
        state.windows.set(WindowKind::PlayerOverlay, overlay);
        state.windows.set(WindowKind::Search, search);
        state.search_window_id = Some(search);
        state.domains.search.state.presentation =
            SearchPresentation::DetachedWindow;

        drop(begin_integrated_playback(&mut state, request));
        drop(crate::domains::ui::search_surface::open_overlay(
            &mut state, None,
        ));
        let late = on_raw_window_closed(&mut state, search);

        assert!(task::into_stream(late.task).is_none());
        assert_eq!(
            state.domains.search.state.presentation,
            SearchPresentation::Overlay
        );
        assert_eq!(state.search_window_id, None);
        assert_eq!(state.windows.get(WindowKind::Search), None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn external_handoff_waits_for_donor_close_before_hide_and_spawn() {
        let mut state = State::default();
        let request =
            state.domains.player.state.begin_playback_request().unwrap();
        assert!(state.domains.player.state.resolve_playback_request(request));
        assert!(
            state
                .domains
                .player
                .state
                .request_external_playback(request)
        );
        let main = window::Id::unique();
        let donor = window::Id::unique();
        state.windows.set(WindowKind::Main, main);
        state.windows.set(WindowKind::PlayerOverlay, donor);

        let begin = begin_external_playback(&mut state, request);
        assert_eq!(state.windows.shell_hidden_for_playback(), Some(request));
        assert_eq!(
            state.windows.player_overlay_state(),
            PlayerOverlayWindowState::Closing
        );
        let mut begin_actions = task::into_stream(begin.task)
            .expect("donor close must be requested");
        assert!(matches!(
            begin_actions.next().await,
            Some(Action::Window(WindowAction::Close(id))) if id == donor
        ));
        assert!(
            begin_actions.next().await.is_none(),
            "shell hide and spawn must wait for raw donor close"
        );

        let closed = on_raw_window_closed(&mut state, donor);
        let mut closed_actions = task::into_stream(closed.task)
            .expect("raw close must continue the ordered handoff");
        assert!(matches!(
            closed_actions.next().await,
            Some(Action::Window(WindowAction::SetMode(
                id,
                window::Mode::Hidden
            ))) if id == main
        ));
        assert!(matches!(
            closed_actions.next().await,
            Some(Action::Output(DomainMessage::Player(
                PlayerMessage::OpenExternalStreamSource {
                    request: emitted
                }
            ))) if emitted == request
        ));
        assert!(closed_actions.next().await.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn external_spawn_failure_restores_shell_before_internal_fallback() {
        let mut state = State::default();
        let request =
            state.domains.player.state.begin_playback_request().unwrap();
        assert!(state.domains.player.state.resolve_playback_request(request));
        assert!(
            state
                .domains
                .player
                .state
                .request_external_playback(request)
        );
        let main = window::Id::unique();
        state.windows.set(WindowKind::Main, main);
        state.windows.begin_external_shell_handoff(request);

        let result = recover_external_playback_launch(&mut state, request);
        assert_eq!(state.windows.shell_hidden_for_playback(), None);
        let mut actions = task::into_stream(result.task)
            .expect("matching launch failure must restore and fall back");
        assert!(matches!(
            actions.next().await,
            Some(Action::Window(WindowAction::Resize(id, _))) if id == main
        ));
        assert!(matches!(
            actions.next().await,
            Some(Action::Window(WindowAction::SetMode(
                id,
                window::Mode::Windowed
            ))) if id == main
        ));
        assert!(matches!(
            actions.next().await,
            Some(Action::Window(WindowAction::GainFocus(id))) if id == main
        ));
        assert!(matches!(
            actions.next().await,
            Some(Action::Output(DomainMessage::Player(
                PlayerMessage::ResumeInternalPlaybackAfterExternalLaunchFailure {
                    request: emitted
                }
            ))) if emitted == request
        ));
        assert!(actions.next().await.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn pre_attach_failure_closes_host_then_restores_main_window() {
        let mut state = State::default();
        let main = window::Id::unique();
        let overlay = window::Id::unique();
        let request = PlaybackRequestId::new(1);
        state.windows.set(WindowKind::Main, main);
        state.windows.set(WindowKind::PlayerOverlay, overlay);
        assert!(state.windows.begin_player_overlay_launch(request));

        let result = dismiss_player_overlay(&mut state, Some(request));
        assert_eq!(
            state.windows.player_overlay_state(),
            PlayerOverlayWindowState::Closing
        );

        let mut actions = task::into_stream(result.task)
            .expect("pre-attach failure should close and restore");
        assert!(matches!(
            actions.next().await,
            Some(Action::Window(WindowAction::Close(id))) if id == overlay
        ));
        assert!(matches!(
            actions.next().await,
            Some(Action::Window(WindowAction::Resize(id, _))) if id == main
        ));
        assert!(matches!(
            actions.next().await,
            Some(Action::Window(WindowAction::SetMode(
                id,
                window::Mode::Windowed
            ))) if id == main
        ));
        assert!(matches!(
            actions.next().await,
            Some(Action::Window(WindowAction::GainFocus(id))) if id == main
        ));
        assert!(actions.next().await.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_fallback_closes_donor_without_restoring_main_window() {
        let mut state = State::default();
        let main = window::Id::unique();
        let overlay = window::Id::unique();
        let request = PlaybackRequestId::new(1);
        state.windows.set(WindowKind::Main, main);
        state.windows.set(WindowKind::PlayerOverlay, overlay);
        assert!(state.windows.begin_player_overlay_launch(request));

        let result =
            dismiss_player_overlay_for_native_fallback(&mut state, request);
        assert_eq!(
            state.windows.player_overlay_state(),
            PlayerOverlayWindowState::Closing
        );

        let mut actions = task::into_stream(result.task)
            .expect("native fallback should close the hidden donor");
        assert!(matches!(
            actions.next().await,
            Some(Action::Window(WindowAction::Close(id))) if id == overlay
        ));
        assert!(
            actions.next().await.is_none(),
            "native fallback must not restore or focus the retained main window"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn playback_exit_dismisses_active_overlay_without_mutating_player() {
        let mut state = State::default();
        let main = window::Id::unique();
        let overlay = window::Id::unique();
        let request = PlaybackRequestId::new(1);
        state.windows.set(WindowKind::Main, main);
        state.windows.set(WindowKind::PlayerOverlay, overlay);
        assert!(state.windows.begin_player_overlay_launch(request));
        assert!(state.windows.activate_player_overlay(request));
        assert!(state.windows.finish_player_overlay_activation(request));
        state.domains.player.state.last_valid_position = 42.5;

        let result = dismiss_player_overlay(&mut state, Some(request));

        assert_eq!(
            state.windows.player_overlay_state(),
            PlayerOverlayWindowState::Closing
        );
        assert_eq!(state.domains.player.state.last_valid_position, 42.5);
        assert_eq!(state.windows.get(WindowKind::Main), Some(main));
        assert_eq!(state.windows.get(WindowKind::PlayerOverlay), Some(overlay));
        drop(result);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_fallback_exit_restores_shell_after_donor_is_gone() {
        let mut state = State::default();
        let main = window::Id::unique();
        let overlay = window::Id::unique();
        let request = PlaybackRequestId::new(7);
        state.windows.set(WindowKind::Main, main);
        state.windows.set(WindowKind::PlayerOverlay, overlay);
        assert!(state.windows.begin_player_overlay_launch(request));

        let close =
            dismiss_player_overlay_for_native_fallback(&mut state, request);
        drop(close);
        assert_eq!(
            state.windows.remove_by_id(overlay),
            Some(WindowKind::PlayerOverlay)
        );

        let result = dismiss_player_overlay(&mut state, Some(request));
        let mut actions = task::into_stream(result.task)
            .expect("fallback exit must restore the retained shell");
        assert!(matches!(
            actions.next().await,
            Some(Action::Window(WindowAction::Resize(id, _))) if id == main
        ));
        assert!(matches!(
            actions.next().await,
            Some(Action::Window(WindowAction::SetMode(
                id,
                window::Mode::Windowed
            ))) if id == main
        ));
        assert!(matches!(
            actions.next().await,
            Some(Action::Window(WindowAction::GainFocus(id))) if id == main
        ));
        assert!(actions.next().await.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn deferred_replacement_failure_restores_before_raw_close() {
        let mut state = State::default();
        let main = window::Id::unique();
        let overlay = window::Id::unique();
        let old = PlaybackRequestId::new(3);
        let replacement = PlaybackRequestId::new(4);
        state.windows.set(WindowKind::Main, main);
        state.windows.set(WindowKind::PlayerOverlay, overlay);
        assert!(state.windows.begin_player_overlay_launch(old));
        drop(dismiss_player_overlay_for_native_fallback(&mut state, old));

        state.domains.player.state.active_playback_request = Some(replacement);
        state.domains.player.state.resolved_playback_request =
            Some(replacement);
        let deferred = begin_integrated_playback(&mut state, replacement);
        assert!(task::into_stream(deferred.task).is_none());
        assert_eq!(
            state.windows.shell_hidden_for_playback(),
            Some(replacement)
        );

        let result = dismiss_player_overlay(&mut state, Some(replacement));
        let mut actions = task::into_stream(result.task)
            .expect("deferred replacement failure must restore the shell");
        assert!(matches!(
            actions.next().await,
            Some(Action::Window(WindowAction::Resize(id, _))) if id == main
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn requestless_stale_exit_cannot_restore_an_owned_shell() {
        let mut state = State::default();
        let main = window::Id::unique();
        let overlay = window::Id::unique();
        let request = PlaybackRequestId::new(5);
        state.windows.set(WindowKind::Main, main);
        state.windows.set(WindowKind::PlayerOverlay, overlay);
        assert!(state.windows.begin_player_overlay_launch(request));

        let result = dismiss_player_overlay(&mut state, None);
        assert!(task::into_stream(result.task).is_none());
        assert_eq!(state.windows.shell_hidden_for_playback(), Some(request));
        assert_eq!(
            state.windows.player_overlay_state(),
            PlayerOverlayWindowState::Launching
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn user_close_never_restores_shell_before_stop_is_reduced() {
        let mut state = State::default();
        let main = window::Id::unique();
        let overlay = window::Id::unique();
        let request = PlaybackRequestId::new(6);
        state.windows.set(WindowKind::Main, main);
        state.windows.set(WindowKind::PlayerOverlay, overlay);
        assert!(state.windows.begin_player_overlay_launch(request));
        assert!(state.windows.activate_player_overlay(request));
        assert!(state.windows.finish_player_overlay_activation(request));

        let result = close_player_overlay(&mut state, request);
        let mut actions = task::into_stream(result.task)
            .expect("user close must close the donor and request stop");
        let mut saw_close = false;
        let mut saw_stop = false;
        while let Some(action) = actions.next().await {
            match action {
                Action::Window(WindowAction::Close(id)) if id == overlay => {
                    saw_close = true;
                }
                Action::Output(DomainMessage::Player(PlayerMessage::Stop)) => {
                    saw_stop = true;
                }
                Action::Window(
                    WindowAction::Resize(id, _)
                    | WindowAction::SetMode(id, _)
                    | WindowAction::GainFocus(id),
                ) if id == main => {
                    panic!(
                        "main shell restoration must wait for PlaybackExited"
                    );
                }
                _ => {}
            }
        }
        assert!(saw_close);
        assert!(saw_stop);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn queued_native_root_close_stops_after_fallback_donor_raw_close() {
        let mut state = State::default();
        let overlay = window::Id::unique();
        let request = PlaybackRequestId::new(7);
        state.windows.set(WindowKind::PlayerOverlay, overlay);
        assert!(state.windows.begin_player_overlay_launch(request));
        assert!(state.windows.activate_player_overlay(request));
        assert!(state.windows.finish_player_overlay_activation(request));

        let fallback =
            dismiss_player_overlay_for_native_fallback(&mut state, request);
        assert_eq!(
            state.windows.player_overlay_state(),
            PlayerOverlayWindowState::Closing
        );
        let mut fallback_actions = task::into_stream(fallback.task)
            .expect("fallback must queue donor close");
        assert!(matches!(
            fallback_actions.next().await,
            Some(Action::Window(WindowAction::Close(id))) if id == overlay
        ));
        assert!(fallback_actions.next().await.is_none());

        let donor_closed = on_raw_window_closed(&mut state, overlay);
        assert!(task::into_stream(donor_closed.task).is_none());
        assert_eq!(state.windows.get(WindowKind::PlayerOverlay), None);

        let user_close = close_player_overlay(&mut state, request);
        let mut user_close_actions = task::into_stream(user_close.task)
            .expect("queued native root close must preserve stop intent");
        assert!(matches!(
            user_close_actions.next().await,
            Some(Action::Output(DomainMessage::Player(PlayerMessage::Stop)))
        ));
        assert!(user_close_actions.next().await.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stale_native_root_close_cannot_stop_replacement_request() {
        let mut state = State::default();
        let overlay = window::Id::unique();
        let stale = PlaybackRequestId::new(7);
        let replacement = PlaybackRequestId::new(8);
        state.windows.set(WindowKind::PlayerOverlay, overlay);
        assert!(state.windows.begin_player_overlay_launch(replacement));
        assert!(state.windows.activate_player_overlay(replacement));
        assert!(state.windows.finish_player_overlay_activation(replacement));

        let result = close_player_overlay(&mut state, stale);

        assert!(task::into_stream(result.task).is_none());
        assert_eq!(
            state.windows.player_overlay_state(),
            PlayerOverlayWindowState::Active
        );
        assert_eq!(
            state.windows.shell_hidden_for_playback(),
            Some(replacement)
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn delayed_handoff_ready_cannot_reveal_or_restore_during_shutdown() {
        let mut state = State::default();
        let main = window::Id::unique();
        let overlay = window::Id::unique();
        let request = PlaybackRequestId::new(9);
        state.windows.set(WindowKind::Main, main);
        state.windows.set(WindowKind::PlayerOverlay, overlay);
        assert!(state.windows.begin_player_overlay_launch(request));
        assert!(state.windows.activate_player_overlay(request));
        state.domains.player.state.root_shutdown_in_progress = true;

        let result = finish_player_overlay_activation(&mut state, request);

        assert!(task::into_stream(result.task).is_none());
        assert_eq!(
            state.windows.player_overlay_state(),
            PlayerOverlayWindowState::Activating
        );
        assert_eq!(state.windows.shell_hidden_for_playback(), Some(request));
        assert_eq!(
            state.windows.player_overlay_launch_request(),
            Some(request)
        );
    }
}
