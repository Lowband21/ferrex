#[derive(Debug, Clone, Copy)]
pub struct MediaSubscriptionInputs {
    pub player_visible: bool,
    pub playback_active: bool,
}

/// Return whether media-domain runtime subscriptions should be active.
///
/// The desktop UI owns the concrete Iced timers; this crate only exposes the
/// dependency-light decision so the media domain does not pull in UI runtime
/// dependencies.
pub fn subscriptions_active(inputs: MediaSubscriptionInputs) -> bool {
    inputs.player_visible && inputs.playback_active
}
