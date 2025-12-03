use crate::state::State;

/// Extend the UI keep-alive window used to keep animations/rendering active
/// after user-driven scrolls or carousel motions. This prevents visible stalls
/// while atlas uploads complete. Duration is controlled by RuntimeConfig.
pub fn bump_keep_alive(state: &mut State) {
    use std::time::{Duration, Instant};
    let keep_alive_ms = state.runtime_config.keep_alive_ms();
    let until = Instant::now() + Duration::from_millis(keep_alive_ms);
    let ui_until = &mut state.domains.ui.state.poster_anim_active_until;
    *ui_until = Some(ui_until.map(|u| u.max(until)).unwrap_or(until));
}
