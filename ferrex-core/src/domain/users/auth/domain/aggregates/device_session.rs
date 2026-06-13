use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::users::auth::domain::events::AuthEvent;
use crate::domain::users::auth::domain::value_objects::{
    DeviceFingerprint, SessionToken,
};

const DEFAULT_DEVICE_TRUST_DURATION_DAYS: i64 = 30;
const DEFAULT_PIN_LOCKOUT_MINUTES: i64 = 5;

fn default_device_trust_duration() -> Duration {
    Duration::days(DEFAULT_DEVICE_TRUST_DURATION_DAYS)
}

fn default_pin_lockout_duration() -> Duration {
    Duration::minutes(DEFAULT_PIN_LOCKOUT_MINUTES)
}

/// Errors that can occur with device sessions
#[derive(Debug, Error)]
pub enum DeviceSessionError {
    #[error("Device has been revoked")]
    DeviceRevoked,

    #[error("Session has expired")]
    SessionExpired,

    #[error("Invalid state transition")]
    InvalidStateTransition,

    #[error("Too many failed attempts")]
    TooManyFailedAttempts,

    #[error("PIN required but not set")]
    PinNotSet,

    #[error("Invalid PIN")]
    InvalidPin,

    #[error("Device not trusted")]
    DeviceNotTrusted,
}

/// Device trust status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceStatus {
    /// Device is pending trust (needs PIN verification)
    Pending,

    /// Device is trusted and can authenticate
    Trusted,

    /// Device has been revoked and cannot authenticate
    Revoked,
}

/// Client/device metadata persisted with a device trust record.
#[derive(Debug, Clone)]
pub struct DeviceSessionClientMetadata {
    pub platform: Option<String>,
    pub app_version: Option<String>,
    pub hardware_id: Option<String>,
    pub device_public_key: Option<String>,
    pub device_key_alg: Option<String>,
    pub trusted_until: Option<DateTime<Utc>>,
    pub auto_login_enabled: Option<bool>,
    pub metadata: Option<Value>,
}

impl Default for DeviceSessionClientMetadata {
    fn default() -> Self {
        Self {
            platform: None,
            app_version: None,
            hardware_id: None,
            device_public_key: None,
            device_key_alg: None,
            trusted_until: None,
            auto_login_enabled: None,
            metadata: None,
        }
    }
}

#[cfg(feature = "database")]
#[derive(Debug, Clone)]
pub(crate) struct DeviceSessionHydration {
    pub id: Uuid,
    pub user_id: Uuid,
    pub device_fingerprint: DeviceFingerprint,
    pub device_name: String,
    pub platform: Option<String>,
    pub app_version: Option<String>,
    pub hardware_id: Option<String>,
    pub device_public_key: Option<String>,
    pub device_key_alg: Option<String>,
    pub status: DeviceStatus,
    pub pin_configured: bool,
    pub session_token: Option<SessionToken>,
    pub failed_attempts: u8,
    pub locked_until: Option<DateTime<Utc>>,
    pub trusted_until: Option<DateTime<Utc>>,
    pub auto_login_enabled: bool,
    pub first_authenticated_by: Uuid,
    pub first_authenticated_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub revoked_by: Option<Uuid>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revoked_reason: Option<String>,
}

/// Device session aggregate root
///
/// This aggregate manages the lifecycle of a device's authentication session,
/// including trust relationships, PIN management, and session tokens.
#[derive(Debug, Clone)]
pub struct DeviceSession {
    /// Unique session ID
    id: Uuid,

    /// User this session belongs to
    user_id: Uuid,

    /// Device fingerprint
    device_fingerprint: DeviceFingerprint,

    /// Human-readable device name
    device_name: String,

    /// Device-bound public key for possession validation (PEM/base64 format)
    device_public_key: Option<String>,

    /// Public key algorithm identifier (e.g., 'ed25519')
    device_key_alg: Option<String>,

