use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use crate::auth::domain::aggregates::{DeviceSession, UserAuthentication};
use crate::auth::domain::value_objects::{
    DeviceFingerprint, RefreshToken, RevocationReason, SessionScope,
};
use chrono::{DateTime, Utc};
use serde_json::Value;

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
    async fn exists_by_fingerprint(&self, fingerprint: &DeviceFingerprint) -> Result<bool>;
    async fn pin_status_by_fingerprint(
        &self,
        fingerprint: &DeviceFingerprint,
    ) -> Result<Vec<DevicePinStatus>>;
}

#[derive(Debug, Clone)]
pub struct DevicePinStatus {
    pub user_id: Uuid,
    pub has_pin: bool,
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

    async fn mark_used(&self, token_id: Uuid, reason: RevocationReason) -> Result<()>;

    async fn revoke_family(&self, family_id: Uuid, reason: RevocationReason) -> Result<()>;

    async fn revoke_for_user(&self, user_id: Uuid, reason: RevocationReason) -> Result<()>;

    async fn revoke_for_device(
        &self,
        device_session_id: Uuid,
        reason: RevocationReason,
    ) -> Result<()>;
}

#[async_trait]
pub trait AuthSessionRepository: Send + Sync {
    async fn insert_session(
        &self,
        user_id: Uuid,
        device_session_id: Option<Uuid>,
        scope: SessionScope,
        token_hash: &str,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Uuid>;

    async fn revoke_by_hash(&self, token_hash: &str, reason: RevocationReason) -> Result<()>;

    async fn find_by_hash(&self, token_hash: &str) -> Result<Option<AuthSessionRecord>>;

    async fn touch(&self, session_id: Uuid) -> Result<()>;

    async fn revoke_by_user(&self, user_id: Uuid, reason: RevocationReason) -> Result<()>;

    async fn revoke_by_device(
        &self,
        device_session_id: Uuid,
        reason: RevocationReason,
    ) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct AuthSessionRecord {
    pub id: Uuid,
    pub user_id: Uuid,
    pub device_session_id: Option<Uuid>,
    pub scope: SessionScope,
    pub expires_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub revoked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthAuditEventKind {
    PasswordLoginSuccess,
    PasswordLoginFailure,
    PinLoginSuccess,
    PinLoginFailure,
    DeviceRegistered,
    DeviceRevoked,
    PinSet,
    PinRemoved,
    SessionCreated,
    SessionRevoked,
    AutoLogin,
}

impl AuthAuditEventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PasswordLoginSuccess => "password_login_success",
            Self::PasswordLoginFailure => "password_login_failure",
            Self::PinLoginSuccess => "pin_login_success",
            Self::PinLoginFailure => "pin_login_failure",
            Self::DeviceRegistered => "device_registered",
            Self::DeviceRevoked => "device_revoked",
            Self::PinSet => "pin_set",
            Self::PinRemoved => "pin_removed",
            Self::SessionCreated => "session_created",
            Self::SessionRevoked => "session_revoked",
            Self::AutoLogin => "auto_login",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuthEventLog {
    pub event_type: AuthAuditEventKind,
    pub user_id: Option<Uuid>,
    pub device_session_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
    pub success: bool,
    pub failure_reason: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub metadata: Value,
    pub occurred_at: DateTime<Utc>,
}

impl AuthEventLog {
    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = metadata;
        self
    }
}

#[async_trait]
pub trait AuthEventRepository: Send + Sync {
    async fn record(&self, events: Vec<AuthEventLog>) -> Result<()>;
}
