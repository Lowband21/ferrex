use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

use crate::infrastructure::api_client::ApiClient;

#[async_trait]
pub trait SettingsService: Send + Sync {
    async fn list_user_devices(&self) -> Result<Vec<ferrex_core::auth::device::AuthenticatedDevice>>;
    async fn revoke_device(&self, device_id: Uuid) -> Result<()>;
}

#[derive(Clone)]
pub struct SettingsApiAdapter {
    client: Arc<ApiClient>,
}

impl SettingsApiAdapter {
    pub fn new(client: Arc<ApiClient>) -> Self { Self { client } }
}

#[async_trait]
impl SettingsService for SettingsApiAdapter {
    async fn list_user_devices(&self) -> Result<Vec<ferrex_core::auth::device::AuthenticatedDevice>> {
        self.client.list_user_devices().await
    }

    async fn revoke_device(&self, device_id: Uuid) -> Result<()> {
        self.client.revoke_device(device_id).await
    }
}