    /// Client platform recorded during registration/check-in.
    platform: Option<String>,

    /// Client app version recorded during registration/check-in.
    app_version: Option<String>,

    /// Optional stable hardware identifier recorded during registration/check-in.
    hardware_id: Option<String>,

    /// Current device status
    status: DeviceStatus,

    /// Whether the user currently has a PIN configured
    pin_configured: bool,

    /// Current session token
    session_token: Option<SessionToken>,

    /// Whether the current session token should be persisted on save.
    session_token_dirty: bool,

    /// Failed PIN attempts
    failed_attempts: u8,

    /// PIN lockout deadline, if the device is currently locked.
    locked_until: Option<DateTime<Utc>>,

    /// Trust expiry deadline for this device.
    trusted_until: Option<DateTime<Utc>>,

    /// Whether this specific device may auto-login/remember the user.
    auto_login_enabled: bool,

    /// User who first authenticated this device.
    first_authenticated_by: Uuid,

    /// When this device was first authenticated.
    first_authenticated_at: DateTime<Utc>,

    /// Last server check-in timestamp.
    last_seen_at: DateTime<Utc>,

    /// When the device was first registered
    created_at: DateTime<Utc>,

    /// Last activity timestamp
    last_activity: DateTime<Utc>,

    /// Last row update timestamp.
    updated_at: DateTime<Utc>,

    /// Additional persisted device metadata.
    metadata: Value,

    /// Revocation actor, if recorded.
    revoked_by: Option<Uuid>,

    /// Revocation timestamp, if recorded.
    revoked_at: Option<DateTime<Utc>>,

    /// Revocation reason, if recorded.
    revoked_reason: Option<String>,

    /// Domain events to be published
    events: Vec<AuthEvent>,
}

impl DeviceSession {
    /// Rehydrate a device session from persisted storage.
    #[cfg(feature = "database")]
    pub(crate) fn hydrate(row: DeviceSessionHydration) -> Self {
        Self {
            id: row.id,
            user_id: row.user_id,
            device_fingerprint: row.device_fingerprint,
            device_name: row.device_name,
            device_public_key: row.device_public_key,
            device_key_alg: row.device_key_alg,
            platform: row.platform,
            app_version: row.app_version,
            hardware_id: row.hardware_id,
            status: row.status,
            pin_configured: row.pin_configured,
            session_token: row.session_token,
            session_token_dirty: false,
            failed_attempts: row.failed_attempts,
            locked_until: row.locked_until,
            trusted_until: row.trusted_until,
            auto_login_enabled: row.auto_login_enabled,
            first_authenticated_by: row.first_authenticated_by,
            first_authenticated_at: row.first_authenticated_at,
            last_seen_at: row.last_seen_at,
            created_at: row.created_at,
            last_activity: row.last_activity,
            updated_at: row.updated_at,
            metadata: row.metadata,
            revoked_by: row.revoked_by,
            revoked_at: row.revoked_at,
            revoked_reason: row.revoked_reason,
            events: Vec::new(),
        }
    }

    /// Create a new device session
    pub fn new(
        user_id: Uuid,
        device_fingerprint: DeviceFingerprint,
        device_name: String,
    ) -> Self {
        Self::new_with_metadata(
            user_id,
            device_fingerprint,
            device_name,
            DeviceSessionClientMetadata::default(),
        )
    }

