use async_trait::async_trait;
use uuid::Uuid;
use anyhow::Result;

use crate::auth::domain::aggregates::{UserAuthentication, DeviceSession};
use crate::auth::domain::value_objects::DeviceFingerprint;

#[async_trait]
pub trait UserAuthenticationRepository: Send + Sync {
    async fn find_by_id(&self, user_id: Uuid) -> Result<Option<UserAuthentication>>;
    async fn find_by_username(&self, username: &str) -> Result<Option<UserAuthentication>>;
    async fn save(&self, user_auth: &UserAuthentication) -> Result<()>;
}

#[async_trait]
pub trait DeviceSessionRepository: Send + Sync {
    async fn find_by_id(&self, session_id: Uuid) -> Result<Option<DeviceSession>>;
    async fn find_by_user_and_fingerprint(&self, user_id: Uuid, fingerprint: &DeviceFingerprint) -> Result<Option<DeviceSession>>;
    async fn find_by_user_id(&self, user_id: Uuid) -> Result<Vec<DeviceSession>>;
    async fn save(&self, session: &DeviceSession) -> Result<()>;
}