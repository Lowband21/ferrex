use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

use crate::domain::users::auth::AuthCrypto;
use crate::domain::users::auth::domain::aggregates::{
    DeviceSession, DeviceStatus,
};
use crate::domain::users::auth::domain::events::AuthEvent;
use crate::domain::users::auth::domain::value_objects::PinCodeError;
use crate::domain::users::auth::domain::value_objects::{
    DeviceFingerprint, PinCode, PinPolicy, SessionToken,
};

/// Errors that can occur during user authentication
#[derive(Debug, Error)]
pub enum UserAuthenticationError {
    #[error("User not found")]
    UserNotFound,

    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("Account locked")]
    AccountLocked,

    #[error("Account inactive")]
    AccountInactive,

    #[error("Device not found")]
    DeviceNotFound,

    #[error("Too many devices")]
    TooManyDevices,

    #[error("Invalid device session")]
    InvalidDeviceSession(
        #[from]
        crate::domain::users::auth::domain::aggregates::DeviceSessionError,
    ),
}

/// User authentication aggregate
///
/// This aggregate manages user authentication across multiple devices,
/// enforcing business rules and maintaining consistency.
#[derive(Debug, Clone)]
pub struct UserAuthentication {
    /// User ID
    user_id: Uuid,

    /// Username for login
    username: String,

    /// Password hash (Argon2id)
    password_hash: String,

    /// Whether the account is active
    is_active: bool,

    /// Whether the account is locked
    is_locked: bool,

    /// Failed login attempts
    failed_login_attempts: u8,

    /// When the account lock expires
    locked_until: Option<DateTime<Utc>>,

    /// User-level PIN credential (hashed client proof)
    user_pin: Option<PinCode>,

    /// Last time the PIN was updated
    pin_updated_at: Option<DateTime<Utc>>,

    /// Server-issued salt that scopes client-side proofs
    pin_client_salt: Vec<u8>,

    /// Device sessions by device fingerprint
    device_sessions: HashMap<String, DeviceSession>,

    /// Maximum devices per user
    max_devices: usize,

    /// Last successful login
    last_login: Option<DateTime<Utc>>,

    /// Domain events
    events: Vec<AuthEvent>,
}

