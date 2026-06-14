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
    let snapshot = PlaybackSubscriptionState {
        is_player_view: matches!(
            &state.domains.ui.state.view,
            ViewState::Player
        ),
        has_video: player.video_opt.is_some(),
        controls_visible: player.controls,
        external_mpv_active: player.external_mpv_active,
        is_playing: player.is_playing(),
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
    }

    Subscription::batch(subscriptions)
}