    /// Create a new device session with persisted client metadata.
    pub fn new_with_metadata(
        user_id: Uuid,
        device_fingerprint: DeviceFingerprint,
        device_name: String,
        metadata: DeviceSessionClientMetadata,
    ) -> Self {
        let now = Utc::now();
        let id = Uuid::now_v7();

        let mut session = Self {
            id,
            user_id,
            device_fingerprint,
            device_name: device_name.clone(),
            device_public_key: None,
            device_key_alg: None,
            platform: None,
            app_version: None,
            hardware_id: None,
            status: DeviceStatus::Pending,
            pin_configured: false,
            session_token: None,
            session_token_dirty: false,
            failed_attempts: 0,
            locked_until: None,
            trusted_until: None,
            auto_login_enabled: false,
            first_authenticated_by: user_id,
            first_authenticated_at: now,
            last_seen_at: now,
            created_at: now,
            last_activity: now,
            updated_at: now,
            metadata: json!({}),
            revoked_by: None,
            revoked_at: None,
            revoked_reason: None,
            events: Vec::new(),
        };

        session.apply_client_metadata(metadata);
        session.add_event(AuthEvent::DeviceRegistered {
            session_id: id,
            user_id,
            device_name,
            timestamp: now,
        });

        session
    }

    /// Attach a device public key and algorithm once registered
    pub fn set_device_public_key(
        &mut self,
        alg: impl Into<String>,
        key: impl Into<String>,
    ) {
        self.device_key_alg = Some(alg.into());
        self.device_public_key = Some(key.into());
        self.touch();
    }

    /// Apply metadata learned during a client check-in/login.
    pub fn apply_client_metadata(
        &mut self,
        metadata: DeviceSessionClientMetadata,
    ) {
        if let Some(platform) = metadata.platform {
            self.platform = Some(platform);
        }
        if let Some(app_version) = metadata.app_version {
            self.app_version = Some(app_version);
        }
        if let Some(hardware_id) = metadata.hardware_id {
            self.hardware_id = Some(hardware_id);
        }
        if let (Some(alg), Some(public_key)) =
            (metadata.device_key_alg, metadata.device_public_key)
        {
            self.device_key_alg = Some(alg);
            self.device_public_key = Some(public_key);
        }
        if let Some(trusted_until) = metadata.trusted_until {
            self.trusted_until = Some(trusted_until);
        }
        if let Some(auto_login_enabled) = metadata.auto_login_enabled {
            self.auto_login_enabled = auto_login_enabled;
        }
        if let Some(metadata) = metadata.metadata {
            self.metadata = metadata;
        }
        self.touch();
    }

    /// Mark the device as trusted after a PIN has been configured for the user.
    pub fn mark_trusted_after_pin_setup(&mut self) {
        self.mark_trusted_after_pin_setup_for(default_device_trust_duration());
    }

    /// Mark the device as trusted using the configured trust window.
    pub fn mark_trusted_after_pin_setup_for(
        &mut self,
        trust_duration: Duration,
    ) {
        if self.status == DeviceStatus::Pending {
            self.status = DeviceStatus::Trusted;
            self.add_event(AuthEvent::DeviceTrusted {
                session_id: self.id,
                user_id: self.user_id,
                timestamp: Utc::now(),
            });
        }

        self.pin_configured = true;
        self.failed_attempts = 0;
        self.locked_until = None;
        self.trusted_until = Some(Utc::now() + trust_duration);
        self.touch();

        self.add_event(AuthEvent::PinSet {
            session_id: self.id,
            user_id: self.user_id,
            timestamp: Utc::now(),
        });
    }

    /// Remove the configured PIN association and return the device to a pending state.
    pub fn clear_pin_association(&mut self) -> Result<(), DeviceSessionError> {
        if self.status == DeviceStatus::Revoked {
            return Err(DeviceSessionError::DeviceRevoked);
        }

        if self.status != DeviceStatus::Pending {
            self.status = DeviceStatus::Pending;
        }

        self.session_token = None;
        self.session_token_dirty = false;
        self.failed_attempts = 0;
        self.locked_until = None;
        self.trusted_until = None;
        self.touch();
        self.pin_configured = false;

        self.add_event(AuthEvent::PinRemoved {
            session_id: self.id,
            user_id: self.user_id,
            timestamp: Utc::now(),
        });

        Ok(())
    }

