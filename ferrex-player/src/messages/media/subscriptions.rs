use super::Message;
use crate::messages::DomainMessage;
use crate::state::{State, ViewState};
use iced::Subscription;

/// Creates all media/player-related subscriptions
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    let mut subscriptions = vec![];

    // Only subscribe to player events when we're in player view with a video
    if matches!(&state.view, ViewState::Player) && state.player.video_opt.is_some() {
        // Timer for checking controls visibility
        subscriptions.push(
            iced::time::every(std::time::Duration::from_millis(500))
                .map(|_| DomainMessage::Media(Message::CheckControlsVisibility)),
        );

        // Subscribe to keyboard shortcuts
        subscriptions.push(keyboard_shortcuts());
    }

    Subscription::batch(subscriptions)
}

/// Creates keyboard shortcut subscription for player controls
fn keyboard_shortcuts() -> Subscription<DomainMessage> {
    iced::keyboard::on_key_press(|key, modifiers| {
        use iced::keyboard::{key::Named, Key};

        let msg = match key {
            Key::Named(Named::Space) => Some(Message::PlayPause),
            Key::Named(Named::ArrowLeft) => {
                if modifiers.shift() {
                    Some(Message::SeekRelative(-30.0))
                } else {
                    Some(Message::SeekBackward)
                }
            }
            Key::Named(Named::ArrowRight) => {
                if modifiers.shift() {
                    Some(Message::SeekRelative(30.0))
                } else {
                    Some(Message::SeekForward)
                }
            }
            Key::Named(Named::ArrowUp) => Some(Message::SetVolume(1.1)),
            Key::Named(Named::ArrowDown) => Some(Message::SetVolume(0.9)),
            Key::Named(Named::Escape) => Some(Message::ExitFullscreen),
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

        msg.map(DomainMessage::Media)
    })
}
