use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use chrono::{Duration, Utc};
use ferrex_core::auth::device::AuthDeviceStatus;
use ferrex_core::player_prelude::{AuthenticatedDevice, Platform};
use serde_json::json;
use uuid::Uuid;

use crate::infrastructure::services::settings::SettingsService;

#[derive(Debug, Clone)]
pub struct TestSettingsService {
    devices: Arc<RwLock<Vec<AuthenticatedDevice>>>,
}

impl Default for TestSettingsService {
    fn default() -> Self {
        Self::with_default_device()
    }
}

impl TestSettingsService {
    pub fn new(devices: Vec<AuthenticatedDevice>) -> Self {
        Self {
            devices: Arc::new(RwLock::new(devices)),
        }
    }

    pub fn with_default_device() -> Self {
        let owner = Uuid::now_v7();
        let device = AuthenticatedDevice {
            id: Uuid::now_v7(),
            user_id: owner,
            fingerprint: "test-device".into(),
            name: "Ferrex Player".into(),
            platform: Platform::Linux,
            app_version: Some("tester".into()),
            hardware_id: None,
            status: AuthDeviceStatus::Trusted,
            pin_hash: None,
            pin_set_at: None,
            pin_last_used_at: None,
            failed_attempts: 0,
            locked_until: None,
            first_authenticated_by: owner,
            first_authenticated_at: Utc::now() - Duration::minutes(5),
            trusted_until: Some(Utc::now() + Duration::days(30)),
            last_seen_at: Utc::now(),
            last_activity: Utc::now(),
            auto_login_enabled: true,
            revoked_by: None,
            revoked_at: None,
            revoked_reason: None,
            created_at: Utc::now() - Duration::minutes(10),
            updated_at: Utc::now(),
            metadata: json!({"source": "test"}),
        };

        Self::new(vec![device])
    }

    pub fn add_device(&self, device: AuthenticatedDevice) {
        if let Ok(mut guard) = self.devices.write() {
            guard.push(device);
        }
    }

    pub fn devices(&self) -> Vec<AuthenticatedDevice> {
        self.devices.read().expect("lock poisoned").clone()
    }
}

#[async_trait]
impl SettingsService for TestSettingsService {
    async fn list_user_devices(&self) -> anyhow::Result<Vec<AuthenticatedDevice>> {
        Ok(self.devices.read().expect("lock poisoned").clone())
    }

    async fn revoke_device(&self, device_id: Uuid) -> anyhow::Result<()> {
        if let Ok(mut guard) = self.devices.write() {
            guard.retain(|device| device.id != device_id);
        }
        Ok(())
    }
}