    /// Ensure the device can attempt PIN authentication.
    pub fn ensure_pin_available(
        &mut self,
        max_attempts: u8,
    ) -> Result<(), DeviceSessionError> {
        match self.status {
            DeviceStatus::Revoked => {
                return Err(DeviceSessionError::DeviceRevoked);
            }
            DeviceStatus::Pending => {
                return Err(DeviceSessionError::DeviceNotTrusted);
            }
            DeviceStatus::Trusted => {}
        }

        if !self.pin_configured {
            return Err(DeviceSessionError::PinNotSet);
        }

        let now = Utc::now();
        if let Some(locked_until) = self.locked_until {
            if locked_until > now {
                return Err(DeviceSessionError::TooManyFailedAttempts);
            }

            self.locked_until = None;
            self.failed_attempts = 0;
            self.touch();
        }

        if let Some(trusted_until) = self.trusted_until
            && trusted_until < now
        {
            return Err(DeviceSessionError::DeviceNotTrusted);
        }

        if self.failed_attempts >= max_attempts {
            return Err(DeviceSessionError::TooManyFailedAttempts);
        }

        Ok(())
    }

    /// Record a failed PIN authentication attempt for this device.
    pub fn register_pin_failure(
        &mut self,
        max_attempts: u8,
    ) -> DeviceSessionError {
        self.register_pin_failure_with_lockout(
            max_attempts,
            default_pin_lockout_duration(),
        )
    }

    /// Record a failed PIN authentication attempt with the configured lockout.
    pub fn register_pin_failure_with_lockout(
        &mut self,
        max_attempts: u8,
        lockout_duration: Duration,
    ) -> DeviceSessionError {
        self.failed_attempts = self.failed_attempts.saturating_add(1);
        if self.failed_attempts >= max_attempts {
            self.locked_until = Some(Utc::now() + lockout_duration);
        }
        self.touch();

        self.add_event(AuthEvent::AuthenticationFailed {
            session_id: self.id,
            user_id: self.user_id,
            reason: "Invalid PIN".to_string(),
            timestamp: Utc::now(),
        });

        if self.failed_attempts >= max_attempts {
            DeviceSessionError::TooManyFailedAttempts
        } else {
            DeviceSessionError::InvalidPin
        }
    }

    /// Issue a session token after a successful PIN verification.
    pub fn issue_pin_session(
        &mut self,
        session_lifetime: Duration,
    ) -> Result<SessionToken, DeviceSessionError> {
        self.issue_pin_session_with_trust_duration(
            session_lifetime,
            default_device_trust_duration(),
        )
    }

    /// Issue a session token and extend trust using the configured trust window.
    pub fn issue_pin_session_with_trust_duration(
        &mut self,
        session_lifetime: Duration,
        trust_duration: Duration,
    ) -> Result<SessionToken, DeviceSessionError> {
        if self.status != DeviceStatus::Trusted {
            return Err(DeviceSessionError::DeviceNotTrusted);
        }

        let token = SessionToken::generate(session_lifetime)
            .map_err(|_| DeviceSessionError::InvalidStateTransition)?;

        self.session_token = Some(token.clone());
        self.session_token_dirty = true;
        self.failed_attempts = 0;
        self.locked_until = None;
        self.trusted_until = Some(Utc::now() + trust_duration);
        self.touch();

        self.add_event(AuthEvent::SessionCreated {
            session_id: self.id,
            user_id: self.user_id,
            expires_at: token.expires_at(),
            timestamp: Utc::now(),
        });

        Ok(token)
    }

    /// Reset failure counters after a successful PIN verification.
    pub fn record_pin_success(&mut self) {
        self.record_pin_success_for(default_device_trust_duration());
    }

    /// Reset failure counters and extend trust using the configured trust window.
    pub fn record_pin_success_for(&mut self, trust_duration: Duration) {
        self.failed_attempts = 0;
        self.locked_until = None;
        self.trusted_until = Some(Utc::now() + trust_duration);
        self.touch();
    }

