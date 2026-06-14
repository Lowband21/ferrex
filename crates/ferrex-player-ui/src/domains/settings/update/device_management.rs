//! Desktop adapter for settings device-management effects.

use crate::{
    common::messages::{DomainMessage, DomainUpdateResult},
    domains::settings::{
        messages::SettingsMessage, sections::devices::state::UserDevice,
    },
    state::State,
};
use ferrex_player_settings::update::device_management as reducer;
use iced::Task;
use uuid::Uuid;

use super::apply_settings_update;

pub fn handle_load_devices(state: &mut State) -> DomainUpdateResult {
    let update = reducer::load_devices(
        &mut state.domains.settings.device_management_state,
    );
    apply_settings_update(state, update)
}

pub fn handle_devices_loaded(
    state: &mut State,
    result: Result<Vec<UserDevice>, String>,
) -> DomainUpdateResult {
    let update = reducer::devices_loaded(
        &mut state.domains.settings.device_management_state,
        result,
    );
    apply_settings_update(state, update)
}

pub fn handle_refresh_devices(state: &mut State) -> DomainUpdateResult {
    let update = reducer::refresh_devices(
        &mut state.domains.settings.device_management_state,
    );
    apply_settings_update(state, update)
}

pub fn handle_revoke_device(
    state: &mut State,
    device_id: String,
) -> DomainUpdateResult {
    let update = reducer::revoke_device(
        &mut state.domains.settings.device_management_state,
        device_id,
    );
    apply_settings_update(state, update)
}

pub fn handle_device_revoked(
    state: &mut State,
    result: Result<String, String>,
) -> DomainUpdateResult {
    let update = reducer::device_revoked(
        &mut state.domains.settings.device_management_state,
        result,
    );
    apply_settings_update(state, update)
}

pub(crate) fn load_devices_task(state: &State) -> Task<DomainMessage> {
    let settings_service = state.domains.settings.settings_service.clone();
    let auth_service = state.domains.settings.auth_service.clone();

    Task::perform(
        async move {
            let current_device_session_id = auth_service
                .current_device_session_id()
                .await
                .map_err(|error| error.to_string())?;

            settings_service
                .list_user_devices()
                .await
                .map(|devices| {
                    reducer::user_devices_from_authenticated(
                        devices,
                        current_device_session_id,
                    )
                })
                .map_err(|error| format!("Failed to load devices: {error}"))
        },
        |result| {
            DomainMessage::Settings(SettingsMessage::DevicesLoaded(result))
        },
    )
}

pub(crate) fn revoke_device_task(
    state: &State,
    device_id: Uuid,
    original: String,
) -> Task<DomainMessage> {
    let settings_service = state.domains.settings.settings_service.clone();
    Task::perform(
        async move {
            settings_service
                .revoke_device(device_id)
                .await
                .map(|()| original)
                .map_err(|error| format!("Failed to revoke device: {error}"))
        },
        |result| {
            DomainMessage::Settings(SettingsMessage::DeviceRevoked(result))
        },
    )
}
