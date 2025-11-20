use std::sync::Arc;

use chrono::Utc;
use ferrex_core::auth::device::AuthenticatedDevice;
use ferrex_player::domains::settings::messages as settings_messages;
use ferrex_player::domains::ui::views::settings::device_management::UserDevice;
use ferrex_player::infrastructure::services::settings::SettingsService;
use ferrex_player::state_refactored::State;
use iced::Task;
use serde_json::json;
use uuid::Uuid;

struct MockSettingsServiceOk;
#[async_trait::async_trait]
impl SettingsService for MockSettingsServiceOk {
    async fn list_user_devices(&self) -> anyhow::Result<Vec<AuthenticatedDevice>> {
        Ok(vec![AuthenticatedDevice {
            id: Uuid::new_v4(),
            fingerprint: "fp-test".to_string(),
            name: "Test Device".to_string(),
            platform: ferrex_core::auth::Platform::Linux,
            app_version: Some("1.0.0".to_string()),
            first_authenticated_by: Uuid::new_v4(),
            first_authenticated_at: Utc::now(),
            trusted_until: Utc::now(),
            last_seen_at: Utc::now(),
            revoked: false,
            revoked_by: None,
            revoked_at: None,
            metadata: json!({}),
        }])
    }
    async fn revoke_device(&self, _device_id: Uuid) -> anyhow::Result<()> {
        Ok(())
    }
}

struct MockSettingsServiceErr;
#[async_trait::async_trait]
impl SettingsService for MockSettingsServiceErr {
    async fn list_user_devices(&self) -> anyhow::Result<Vec<AuthenticatedDevice>> {
        anyhow::bail!("boom")
    }
    async fn revoke_device(&self, _device_id: Uuid) -> anyhow::Result<()> {
        anyhow::bail!("boom")
    }
}

fn new_state_with_service(service: Option<Arc<dyn SettingsService>>) -> State {
    let mut state = State::default();
    // Ensure settings view is device management for clarity
    state.domains.settings.current_view =
        ferrex_player::domains::settings::state::SettingsView::DeviceManagement;
    if let Some(svc) = service {
        state.domains.settings.settings_service = svc;
    }
    state
}

#[test]
fn handle_load_devices_without_service_is_noop() {
    let mut state = new_state_with_service(None);
    assert!(
        state
            .domains
            .settings
            .device_management_state
            .devices
            .is_empty()
    );

    let result = ferrex_player::domains::settings::update::device_management::handle_load_devices(
        &mut state,
    );
    let _task = result.task; // Extract task from DomainUpdateResult

    // We cannot easily inspect the Task, but we can assert state toggles were set
    assert!(state.domains.settings.device_management_state.loading);
    assert!(
        state
            .domains
            .settings
            .device_management_state
            .error_message
            .is_none()
    );

    // Simulate completion with an error and ensure reducer updates state
    let result: Result<Vec<UserDevice>, String> = Err("No service".to_string());
    let _ = ferrex_player::domains::settings::update::device_management::handle_devices_loaded(
        &mut state, result,
    );
    assert!(!state.domains.settings.device_management_state.loading);
    assert!(
        state
            .domains
            .settings
            .device_management_state
            .error_message
            .is_some()
    );
}

#[test]
fn handle_devices_loaded_success_updates_state() {
    let mut state = new_state_with_service(None);
    state.domains.settings.device_management_state.loading = true;
    let devices = vec![UserDevice {
        device_id: "abc".into(),
        device_name: "Test".into(),
        device_type: "desktop".into(),
        last_active: chrono::Utc::now(),
        is_current_device: false,
        location: None,
    }];
    let _ = ferrex_player::domains::settings::update::device_management::handle_devices_loaded(
        &mut state,
        Ok(devices.clone()),
    );
    assert_eq!(
        state.domains.settings.device_management_state.devices.len(),
        1
    );
    assert!(
        state
            .domains
            .settings
            .device_management_state
            .error_message
            .is_none()
    );
}

#[test]
fn handle_revoke_device_invalid_id_is_noop() {
    let mut state = new_state_with_service(Some(Arc::new(MockSettingsServiceOk)));
    let _task = ferrex_player::domains::settings::update::device_management::handle_revoke_device(
        &mut state,
        "not-a-uuid".into(),
    );
    // Ensure no panic and no changes to devices list
    assert!(
        state
            .domains
            .settings
            .device_management_state
            .devices
            .is_empty()
    );
}