    /// Refresh the session token if valid
    pub fn refresh_token(
        &mut self,
        session_lifetime: Duration,
    ) -> Result<SessionToken, DeviceSessionError> {
        self.refresh_token_with_trust_duration(
            session_lifetime,
            default_device_trust_duration(),
        )
    }

    /// Refresh a session token and extend trust using the configured trust window.
    pub fn refresh_token_with_trust_duration(
        &mut self,
        session_lifetime: Duration,
        trust_duration: Duration,
    ) -> Result<SessionToken, DeviceSessionError> {
        // Check device status
        if self.status != DeviceStatus::Trusted {
            return Err(DeviceSessionError::DeviceNotTrusted);
        }

        // Check if current token is still valid
        let current_token = self
            .session_token
            .as_ref()
            .ok_or(DeviceSessionError::SessionExpired)?;

        if current_token.is_expired() {
            return Err(DeviceSessionError::SessionExpired);
        }

        // Generate new token
        let token = SessionToken::generate(session_lifetime)
            .map_err(|_| DeviceSessionError::InvalidStateTransition)?;

        self.session_token = Some(token.clone());
        self.session_token_dirty = true;
        self.trusted_until = Some(Utc::now() + trust_duration);
        self.touch();

        self.add_event(AuthEvent::SessionRefreshed {
            session_id: self.id,
            user_id: self.user_id,
            expires_at: token.expires_at(),
            timestamp: Utc::now(),
        });

        Ok(token)
    }

    /// Revoke this device session
    pub fn revoke(&mut self) -> Result<(), DeviceSessionError> {
        if self.status == DeviceStatus::Revoked {
            return Ok(()); // Already revoked
        }

        self.status = DeviceStatus::Revoked;
        self.session_token = None;
        self.session_token_dirty = false;
        self.trusted_until = None;
        self.auto_login_enabled = false;
        self.revoked_at = Some(Utc::now());
        self.touch();

        self.add_event(AuthEvent::DeviceRevoked {
            session_id: self.id,
            user_id: self.user_id,
            timestamp: Utc::now(),
        });

        Ok(())
    }

    /// Update last activity timestamp
    pub fn update_activity(&mut self) {
        self.update_activity_with_trust_duration(
            default_device_trust_duration(),
        );
    }

    /// Update last activity and extend explicit trust with the configured window.
    pub fn update_activity_with_trust_duration(
        &mut self,
        trust_duration: Duration,
    ) {
        self.touch();
        if self.status == DeviceStatus::Trusted {
            self.trusted_until = Some(Utc::now() + trust_duration);
        }
    }

    fn touch(&mut self) {
        let now = Utc::now();
        self.last_activity = now;
        self.last_seen_at = now;
        self.updated_at = now;
    }

    /// Check if the session token is valid
    pub fn is_token_valid(&self) -> bool {
        self.session_token
            .as_ref()
            .map(|t| t.is_valid())
            .unwrap_or(false)
    }

    /// Add a domain event
    fn add_event(&mut self, event: AuthEvent) {
        self.events.push(event);
    }

    /// Take all pending events (for publishing)
    pub fn take_events(&mut self) -> Vec<AuthEvent> {
        std::mem::take(&mut self.events)
    }

