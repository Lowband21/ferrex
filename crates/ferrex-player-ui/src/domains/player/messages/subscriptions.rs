use crate::{
    common::messages::DomainMessage, domains::ui::types::ViewState,
    state::State,
};
use ferrex_player_playback::messages::subscriptions::{
    PlaybackSubscriptionState, subscription as playback_subscription,
};
use iced::Subscription;

/// Creates all player-related subscriptions (keyboard + overlay timers).
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    let player = &state.domains.player.state;
    let playback = player.playback_snapshot();
    let snapshot = PlaybackSubscriptionState {
        is_player_view: matches!(
            &state.domains.ui.state.view,
            ViewState::Player
        ),
        has_internal_video: player.video_opt.is_some(),
        has_active_playback: playback
            .is_some_and(|snapshot| snapshot.has_active_session()),
        playback_target: playback.map(|snapshot| snapshot.target),
        native_presenter_refresh_required: player
            .video_opt
            .as_ref()
            .is_some_and(|session| session.native_presenter_refresh_required()),
        controls_visible: player.controls,
        event_signal: player
            .video_opt
            .as_ref()
            .and_then(|session| session.event_signal()),
        is_playing: playback.is_some_and(|snapshot| snapshot.is_playing()),
        tenfoot_mode: state.interface_mode.is_tenfoot(),
        search_open: state.domains.search.state.presentation.is_open(),
    };

    let mut subscriptions =
        vec![playback_subscription(snapshot).map(DomainMessage::Player)];

    if state.interface_mode.is_tenfoot() {
        subscriptions.push(
            crate::domains::ui::views::tenfoot::player_overlay::keyboard_subscription(
                state,
            ),
        );
        subscriptions.push(
            crate::domains::ui::views::tenfoot::player_overlay::controller_subscription(
                state,
            ),
        );
    }

    Subscription::batch(subscriptions)
}
