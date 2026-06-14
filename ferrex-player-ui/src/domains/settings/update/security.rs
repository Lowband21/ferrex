//! Desktop adapter for settings security reducers.

use crate::{
    common::messages::{DomainMessage, DomainUpdateResult},
    domains::settings::messages::SettingsMessage,
    state::State,
};
use ferrex_player_settings::update::security as reducer;
use iced::Task;

use super::apply_settings_update;

pub fn handle_show_change_password(state: &mut State) -> DomainUpdateResult {
    let update =
        reducer::show_change_password(&mut state.domains.settings.security);
    apply_settings_update(state, update)
}

pub fn handle_update_password_current(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    let update = reducer::update_password_current(
        &mut state.domains.settings.security,
        value,
    );
    apply_settings_update(state, update)
}

pub fn handle_update_password_new(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    let update = reducer::update_password_new(
        &mut state.domains.settings.security,
        value,
    );
    apply_settings_update(state, update)
}

pub fn handle_update_password_confirm(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    let update = reducer::update_password_confirm(
        &mut state.domains.settings.security,
        value,
    );
    apply_settings_update(state, update)
}

pub fn handle_toggle_password_visibility(
    state: &mut State,
) -> DomainUpdateResult {
    let update = reducer::toggle_password_visibility(
        &mut state.domains.settings.security,
    );
    apply_settings_update(state, update)
}

pub fn handle_submit_password_change(state: &mut State) -> DomainUpdateResult {
    let update =
        reducer::submit_password_change(&mut state.domains.settings.security);
    apply_settings_update(state, update)
}

pub fn handle_password_change_result(
    state: &mut State,
    result: Result<(), String>,
) -> DomainUpdateResult {
    let update = reducer::password_change_result(
        &mut state.domains.settings.security,
        result,
    );
    apply_settings_update(state, update)
}

pub fn handle_cancel_password_change(state: &mut State) -> DomainUpdateResult {
    let update =
        reducer::cancel_password_change(&mut state.domains.settings.security);
    apply_settings_update(state, update)
}

pub fn handle_check_user_has_pin(state: &mut State) -> DomainUpdateResult {
    let update = reducer::check_user_has_pin();
    apply_settings_update(state, update)
}

pub fn handle_user_has_pin_result(
    state: &mut State,
    has_pin: bool,
) -> DomainUpdateResult {
    let update = reducer::user_has_pin_result(
        &mut state.domains.settings.security,
        has_pin,
    );
    apply_settings_update(state, update)
}

pub fn handle_show_set_pin(state: &mut State) -> DomainUpdateResult {
    let update = reducer::show_set_pin(&mut state.domains.settings.security);
    apply_settings_update(state, update)
}

pub fn handle_show_change_pin(state: &mut State) -> DomainUpdateResult {
    let update = reducer::show_change_pin(&mut state.domains.settings.security);
    apply_settings_update(state, update)
}

pub fn handle_update_pin_current(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    let policy = (&state.domains.auth.state.pin_policy).into();
    let update = reducer::update_pin_current(
        &mut state.domains.settings.security,
        value,
        policy,
    );
    apply_settings_update(state, update)
}

pub fn handle_update_pin_new(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    let policy = (&state.domains.auth.state.pin_policy).into();
    let update = reducer::update_pin_new(
        &mut state.domains.settings.security,
        value,
        policy,
    );
    apply_settings_update(state, update)
}

pub fn handle_update_pin_confirm(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    let policy = (&state.domains.auth.state.pin_policy).into();
    let update = reducer::update_pin_confirm(
        &mut state.domains.settings.security,
        value,
        policy,
    );
    apply_settings_update(state, update)
}

pub fn handle_submit_pin_change(state: &mut State) -> DomainUpdateResult {
    let policy = (&state.domains.auth.state.pin_policy).into();
    let update = reducer::submit_pin_change(
        &mut state.domains.settings.security,
        policy,
    );
    apply_settings_update(state, update)
}

pub fn handle_submit_pin_removal(state: &mut State) -> DomainUpdateResult {
    let update =
        reducer::submit_pin_removal(&mut state.domains.settings.security);
    apply_settings_update(state, update)
}

pub fn handle_pin_change_result(
    state: &mut State,
    result: Result<(), String>,
) -> DomainUpdateResult {
    let update = reducer::pin_change_result(
        &mut state.domains.settings.security,
        result,
    );
    apply_settings_update(state, update)
}

pub fn handle_pin_removal_result(
    state: &mut State,
    result: Result<(), String>,
) -> DomainUpdateResult {
    let update = reducer::pin_removal_result(
        &mut state.domains.settings.security,
        result,
    );
    apply_settings_update(state, update)
}

pub fn handle_cancel_pin_change(state: &mut State) -> DomainUpdateResult {
    let update =
        reducer::cancel_pin_change(&mut state.domains.settings.security);
    apply_settings_update(state, update)
}

pub(crate) fn check_user_has_pin_task(state: &State) -> Task<DomainMessage> {
    let auth_service = state.domains.auth.state.auth_service.clone();
    Task::perform(
        async move {
            let maybe_user =
                auth_service.get_current_user().await.ok().flatten();
            if let Some(user) = maybe_user {
                auth_service
                    .check_device_auth(user.id)
                    .await
                    .map(|status| status.has_pin)
                    .unwrap_or(false)
            } else {
                false
            }
        },
        |has_pin| {
            DomainMessage::Settings(SettingsMessage::UserHasPinResult(has_pin))
        },
    )
}