    // Getters for read-only access
    pub fn id(&self) -> Uuid {
        self.id
    }
    pub fn user_id(&self) -> Uuid {
        self.user_id
    }
    pub fn device_fingerprint(&self) -> &DeviceFingerprint {
        &self.device_fingerprint
    }
    pub fn device_name(&self) -> &str {
        &self.device_name
    }
    pub fn device_public_key(&self) -> Option<&str> {
        self.device_public_key.as_deref()
    }
    pub fn device_key_alg(&self) -> Option<&str> {
        self.device_key_alg.as_deref()
    }
    pub fn platform(&self) -> Option<&str> {
        self.platform.as_deref()
    }
    pub fn app_version(&self) -> Option<&str> {
        self.app_version.as_deref()
    }
    pub fn hardware_id(&self) -> Option<&str> {
        self.hardware_id.as_deref()
    }
    pub fn status(&self) -> DeviceStatus {
        self.status
    }
    pub fn has_pin(&self) -> bool {
        self.pin_configured
    }
    pub fn failed_attempts(&self) -> u8 {
        self.failed_attempts
    }
    pub fn locked_until(&self) -> Option<DateTime<Utc>> {
        self.locked_until
    }
    pub fn trusted_until(&self) -> Option<DateTime<Utc>> {
        self.trusted_until
    }
    pub fn auto_login_enabled(&self) -> bool {
        self.auto_login_enabled
    }
    pub fn first_authenticated_by(&self) -> Uuid {
        self.first_authenticated_by
    }
    pub fn first_authenticated_at(&self) -> DateTime<Utc> {
        self.first_authenticated_at
    }
    pub fn last_seen_at(&self) -> DateTime<Utc> {
        self.last_seen_at
    }
    pub fn metadata(&self) -> &Value {
        &self.metadata
    }
    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
    pub fn revoked_by(&self) -> Option<Uuid> {
        self.revoked_by
    }
    pub fn revoked_at(&self) -> Option<DateTime<Utc>> {
        self.revoked_at
    }
    pub fn revoked_reason(&self) -> Option<&str> {
        self.revoked_reason.as_deref()
    }
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    pub fn last_activity(&self) -> DateTime<Utc> {
        self.last_activity
    }
    pub fn session_token(&self) -> Option<&SessionToken> {
        self.session_token.as_ref()
    }

    pub fn should_persist_session_token(&self) -> bool {
        self.session_token_dirty
    }

    /// Whether the device session is currently trusted.
    pub fn is_trusted(&self) -> bool {
        self.status == DeviceStatus::Trusted
            && self.revoked_at.is_none()
            && self
                .trusted_until
                .map(|trusted_until| trusted_until >= Utc::now())
                .unwrap_or(true)
    }

    /// Whether the device session has been revoked.
    pub fn is_revoked(&self) -> bool {
        self.status == DeviceStatus::Revoked
    }

    /// Replace the in-memory session token (typically with the persisted hash)
    pub fn set_persisted_token(&mut self, token: Option<SessionToken>) {
        self.session_token = token;
        self.session_token_dirty = self.session_token.is_some();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_lifecycle() {
        let fingerprint = DeviceFingerprint::new(
            "Linux".to_string(),
            Some("Intel".to_string()),
            None,
            None,
            None,
        )
        .unwrap();

        let mut session = DeviceSession::new(
            Uuid::now_v7(),
            fingerprint,
            "Test Device".to_string(),
        );

        // Initially pending
        assert_eq!(session.status(), DeviceStatus::Pending);
        assert!(!session.has_pin());

        // Mark trusted after PIN setup
        session.mark_trusted_after_pin_setup();

        // Now trusted
        assert_eq!(session.status(), DeviceStatus::Trusted);
        assert!(session.has_pin());

        // Authenticate (server verification happens upstream)
        session.ensure_pin_available(3).unwrap();
        let token = session.issue_pin_session(Duration::hours(1)).unwrap();
        assert!(token.is_valid());

        // Simulate a failure and ensure lockout rules apply
        session.ensure_pin_available(3).unwrap();
        assert!(matches!(
            session.register_pin_failure(3),
            DeviceSessionError::InvalidPin
        ));

        session.clear_pin_association().unwrap();
        assert_eq!(session.status(), DeviceStatus::Pending);
        assert!(!session.has_pin());

        // Revoke
        session.revoke().unwrap();
        assert_eq!(session.status(), DeviceStatus::Revoked);
    }
}
