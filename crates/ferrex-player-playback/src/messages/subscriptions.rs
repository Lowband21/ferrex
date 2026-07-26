use crate::{
    constants::seeking::*,
    contract::{BackendKind, PlaybackEventSignal, PlaybackTarget},
    messages::PlayerMessage,
};
use futures::{StreamExt, stream::BoxStream};
use iced::Subscription;
use iced::event;
use iced::keyboard::{self, Key, Modifiers, key::Named};

/// State snapshot needed to compose playback subscriptions.
#[derive(Debug, Clone, Default)]
pub struct PlaybackSubscriptionState {
    pub is_player_view: bool,
    /// An in-process session owns an Iced/native presentation surface.
    pub has_internal_video: bool,
    /// Any backend snapshot currently represents an active lifecycle.
    pub has_active_playback: bool,
    pub playback_target: Option<PlaybackTarget>,
    /// Integrated presentation or a pending native-window fallback proof
    /// still needs UI-thread AppKit/Win32 refresh turns.
    pub native_presenter_refresh_required: bool,
    pub controls_visible: bool,
    pub event_signal: Option<PlaybackEventSignal>,
    pub is_playing: bool,
    pub tenfoot_mode: bool,
    pub search_open: bool,
}

/// Creates all core playback subscriptions (keyboard + overlay timers).
pub fn subscription(
    state: PlaybackSubscriptionState,
) -> Subscription<PlayerMessage> {
    let mut subs = vec![];

    // Only run the controls visibility timer when the Iced overlay is visible
    // over an in-process presentation surface.
    if state.is_player_view
        && state.has_internal_video
        && state.controls_visible
    {
        subs.push(
            iced::time::every(std::time::Duration::from_millis(500))
                .map(|_| PlayerMessage::CheckControlsVisibility),
        );
    }

    // The retained process adapter has no push wakeup, so poll its private IPC
    // while its backend-neutral snapshot remains active.
    if state.has_active_playback
        && state
            .playback_target
            .is_some_and(|target| target.backend == BackendKind::ExternalMpv)
    {
        subs.push(
            iced::time::every(std::time::Duration::from_secs(1))
                .map(|_| PlayerMessage::PollExternalMpv),
        );
    }

    // Native control planes wake Iced only after copied events are queued. The
    // message then drains all pending events; no video-frame or timer-driven
    // redraw loop is needed.
    if state.has_internal_video
        && let Some(event_signal) = state.event_signal.clone()
    {
        subs.push(Subscription::run_with(event_signal, playback_event_stream));
    }

    // A native-root presenter follows an mpv-owned OS window. Moving,
    // minimizing, changing Spaces, or crossing a DPI boundary need not change
    // Iced's slot revision or emit an mpv property event, so refresh the
    // platform relationship while the integrated session is active.
    if state.has_internal_video
        && state.has_active_playback
        && state.native_presenter_refresh_required
    {
        subs.push(
            iced::time::every(std::time::Duration::from_millis(16))
                .map(|_| PlayerMessage::NativePresenterRefresh),
        );
    }

    // Progress persistence consumes the same snapshot for in-process and
    // external backends.
    if state.is_player_view && state.has_active_playback && state.is_playing {
        subs.push(
            iced::time::every(std::time::Duration::from_secs(10))
                .map(|_| PlayerMessage::ProgressHeartbeat),
        );
    }

    // Player specific keyboard control. 10-foot mode owns its own
    // spatial-navigation handler so desktop seek/volume shortcuts do not
    // compete with overlay focus movement.
    if !state.tenfoot_mode {
        subs.push(keyboard_shortcuts(state));
    }

    Subscription::batch(subs)
}

fn playback_event_stream(
    signal: &PlaybackEventSignal,
) -> BoxStream<'static, PlayerMessage> {
    let signal = signal.clone();
    futures::stream::unfold(signal, |signal| async move {
        let waiter = signal.clone();
        let notified =
            tokio::task::spawn_blocking(move || waiter.wait_blocking())
                .await
                .unwrap_or(false);
        notified.then_some((PlayerMessage::PlaybackEventsReady, signal))
    })
    .boxed()
}

fn keyboard_shortcuts(
    state: PlaybackSubscriptionState,
) -> Subscription<PlayerMessage> {
    if state.tenfoot_mode || state.search_open {
        return Subscription::none();
    }

    let accepts_iced_input = state.has_internal_video
        && state
            .playback_target
            .is_none_or(|target| target.backend != BackendKind::ExternalMpv);

    if !(state.is_player_view && accepts_iced_input) {
        return Subscription::none();
    }

    event::listen_with(|event, _status, _id| {
        let iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            ..
        }) = event
        else {
            return None;
        };
        handle_player_key_press(key, modifiers)
    })
}

fn handle_player_key_press(
    key: Key,
    modifiers: Modifiers,
) -> Option<PlayerMessage> {
    match key {
        Key::Named(Named::Space) => Some(PlayerMessage::PlayPause),
        Key::Named(Named::ArrowLeft) => {
            if modifiers.shift() {
                Some(PlayerMessage::SeekRelative(SEEK_BACKWARD_FINE))
            } else {
                Some(PlayerMessage::SeekRelative(SEEK_BACKWARD_COURSE))
            }
        }
        Key::Named(Named::ArrowRight) => {
            if modifiers.shift() {
                Some(PlayerMessage::SeekRelative(SEEK_FORWARD_FINE))
            } else {
                Some(PlayerMessage::SeekRelative(SEEK_FORWARD_COURSE))
            }
        }
        Key::Named(Named::ArrowUp) => Some(PlayerMessage::SetVolume(1.1)),
        Key::Named(Named::ArrowDown) => Some(PlayerMessage::SetVolume(0.9)),
        Key::Named(Named::Escape) => None,
        Key::Character(c) if c.as_str() == "f" || c.as_str() == "F" => {
            Some(PlayerMessage::ToggleFullscreen)
        }
        Key::Named(Named::F11) => Some(PlayerMessage::ToggleFullscreen),
        Key::Character(c) if c.as_str() == "m" || c.as_str() == "M" => {
            Some(PlayerMessage::ToggleMute)
        }
        Key::Character(c) if c.as_str() == "s" || c.as_str() == "S" => {
            if modifiers.shift() {
                Some(PlayerMessage::ToggleSubtitleMenu)
            } else {
                Some(PlayerMessage::CycleSubtitleSimple)
            }
        }
        Key::Character(c) if c.as_str() == "a" || c.as_str() == "A" => {
            Some(PlayerMessage::CycleAudioTrack)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::mpsc, time::Duration};

    use super::*;
    use crate::contract::SessionGeneration;

    #[tokio::test]
    async fn native_event_stream_wakes_once_and_ends_on_disconnect() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let signal =
            PlaybackEventSignal::new(SessionGeneration::INITIAL, receiver);
        let mut stream = playback_event_stream(&signal);

        sender.try_send(()).unwrap();
        let message =
            tokio::time::timeout(Duration::from_secs(1), stream.next())
                .await
                .expect("event signal wakes subscription")
                .expect("signal stream remains open");
        assert!(matches!(message, PlayerMessage::PlaybackEventsReady));

        drop(sender);
        assert!(
            tokio::time::timeout(Duration::from_secs(1), stream.next())
                .await
                .expect("disconnect wakes waiter")
                .is_none()
        );
    }
}
