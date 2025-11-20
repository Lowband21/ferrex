use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use crate::auth::domain::aggregates::{DeviceSession, UserAuthentication};
use crate::auth::domain::value_objects::{DeviceFingerprint, RefreshToken};
use chrono::{DateTime, Utc};

#[async_trait]
pub trait UserAuthenticationRepository: Send + Sync {
    async fn find_by_id(&self, user_id: Uuid) -> Result<Option<UserAuthentication>>;
    async fn find_by_username(&self, username: &str) -> Result<Option<UserAuthentication>>;
    async fn save(&self, user_auth: &UserAuthentication) -> Result<()>;
}

#[async_trait]
pub trait DeviceSessionRepository: Send + Sync {
    async fn find_by_id(&self, session_id: Uuid) -> Result<Option<DeviceSession>>;
    async fn find_by_user_and_fingerprint(
        &self,
        user_id: Uuid,
        fingerprint: &DeviceFingerprint,
    ) -> Result<Option<DeviceSession>>;
    async fn find_by_user_id(&self, user_id: Uuid) -> Result<Vec<DeviceSession>>;
    async fn save(&self, session: &DeviceSession) -> Result<Option<Uuid>>;
}

#[derive(Debug, Clone)]
pub struct RefreshTokenRecord {
    pub id: Uuid,
    pub user_id: Uuid,
    pub device_session_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
    pub token: RefreshToken,
    pub revoked: bool,
    pub revoked_reason: Option<String>,
    pub used_count: i32,
}

#[async_trait]
pub trait RefreshTokenRepository: Send + Sync {
    async fn insert_refresh_token(
        &self,
        token_hash: &str,
        user_id: Uuid,
        device_session_id: Option<Uuid>,
        session_id: Option<Uuid>,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        family_id: Uuid,
        generation: i32,
    ) -> Result<Uuid>;

    async fn get_active_refresh_token(
        &self,
        token_hash: &str,
    ) -> Result<Option<RefreshTokenRecord>>;

    async fn mark_used(&self, token_id: Uuid, reason: &str) -> Result<()>;

    async fn revoke_family(&self, family_id: Uuid, reason: &str) -> Result<()>;
}

#[async_trait]
pub trait AuthSessionRepository: Send + Sync {
    async fn insert_session(
        &self,
        user_id: Uuid,
        device_session_id: Option<Uuid>,
        token_hash: &str,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Uuid>;

    async fn revoke_by_hash(&self, token_hash: &str, reason: &str) -> Result<()>;
}
