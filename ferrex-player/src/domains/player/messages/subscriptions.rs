use crate::common::messages::DomainMessage;
use crate::domains::player::messages::Message;
use crate::infra::constants::player::seeking::*;
use crate::state::State;
use iced::Subscription;

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
                |_| DomainMessage::Player(Message::CheckControlsVisibility),
            ),
        );
    }

    // If using external player, poll for position updates every second
    if state.domains.player.state.external_mpv_active {
        subs.push(
            iced::time::every(std::time::Duration::from_secs(1))
                .map(|_| DomainMessage::Player(Message::PollExternalMpv)),
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
            iced::time::every(std::time::Duration::from_secs(10))
                .map(|_| DomainMessage::Player(Message::ProgressHeartbeat)),
        );
    }

    // Player specific keyboard control
    subs.push(keyboard_shortcuts(state));

    Subscription::batch(subs)
}

fn keyboard_shortcuts(state: &State) -> Subscription<DomainMessage> {
    if state.search_window_id.is_some() {
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

    iced::keyboard::on_key_press(|key, modifiers| {
        use iced::keyboard::{key::Named, Key};
        let msg = match key {
            Key::Named(Named::Space) => Some(Message::PlayPause),
            Key::Named(Named::ArrowLeft) => {
                if modifiers.shift() {
                    Some(Message::SeekRelative(SEEK_BACKWARD_FINE))
                } else {
                    Some(Message::SeekRelative(SEEK_BACKWARD_COURSE))
                }
            }
            Key::Named(Named::ArrowRight) => {
                if modifiers.shift() {
                    Some(Message::SeekRelative(SEEK_FORWARD_FINE))
                } else {
                    Some(Message::SeekRelative(SEEK_FORWARD_COURSE))
                }
            }
            Key::Named(Named::ArrowUp) => Some(Message::SetVolume(1.1)),
            Key::Named(Named::ArrowDown) => Some(Message::SetVolume(0.9)),
            Key::Named(Named::Escape) => None,
            Key::Character(c) if c.as_str() == "f" || c.as_str() == "F" => {
                Some(Message::ToggleFullscreen)
            }
            Key::Named(Named::F11) => Some(Message::ToggleFullscreen),
            Key::Character(c) if c.as_str() == "m" || c.as_str() == "M" => {
                Some(Message::ToggleMute)
            }
            Key::Character(c) if c.as_str() == "s" || c.as_str() == "S" => {
                if modifiers.shift() {
                    Some(Message::ToggleSubtitleMenu)
                } else {
                    Some(Message::CycleSubtitleSimple)
                }
            }
            Key::Character(c) if c.as_str() == "a" || c.as_str() == "A" => {
                Some(Message::CycleAudioTrack)
            }
            _ => None,
        };
        msg.map(DomainMessage::Player)
    })
}
