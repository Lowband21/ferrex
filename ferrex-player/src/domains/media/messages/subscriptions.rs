use super::MediaMessage;
use crate::common::messages::DomainMessage;
use crate::domains::ui::types::ViewState;
use crate::state::State;
use iced::Subscription;

/// Creates all media/player-related subscriptions
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    let mut subscriptions = vec![];

    // Poll external MPV cross‑platform when active (no feature gate)
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
                iced::time::every(std::time::Duration::from_millis(500))
                    .map(|_| DomainMessage::Player(crate::domains::player::messages::PlayerMessage::CheckControlsVisibility)),
            );
        }

        // For Wayland video, avoid per-frame wakeups unless overlay is visible.
        // When overlay is hidden, keep a light 10s heartbeat to refresh state minimally.
        let is_playing = state.domains.player.state.is_playing();
        // TODO: Reenable wayland optimization?
        let is_wayland = false;
        //let is_wayland = state
        //    .domains
        //    .player
        //    .state
        //    .video_opt
        //    .as_ref()
        //    .map(|v| v.is_wayland_video())
        //    .unwrap_or(false);
        let overlay_active = {
            let ps = &state.domains.player.state;
            ps.controls
                || ps.show_settings
                || ps.show_subtitle_menu
                || ps.show_quality_menu
                || ps.track_notification.is_some()
        };
        if is_wayland && is_playing && !overlay_active {
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

        // Causes panic during playback only in debug builds
        // Subscribe to watch progress updates
        //subscriptions.push(watch_progress_subscription(state));
    }

    Subscription::batch(subscriptions)
}

/// Creates a subscription for sending watch progress updates to the server
fn watch_progress_subscription(state: &State) -> Subscription<DomainMessage> {
    use std::time::Duration;

    // Only create subscription if we have an API service and media loaded
    if state.domains.media.state.api_service.is_none() {
        log::trace!("Watch progress subscription: No API service");
        return Subscription::none();
    }

    if state.domains.media.state.current_media_id.is_none() {
        log::trace!("Watch progress subscription: No current media ID");
        return Subscription::none();
    }

    let has_video = state.domains.player.state.video_opt.is_some();
    let is_playing = state.domains.player.state.is_playing();

    // Only send updates if we have a video playing
    if !has_video {
        log::trace!("Watch progress subscription: No video");
        return Subscription::none();
    }

    if !is_playing {
        log::trace!("Watch progress subscription: Not playing");
        return Subscription::none();
    }

    log::debug!(
        "Watch progress subscription: Active - will send update every 10s"
    );

    let player_state = &state.domains.player.state;

    if has_video && is_playing {
        match (
            player_state.current_media_id.clone(),
            player_state
                .video_opt
                .as_ref()
                .unwrap()
                .position()
                .as_secs_f64(),
            player_state
                .video_opt
                .as_ref()
                .unwrap()
                .duration()
                .as_secs_f64(),
        ) {
            (Some(media_id), position, duration) => {
                // Send progress update every 10 seconds while playing
                iced::time::every(Duration::from_secs(10)).map(move |_| {
                    log::debug!("Watch progress subscription: Triggering SendProgressUpdate");
                    DomainMessage::Media(MediaMessage::SendProgressUpdateWithData(
                        media_id, position, duration,
                    ))
                })
            }
            _ => Subscription::none(),
        }
    } else {
        Subscription::none()
    }
}