impl UserAuthentication {
    /// Rehydrate a user authentication aggregate from persisted storage
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn hydrate(
        user_id: Uuid,
        username: String,
        password_hash: String,
        is_active: bool,
        is_locked: bool,
        failed_login_attempts: u8,
        locked_until: Option<DateTime<Utc>>,
        user_pin: Option<PinCode>,
        pin_updated_at: Option<DateTime<Utc>>,
        pin_client_salt: Vec<u8>,
        device_sessions: HashMap<String, DeviceSession>,
        max_devices: usize,
        last_login: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            user_id,
            username,
            password_hash,
            is_active,
            is_locked,
            failed_login_attempts,
            locked_until,
            user_pin,
            pin_updated_at,
            pin_client_salt,
            device_sessions,
            max_devices,
            last_login,
            events: Vec::new(),
        }
    }

    /// Create a new user authentication aggregate
    pub fn new(
        user_id: Uuid,
        username: String,
        password_hash: String,
        max_devices: usize,
    ) -> Self {
        let mut salt = vec![0u8; 16];
        rand::rng().fill_bytes(&mut salt);

        Self {
            user_id,
            username,
            password_hash,
            is_active: true,
            is_locked: false,
            failed_login_attempts: 0,
            locked_until: None,
            user_pin: None,
            pin_updated_at: None,
            pin_client_salt: salt,
            device_sessions: HashMap::new(),
            max_devices,
            last_login: None,
            events: Vec::new(),
        }
    }

    /// Whether the user currently has a configured PIN.
    pub fn has_user_pin(&self) -> bool {
        self.user_pin.is_some()
    }

    /// Timestamp of the last PIN update, if any.
    pub fn pin_updated_at(&self) -> Option<DateTime<Utc>> {
        self.pin_updated_at
    }

    /// Current server-issued salt for client PIN proofs.
    pub fn pin_client_salt(&self) -> &[u8] {
        &self.pin_client_salt
    }

    /// Configure or replace the user-level PIN using a client-derived proof.
    pub fn set_user_pin(
        &mut self,
        client_proof: &str,
        policy: &PinPolicy,
        crypto: &AuthCrypto,
    ) -> Result<(), PinCodeError> {
        let pin = PinCode::new(client_proof.to_string(), policy, crypto)?;
        self.user_pin = Some(pin);
        self.pin_updated_at = Some(Utc::now());
        Ok(())
    }

    /// Remove the configured user-level PIN.
    pub fn clear_user_pin(&mut self) {
        self.user_pin = None;
        self.pin_updated_at = None;
    }

    /// Rotate the server-issued salt for future client proofs.
    pub fn rotate_pin_client_salt(&mut self) {
        let mut salt = vec![0u8; 16];
        rand::rng().fill_bytes(&mut salt);
        self.pin_client_salt = salt;
    }

    /// Verify a client proof against the stored user-level PIN hash.
    pub fn verify_user_pin(
        &self,
        client_proof: &str,
        crypto: &AuthCrypto,
    ) -> Result<bool, PinCodeError> {
        match &self.user_pin {
            Some(pin) => pin.verify(client_proof, crypto),
            None => Ok(false),
        }
    }

    /// Get the stored PIN hash if present.
    pub fn pin_hash(&self) -> Option<&str> {
        self.user_pin.as_ref().map(|pin| pin.hash())
    }

    /// Authenticate with username and password
    pub fn authenticate_password(
        &mut self,
        password: &str,
        crypto: &AuthCrypto,
    ) -> Result<(), UserAuthenticationError> {
        use argon2::{Argon2, PasswordHash, PasswordVerifier};

        // Check if account is active
        if !self.is_active {
            return Err(UserAuthenticationError::AccountInactive);
        }

        // Check if account is locked
        if self.is_locked
            && let Some(until) = self.locked_until
        {
            if Utc::now() < until {
                return Err(UserAuthenticationError::AccountLocked);
            } else {
                // Unlock expired lock
                self.is_locked = false;
                self.locked_until = None;
                self.failed_login_attempts = 0;
            }
        }

        // Verify password
        let mut verified = crypto
            .verify_password(password, &self.password_hash)
            .unwrap_or(false);
        let mut needs_rehash = false;

        if !verified {
            let parsed_hash = PasswordHash::new(&self.password_hash)
                .map_err(|_| UserAuthenticationError::InvalidCredentials)?;

            let argon2 = Argon2::default();
            if argon2
                .verify_password(password.as_bytes(), &parsed_hash)
                .is_ok()
            {
                verified = true;
                needs_rehash = true;
            }
        }

        if !verified {
            self.failed_login_attempts += 1;

            // Lock account after 5 failed attempts
            if self.failed_login_attempts >= 5 {
                self.is_locked = true;
                self.locked_until = Some(Utc::now() + Duration::minutes(15));

                self.add_event(AuthEvent::AccountLocked {
                    user_id: self.user_id,
                    locked_until: self.locked_until.unwrap(),
                    timestamp: Utc::now(),
                });
            }

            self.add_event(AuthEvent::AuthenticationFailed {
                session_id: Uuid::nil(), // No session yet
                user_id: self.user_id,
                reason: "Invalid password".to_string(),
                timestamp: Utc::now(),
            });

            return Err(UserAuthenticationError::InvalidCredentials);
        }

        // Reset failed attempts on success
        self.failed_login_attempts = 0;
        self.last_login = Some(Utc::now());

        if needs_rehash && let Ok(new_hash) = crypto.hash_password(password) {
            self.password_hash = new_hash;
        }

        self.add_event(AuthEvent::PasswordAuthenticated {
            user_id: self.user_id,
            timestamp: Utc::now(),
        });

        Ok(())
    }

    /// Register a new device or get existing
    pub fn register_device(
        &mut self,
        device_fingerprint: DeviceFingerprint,
        device_name: String,
    ) -> Result<(), UserAuthenticationError> {
        let fingerprint_str = device_fingerprint.as_str().to_string();

        // Check if device already exists
        if let Some(session) = self.device_sessions.get_mut(&fingerprint_str) {
            // Update activity
            session.update_activity();
            return Ok(());
        }

        // Check device limit
        let active_devices = self
            .device_sessions
            .values()
            .filter(|s| s.status() != DeviceStatus::Revoked)
            .count();

        if active_devices >= self.max_devices {
            return Err(UserAuthenticationError::TooManyDevices);
        }

        // Create new device session
        let session =
            DeviceSession::new(self.user_id, device_fingerprint, device_name);

        self.device_sessions.insert(fingerprint_str, session);

        Ok(())
    }

    /// Get a device session by fingerprint
    pub fn get_device_session(
        &mut self,
        fingerprint: &DeviceFingerprint,
    ) -> Result<&mut DeviceSession, UserAuthenticationError> {
        self.device_sessions
            .get_mut(fingerprint.as_str())
            .ok_or(UserAuthenticationError::DeviceNotFound)
    }

    /// Authenticate device with PIN
    pub fn authenticate_device(
        &mut self,
        fingerprint: &DeviceFingerprint,
        pin_proof: &str,
        max_attempts: u8,
        session_lifetime: Duration,
        crypto: &AuthCrypto,
    ) -> Result<SessionToken, UserAuthenticationError> {
        {
            let session = self.get_device_session(fingerprint)?;
            session.ensure_pin_available(max_attempts)?;
        }

        let verified = self
            .verify_user_pin(pin_proof, crypto)
            .map_err(|_| UserAuthenticationError::InvalidCredentials)?;

        if !verified {
            let session = self.get_device_session(fingerprint)?;
            let err = session.register_pin_failure(max_attempts);
            let device_events = session.take_events();
            self.events.extend(device_events);
            return Err(err.into());
        }

        let session = self.get_device_session(fingerprint)?;
        session.record_pin_success();
        let token = session.issue_pin_session(session_lifetime)?;

        // Collect events from device session
        let device_events = session.take_events();
        self.events.extend(device_events);

        Ok(token)
    }

    /// Set PIN for a device
    pub fn set_device_pin(
        &mut self,
        fingerprint: &DeviceFingerprint,
        pin_proof: String,
        policy: &PinPolicy,
        crypto: &AuthCrypto,
    ) -> Result<(), UserAuthenticationError> {
        self.set_user_pin(&pin_proof, policy, crypto)
            .map_err(|_| UserAuthenticationError::InvalidCredentials)?;

        let session = self.get_device_session(fingerprint)?;
        session.mark_trusted_after_pin_setup();

        // Collect events from device session
        let device_events = session.take_events();
        self.events.extend(device_events);

        Ok(())
    }

    /// Refresh a device session token
    pub fn refresh_device_token(
        &mut self,
        fingerprint: &DeviceFingerprint,
        session_lifetime: Duration,
    ) -> Result<SessionToken, UserAuthenticationError> {
        let session = self.get_device_session(fingerprint)?;
        let token = session.refresh_token(session_lifetime)?;

        // Collect events from device session
        let device_events = session.take_events();
        self.events.extend(device_events);

        Ok(token)
    }

    /// Revoke a device
    pub fn revoke_device(
        &mut self,
        fingerprint: &DeviceFingerprint,
    ) -> Result<(), UserAuthenticationError> {
        let session = self.get_device_session(fingerprint)?;
        session.revoke()?;

        // Collect events from device session
        let device_events = session.take_events();
        self.events.extend(device_events);

        Ok(())
    }

    /// Revoke all devices
    pub fn revoke_all_devices(&mut self) {
        for session in self.device_sessions.values_mut() {
            let _ = session.revoke();
            let device_events = session.take_events();
            self.events.extend(device_events);
        }

        self.add_event(AuthEvent::AllDevicesRevoked {
            user_id: self.user_id,
            timestamp: Utc::now(),
        });
    }

    /// Get active device sessions
    pub fn active_devices(&self) -> Vec<&DeviceSession> {
        self.device_sessions
            .values()
            .filter(|s| s.status() == DeviceStatus::Trusted)
            .collect()
    }

    /// Update password
    pub fn update_password(&mut self, new_password_hash: String) {
        self.password_hash = new_password_hash;
        self.failed_login_attempts = 0;

        // Revoke all devices when password changes
        self.revoke_all_devices();

        self.add_event(AuthEvent::PasswordChanged {
            user_id: self.user_id,
            timestamp: Utc::now(),
        });
    }

    /// Lock the account
    pub fn lock_account(&mut self, duration: Duration) {
        self.is_locked = true;
        self.locked_until = Some(Utc::now() + duration);

        self.add_event(AuthEvent::AccountLocked {
            user_id: self.user_id,
            locked_until: self.locked_until.unwrap(),
            timestamp: Utc::now(),
        });
    }

    /// Unlock the account
    pub fn unlock_account(&mut self) {
        self.is_locked = false;
        self.locked_until = None;
        self.failed_login_attempts = 0;

        self.add_event(AuthEvent::AccountUnlocked {
            user_id: self.user_id,
            timestamp: Utc::now(),
        });
    }

    /// Deactivate the account
    pub fn deactivate(&mut self) {
        self.is_active = false;
        self.revoke_all_devices();

        self.add_event(AuthEvent::AccountDeactivated {
            user_id: self.user_id,
            timestamp: Utc::now(),
        });
    }

    /// Add a domain event
    fn add_event(&mut self, event: AuthEvent) {
        self.events.push(event);
    }

    /// Take all pending events
    pub fn take_events(&mut self) -> Vec<AuthEvent> {
        std::mem::take(&mut self.events)
    }

    // Getters
    pub fn user_id(&self) -> Uuid {
        self.user_id
    }
    pub fn username(&self) -> &str {
        &self.username
    }
    pub fn password_hash(&self) -> &str {
        &self.password_hash
    }
    pub fn is_active(&self) -> bool {
        self.is_active
    }
    pub fn is_locked(&self) -> bool {
        self.is_locked
    }
    pub fn failed_login_attempts(&self) -> u8 {
        self.failed_login_attempts
    }
    pub fn locked_until(&self) -> Option<DateTime<Utc>> {
        self.locked_until
    }
    pub fn last_login(&self) -> Option<DateTime<Utc>> {
        self.last_login
    }

    pub fn max_devices(&self) -> usize {
        self.max_devices
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use argon2::{
        Argon2, PasswordHasher,
        password_hash::{SaltString, rand_core::OsRng},
    };

    fn hash_password(password: &str) -> String {
        let mut rng = OsRng;
        let salt = SaltString::generate(&mut rng);

        let argon2 = Argon2::default();
        argon2
            .hash_password(password.as_bytes(), &salt)
            .unwrap()
            .to_string()
    }

    #[test]
    fn test_user_authentication_flow() {
        let password_hash = hash_password("password123");
        let mut auth = UserAuthentication::new(
            Uuid::now_v7(),
            "testuser".to_string(),
            password_hash,
            5,
        );

        let crypto = AuthCrypto::new("pepper", "token-key").unwrap();

        // Test password authentication
        auth.authenticate_password("password123", &crypto).unwrap();
        assert_eq!(auth.failed_login_attempts, 0);

        // Test failed authentication
        let result = auth.authenticate_password("wrong", &crypto);
        assert!(result.is_err());
        assert_eq!(auth.failed_login_attempts, 1);
    }
}
