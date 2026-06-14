use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::users::auth::policy::{
    AuthSecuritySettings, DeviceTrustPolicy, PasswordPolicy, PinSecurityPolicy,
};
use crate::error::Result;

#[derive(Debug, Clone)]
pub struct SecuritySettingsUpdate {
    pub admin_password_policy: PasswordPolicy,
    pub user_password_policy: PasswordPolicy,
    pub pin_policy: PinSecurityPolicy,
    pub device_trust_policy: DeviceTrustPolicy,
    pub updated_by: Option<Uuid>,
}

#[async_trait]
pub trait SecuritySettingsRepository: Send + Sync {
    async fn get_settings(&self) -> Result<AuthSecuritySettings>;
    async fn update_settings(
        &self,
        update: SecuritySettingsUpdate,
    ) -> Result<AuthSecuritySettings>;
}
