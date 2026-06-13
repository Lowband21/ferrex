use crate::{constants::seeking::*, messages::PlayerMessage};
use iced::Subscription;
use iced::event;
use iced::keyboard::{self, Key, Modifiers, key::Named};

/// State snapshot needed to compose playback subscriptions.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlaybackSubscriptionState {
    pub is_player_view: bool,
    pub has_video: bool,
    pub controls_visible: bool,
    pub external_mpv_active: bool,
    pub is_playing: bool,
    pub tenfoot_mode: bool,
    pub search_open: bool,
}

/// Creates all core playback subscriptions (keyboard + overlay timers).
pub fn subscription(
    state: PlaybackSubscriptionState,
) -> Subscription<PlayerMessage> {
    let mut subs = vec![];

    // Only run the controls visibility timer when overlay is visible and a video is present
    if state.is_player_view && state.has_video && state.controls_visible {
        subs.push(
            iced::time::every(std::time::Duration::from_millis(500))
                .map(|_| PlayerMessage::CheckControlsVisibility),
        );
    }

    // If using external player, poll for position updates every second
    if state.external_mpv_active {
        subs.push(
            iced::time::every(std::time::Duration::from_secs(1))
                .map(|_| PlayerMessage::PollExternalMpv),
        );
    }

    // While playing internally, send a periodic heartbeat to persist progress
    if state.is_player_view && state.has_video && state.is_playing {
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

fn keyboard_shortcuts(
    state: PlaybackSubscriptionState,
) -> Subscription<PlayerMessage> {
    if state.tenfoot_mode || state.search_open {
        return Subscription::none();
    }

    let has_internal_video = state.has_video && !state.external_mpv_active;

    if !(state.is_player_view && has_internal_video) {
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
    let msg = match key {
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
    };
    msg
}
