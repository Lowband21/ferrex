use crate::{
    common::messages::DomainMessage,
    domains::{player::messages::PlayerMessage, ui::types::ViewState},
    state::State,
};
use iced::Subscription;

/// Creates all media/player-related subscriptions
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    let mut subscriptions = vec![];

    // Poll external MPV cross‑platform when active
    if state.domains.player.state.external_mpv_active {
        subscriptions.push(
            iced::time::every(std::time::Duration::from_secs(1)).map(|_| {
                DomainMessage::Player(
                    crate::domains::player::messages::PlayerMessage::PollExternalMpv,
                )
            }),
        );

        return Subscription::batch(subscriptions);
    }

    // Only subscribe to player events when we're in player view with a video
    if matches!(&state.domains.ui.state.view, ViewState::Player)
        && state.domains.player.state.video_opt.is_some()
    {
        // Only run the controls visibility timer when the overlay is visible
        if state.domains.player.state.controls {
            subscriptions.push(
                iced::time::every(std::time::Duration::from_millis(250)).map(
                    |_| {
                        DomainMessage::Player(
                            PlayerMessage::CheckControlsVisibility,
                        )
                    },
                ),
            );
        }

        let is_playing = state.domains.player.state.is_playing();

        // let overlay_active = {
        //     let ps = &state.domains.player.state;
        //     ps.controls
        //         || ps.show_settings
        //         || ps.show_subtitle_menu
        //         || ps.show_quality_menu
        //         || ps.track_notification.is_some()
        // };

        if is_playing {
            subscriptions.push(
                iced::time::every(std::time::Duration::from_secs(10)).map(
                    |_| {
                        DomainMessage::Player(
                            crate::domains::player::messages::PlayerMessage::NewFrame,
                        )
                    },
                ),
            );
        }
    }

    Subscription::batch(subscriptions)
}
