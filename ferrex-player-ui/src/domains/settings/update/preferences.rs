//! Desktop adapter for settings preference reducers.

use crate::{common::messages::DomainUpdateResult, state::State};
use ferrex_core::player_prelude::UserScale;
use ferrex_player_settings::{ScalePreset, update::preferences as reducer};

use super::{apply_settings_update, sync_auto_login_auth_state};

pub fn handle_toggle_auto_login(
    state: &mut State,
    enabled: bool,
) -> DomainUpdateResult {
    let update = reducer::toggle_auto_login(
        &mut state.domains.settings.preferences,
        enabled,
    );
    apply_settings_update(state, update)
}

pub fn handle_auto_login_toggled(
    state: &mut State,
    result: Result<bool, String>,
) -> DomainUpdateResult {
    sync_auto_login_auth_state(state, &result);
    let update = reducer::auto_login_toggled(
        &mut state.domains.settings.preferences,
        result,
    );
    apply_settings_update(state, update)
}

pub fn handle_set_user_scale(
    state: &mut State,
    user_scale: UserScale,
) -> DomainUpdateResult {
    let update = reducer::set_user_scale(
        &mut state.domains.settings.preferences,
        user_scale,
    );
    apply_settings_update(state, update)
}

pub fn handle_set_scale_preset(
    state: &mut State,
    preset: ScalePreset,
) -> DomainUpdateResult {
    let update = reducer::set_scale_preset(preset);
    apply_settings_update(state, update)
}
