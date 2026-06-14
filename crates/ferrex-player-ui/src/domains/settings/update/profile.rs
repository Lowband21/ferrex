//! Desktop adapter for settings profile reducers.

use crate::{common::messages::DomainUpdateResult, state::State};
use ferrex_player_settings::update::profile as reducer;

use super::apply_settings_update;

pub fn handle_update_display_name(
    state: &mut State,
    name: String,
) -> DomainUpdateResult {
    let update =
        reducer::update_display_name(&mut state.domains.settings.profile, name);
    apply_settings_update(state, update)
}

pub fn handle_update_email(
    state: &mut State,
    email: String,
) -> DomainUpdateResult {
    let update =
        reducer::update_email(&mut state.domains.settings.profile, email);
    apply_settings_update(state, update)
}

pub fn handle_submit_profile_changes(state: &mut State) -> DomainUpdateResult {
    let update =
        reducer::submit_profile_changes(&mut state.domains.settings.profile);
    apply_settings_update(state, update)
}

pub fn handle_profile_change_result(
    state: &mut State,
    result: Result<(), String>,
) -> DomainUpdateResult {
    let update = reducer::profile_change_result(
        &mut state.domains.settings.profile,
        result,
    );
    apply_settings_update(state, update)
}
