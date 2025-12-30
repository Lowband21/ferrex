use crate::common::messages::DomainMessage;
use crate::domains::player::messages::PlayerMessage;
use crate::infra::constants::player::seeking::*;
use crate::state::State;
use iced::Subscription;
use iced::event;
use iced::keyboard::{self, Key, Modifiers, key::Named};

/// Creates all player-related subscriptions (keyboard + overlay timers)
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    let mut subs = vec![];

    // Only run the controls visibility timer when overlay is visible and a video is present
    if matches!(
        &state.domains.ui.state.view,
        crate::domains::ui::types::ViewState::Player
    ) && state.domains.player.state.video_opt.is_some()
        && state.domains.player.state.controls
    {
        subs.push(
            iced::time::every(std::time::Duration::from_millis(500)).map(
                |_| {
                    DomainMessage::Player(
                        PlayerMessage::CheckControlsVisibility,
                    )
                },
            ),
        );
    }

    // If using external player, poll for position updates every second
    if state.domains.player.state.external_mpv_active {
        subs.push(
            iced::time::every(std::time::Duration::from_secs(1))
                .map(|_| DomainMessage::Player(PlayerMessage::PollExternalMpv)),
        );
    }

    // While playing internally, send a periodic heartbeat to persist progress
    if matches!(
        &state.domains.ui.state.view,
        crate::domains::ui::types::ViewState::Player
    ) && state.domains.player.state.video_opt.is_some()
        && state.domains.player.state.is_playing()
    {
        subs.push(
            iced::time::every(std::time::Duration::from_secs(10)).map(|_| {
                DomainMessage::Player(PlayerMessage::ProgressHeartbeat)
            }),
        );
    }

    // Player specific keyboard control
    subs.push(keyboard_shortcuts(state));

    Subscription::batch(subs)
}

fn keyboard_shortcuts(state: &State) -> Subscription<DomainMessage> {
    if state.domains.search.state.presentation.is_open() {
        return Subscription::none();
    }

    let is_player_view = matches!(
        &state.domains.ui.state.view,
        crate::domains::ui::types::ViewState::Player
    );

    let has_internal_video = state.domains.player.state.video_opt.is_some()
        && !state.domains.player.state.external_mpv_active;

    if !(is_player_view && has_internal_video) {
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
) -> Option<DomainMessage> {
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
    msg.map(DomainMessage::Player)
}
