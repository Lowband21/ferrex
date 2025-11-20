//! Type-safe authentication state machine with compile-time guarantees
//!
//! This module implements a state machine for authentication flows that ensures
//! only valid state transitions can be compiled. It uses phantom types to encode
//! states at the type level and const generics for configuration.

use crate::AuthToken;
use crate::auth::device::DeviceRegistration;
use crate::rbac::UserPermissions;
use crate::user::User;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::marker::PhantomData;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Marker traits for authentication states
pub trait AuthState: std::fmt::Debug + Clone + Send + Sync + 'static {}

/// Unauthenticated state - no user session
#[derive(Debug, Clone)]
pub struct Unauthenticated;
impl AuthState for Unauthenticated {}

/// User selected state - user chosen but not yet verified
#[derive(Debug, Clone)]
pub struct UserSelected {
    pub user_id: Uuid,
    pub timestamp: Instant,
}
impl AuthState for UserSelected {}

/// Awaiting password state - device check complete, need password
#[derive(Debug, Clone)]
pub struct AwaitingPassword {
    pub user_id: Uuid,
    pub device_id: Uuid,
    pub attempts: u8,
    pub timestamp: Instant,
}
impl AuthState for AwaitingPassword {}

/// Awaiting PIN state - trusted device, need PIN verification
#[derive(Debug, Clone)]
pub struct AwaitingPin {
    pub user_id: Uuid,
    pub device_registration: DeviceRegistration,
    pub attempts: u8,
    pub timestamp: Instant,
}
impl AuthState for AwaitingPin {}

/// Authenticated state - full access granted
#[derive(Debug, Clone)]
pub struct Authenticated {
    pub user: User,
    pub token: AuthToken,
    pub permissions: UserPermissions,
    pub device_id: Uuid,
    pub server_url: String,
    pub timestamp: Instant,
}
impl AuthState for Authenticated {}

/// Setting up PIN state - after password auth, establishing PIN
#[derive(Debug, Clone)]
pub struct SettingUpPin {
    pub user: User,
    pub token: AuthToken,
    pub device_id: Uuid,
    pub timestamp: Instant,
}
impl AuthState for SettingUpPin {}

/// Token refresh state - maintaining session
#[derive(Debug, Clone)]
pub struct Refreshing {
    pub previous_state: Authenticated,
    pub timestamp: Instant,
}
impl AuthState for Refreshing {}

/// Configuration for the state machine using const generics
#[derive(Debug, Clone)]
pub struct AuthConfig<const MAX_ATTEMPTS: u8 = 3, const TIMEOUT_SECS: u64 = 300>;

/// Type-safe authentication state machine
#[derive(Debug, Clone)]
pub struct AuthStateMachine<S: AuthState, const MAX_ATTEMPTS: u8 = 3, const TIMEOUT_SECS: u64 = 300>
{
    state_data: S,
    _phantom: PhantomData<S>,
}

/// State transition results
pub type TransitionResult<S> = Result<S, AuthTransitionError>;

/// Errors that can occur during state transitions
#[derive(Debug, Clone, thiserror::Error)]
pub enum AuthTransitionError {
    #[error("Invalid user ID")]
    InvalidUser,

    #[error("Device not recognized")]
    UnknownDevice,

    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("Too many attempts ({0}/{1})")]
    TooManyAttempts(u8, u8),

    #[error("State timeout after {0:?}")]
    Timeout(Duration),

    #[error("Invalid PIN format")]
    InvalidPinFormat,

    #[error("Token refresh failed: {0}")]
    RefreshFailed(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Storage error: {0}")]
    StorageError(String),
}

// Initial state constructor
impl<const MAX_ATTEMPTS: u8, const TIMEOUT_SECS: u64> Default
    for AuthStateMachine<Unauthenticated, MAX_ATTEMPTS, TIMEOUT_SECS>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_ATTEMPTS: u8, const TIMEOUT_SECS: u64>
    AuthStateMachine<Unauthenticated, MAX_ATTEMPTS, TIMEOUT_SECS>
{
    /// Create a new state machine in the unauthenticated state
    pub fn new() -> Self {
        Self {
            state_data: Unauthenticated,
            _phantom: PhantomData,
        }
    }
}

// State transitions from Unauthenticated
impl<const MAX_ATTEMPTS: u8, const TIMEOUT_SECS: u64>
    AuthStateMachine<Unauthenticated, MAX_ATTEMPTS, TIMEOUT_SECS>
{
    /// Select a user to begin authentication
    pub fn select_user(
        self,
        user_id: Uuid,
    ) -> TransitionResult<AuthStateMachine<UserSelected, MAX_ATTEMPTS, TIMEOUT_SECS>> {
        if user_id == Uuid::nil() {
            return Err(AuthTransitionError::InvalidUser);
        }

        Ok(AuthStateMachine {
            state_data: UserSelected {
                user_id,
                timestamp: Instant::now(),
            },
            _phantom: PhantomData,
        })
    }
}

// State transitions from UserSelected
impl<const MAX_ATTEMPTS: u8, const TIMEOUT_SECS: u64>
    AuthStateMachine<UserSelected, MAX_ATTEMPTS, TIMEOUT_SECS>
{
    /// Check if the state has timed out
    fn check_timeout(&self) -> Result<(), AuthTransitionError> {
        let elapsed = self.state_data.timestamp.elapsed();
        if elapsed > Duration::from_secs(TIMEOUT_SECS) {
            return Err(AuthTransitionError::Timeout(elapsed));
        }
        Ok(())
    }

    /// Device is not trusted, require password
    pub fn require_password(
        self,
        device_id: Uuid,
    ) -> TransitionResult<AuthStateMachine<AwaitingPassword, MAX_ATTEMPTS, TIMEOUT_SECS>> {
        self.check_timeout()?;

        Ok(AuthStateMachine {
            state_data: AwaitingPassword {
                user_id: self.state_data.user_id,
                device_id,
                attempts: 0,
                timestamp: Instant::now(),
            },
            _phantom: PhantomData,
        })
    }

    /// Device is trusted, require PIN
    pub fn require_pin(
        self,
        device_registration: DeviceRegistration,
    ) -> TransitionResult<AuthStateMachine<AwaitingPin, MAX_ATTEMPTS, TIMEOUT_SECS>> {
        self.check_timeout()?;

        Ok(AuthStateMachine {
            state_data: AwaitingPin {
                user_id: self.state_data.user_id,
                device_registration,
                attempts: 0,
                timestamp: Instant::now(),
            },
            _phantom: PhantomData,
        })
    }

    /// Cancel and return to unauthenticated
    pub fn cancel(self) -> AuthStateMachine<Unauthenticated, MAX_ATTEMPTS, TIMEOUT_SECS> {
        AuthStateMachine::new()
    }
}

// State transitions from AwaitingPassword
impl<const MAX_ATTEMPTS: u8, const TIMEOUT_SECS: u64>
    AuthStateMachine<AwaitingPassword, MAX_ATTEMPTS, TIMEOUT_SECS>
{
    /// Check if the state has timed out
    fn check_timeout(&self) -> Result<(), AuthTransitionError> {
        let elapsed = self.state_data.timestamp.elapsed();
        if elapsed > Duration::from_secs(TIMEOUT_SECS) {
            return Err(AuthTransitionError::Timeout(elapsed));
        }
        Ok(())
    }

    /// Attempt password authentication
    pub fn verify_password(
        self,
        user: User,
        token: AuthToken,
        permissions: UserPermissions,
        server_url: String,
    ) -> TransitionResult<AuthStateMachine<Authenticated, MAX_ATTEMPTS, TIMEOUT_SECS>> {
        self.check_timeout()?;

        Ok(AuthStateMachine {
            state_data: Authenticated {
                user,
                token,
                permissions,
                device_id: self.state_data.device_id,
                server_url,
                timestamp: Instant::now(),
            },
            _phantom: PhantomData,
        })
    }

    /// Failed password attempt
    pub fn fail_attempt(
        mut self,
    ) -> TransitionResult<AuthStateMachine<AwaitingPassword, MAX_ATTEMPTS, TIMEOUT_SECS>> {
        self.check_timeout()?;

        self.state_data.attempts += 1;
        if self.state_data.attempts >= MAX_ATTEMPTS {
            return Err(AuthTransitionError::TooManyAttempts(
                self.state_data.attempts,
                MAX_ATTEMPTS,
            ));
        }

        self.state_data.timestamp = Instant::now();
        Ok(self)
    }

    /// Cancel and return to unauthenticated
    pub fn cancel(self) -> AuthStateMachine<Unauthenticated, MAX_ATTEMPTS, TIMEOUT_SECS> {
        AuthStateMachine::new()
    }
}

// State transitions from AwaitingPin
impl<const MAX_ATTEMPTS: u8, const TIMEOUT_SECS: u64>
    AuthStateMachine<AwaitingPin, MAX_ATTEMPTS, TIMEOUT_SECS>
{
    /// Check if the state has timed out
    fn check_timeout(&self) -> Result<(), AuthTransitionError> {
        let elapsed = self.state_data.timestamp.elapsed();
        if elapsed > Duration::from_secs(TIMEOUT_SECS) {
            return Err(AuthTransitionError::Timeout(elapsed));
        }
        Ok(())
    }

    /// Verify PIN and complete authentication
    pub fn verify_pin(
        self,
        user: User,
        token: AuthToken,
        permissions: UserPermissions,
        server_url: String,
    ) -> TransitionResult<AuthStateMachine<Authenticated, MAX_ATTEMPTS, TIMEOUT_SECS>> {
        self.check_timeout()?;

        Ok(AuthStateMachine {
            state_data: Authenticated {
                user,
                token,
                permissions,
                device_id: self.state_data.device_registration.device_id,
                server_url,
                timestamp: Instant::now(),
            },
            _phantom: PhantomData,
        })
    }

    /// Failed PIN attempt
    pub fn fail_attempt(
        mut self,
    ) -> TransitionResult<AuthStateMachine<AwaitingPin, MAX_ATTEMPTS, TIMEOUT_SECS>> {
        self.check_timeout()?;

        self.state_data.attempts += 1;
        if self.state_data.attempts >= MAX_ATTEMPTS {
            return Err(AuthTransitionError::TooManyAttempts(
                self.state_data.attempts,
                MAX_ATTEMPTS,
            ));
        }

        self.state_data.timestamp = Instant::now();
        Ok(self)
    }

    /// Cancel and return to unauthenticated
    pub fn cancel(self) -> AuthStateMachine<Unauthenticated, MAX_ATTEMPTS, TIMEOUT_SECS> {
        AuthStateMachine::new()
    }
}

// State transitions from Authenticated
impl<const MAX_ATTEMPTS: u8, const TIMEOUT_SECS: u64>
    AuthStateMachine<Authenticated, MAX_ATTEMPTS, TIMEOUT_SECS>
{
    /// Begin token refresh
    pub fn start_refresh(self) -> AuthStateMachine<Refreshing, MAX_ATTEMPTS, TIMEOUT_SECS> {
        AuthStateMachine {
            state_data: Refreshing {
                previous_state: self.state_data,
                timestamp: Instant::now(),
            },
            _phantom: PhantomData,
        }
    }

    /// Begin PIN setup for trusted device
    pub fn start_pin_setup(self) -> AuthStateMachine<SettingUpPin, MAX_ATTEMPTS, TIMEOUT_SECS> {
        AuthStateMachine {
            state_data: SettingUpPin {
                user: self.state_data.user,
                token: self.state_data.token,
                device_id: self.state_data.device_id,
                timestamp: Instant::now(),
            },
            _phantom: PhantomData,
        }
    }

    /// Logout and return to unauthenticated
    pub fn logout(self) -> AuthStateMachine<Unauthenticated, MAX_ATTEMPTS, TIMEOUT_SECS> {
        AuthStateMachine::new()
    }
}

// State transitions from Refreshing
impl<const MAX_ATTEMPTS: u8, const TIMEOUT_SECS: u64>
    AuthStateMachine<Refreshing, MAX_ATTEMPTS, TIMEOUT_SECS>
{
    /// Complete token refresh successfully
    pub fn complete_refresh(
        self,
        new_token: AuthToken,
    ) -> AuthStateMachine<Authenticated, MAX_ATTEMPTS, TIMEOUT_SECS> {
        let mut auth_state = self.state_data.previous_state;
        auth_state.token = new_token;
        auth_state.timestamp = Instant::now();

        AuthStateMachine {
            state_data: auth_state,
            _phantom: PhantomData,
        }
    }

    /// Refresh failed, return to previous state
    pub fn fail_refresh(self) -> AuthStateMachine<Authenticated, MAX_ATTEMPTS, TIMEOUT_SECS> {
        AuthStateMachine {
            state_data: self.state_data.previous_state,
            _phantom: PhantomData,
        }
    }

    /// Refresh failed critically, logout
    pub fn critical_failure(self) -> AuthStateMachine<Unauthenticated, MAX_ATTEMPTS, TIMEOUT_SECS> {
        AuthStateMachine::new()
    }
}

// State transitions from SettingUpPin
impl<const MAX_ATTEMPTS: u8, const TIMEOUT_SECS: u64>
    AuthStateMachine<SettingUpPin, MAX_ATTEMPTS, TIMEOUT_SECS>
{
    /// Complete PIN setup
    pub fn complete_setup(
        self,
        permissions: UserPermissions,
        server_url: String,
    ) -> AuthStateMachine<Authenticated, MAX_ATTEMPTS, TIMEOUT_SECS> {
        AuthStateMachine {
            state_data: Authenticated {
                user: self.state_data.user,
                token: self.state_data.token,
                permissions,
                device_id: self.state_data.device_id,
                server_url,
                timestamp: Instant::now(),
            },
            _phantom: PhantomData,
        }
    }

    /// Cancel PIN setup but remain authenticated
    pub fn skip_setup(
        self,
        permissions: UserPermissions,
        server_url: String,
    ) -> AuthStateMachine<Authenticated, MAX_ATTEMPTS, TIMEOUT_SECS> {
        self.complete_setup(permissions, server_url)
    }
}

// Display implementations for all states
impl Display for Unauthenticated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Unauthenticated")
    }
}

impl Display for UserSelected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UserSelected(user_id: {})", self.user_id)
    }
}

impl Display for AwaitingPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "AwaitingPassword(user_id: {}, attempts: {})",
            self.user_id, self.attempts
        )
    }
}

impl Display for AwaitingPin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "AwaitingPin(user_id: {}, attempts: {})",
            self.user_id, self.attempts
        )
    }
}

impl Display for Authenticated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Authenticated(user: {}, device: {})",
            self.user.email.as_deref().unwrap_or("no email"),
            self.device_id
        )
    }
}

impl Display for SettingUpPin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SettingUpPin(user: {})",
            self.user.email.as_deref().unwrap_or("no email")
        )
    }
}

impl Display for Refreshing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Refreshing(user: {})",
            self.previous_state
                .user
                .email
                .as_deref()
                .unwrap_or("no email")
        )
    }
}

/// State serialization for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SerializedAuthState {
    Unauthenticated,
    UserSelected {
        user_id: Uuid,
    },
    AwaitingPassword {
        user_id: Uuid,
        device_id: Uuid,
        attempts: u8,
    },
    AwaitingPin {
        user_id: Uuid,
        device_id: Uuid,
        attempts: u8,
    },
    Authenticated {
        user_id: Uuid,
        device_id: Uuid,
        token_hash: String,
    },
    SettingUpPin {
        user_id: Uuid,
        device_id: Uuid,
    },
    Refreshing {
        user_id: Uuid,
        device_id: Uuid,
    },
}

// Helper methods for all state machines
impl<S: AuthState, const MAX_ATTEMPTS: u8, const TIMEOUT_SECS: u64>
    AuthStateMachine<S, MAX_ATTEMPTS, TIMEOUT_SECS>
{
    /// Get the current state data
    pub fn state(&self) -> &S {
        &self.state_data
    }

    /// Check if the state machine is in a specific state type
    pub fn is<T: AuthState>(&self) -> bool {
        std::any::TypeId::of::<S>() == std::any::TypeId::of::<T>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_password_flow() {
        // Start unauthenticated
        let machine = AuthStateMachine::<Unauthenticated, 3, 300>::new();

        // Select user
        let user_id = Uuid::now_v7();
        let machine = machine.select_user(user_id).unwrap();

        // Require password
        let device_id = Uuid::now_v7();
        let machine = machine.require_password(device_id).unwrap();

        // Verify password
        let user = User {
            id: user_id,
            avatar_url: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            username: "testuser".to_string(),
            last_login: Some(chrono::Utc::now()),
            is_active: true,
            preferences: crate::UserPreferences::default(),
            display_name: "Test User".to_string(),
            email: Some("test@example.com".to_string()),
        };

        let token = AuthToken {
            access_token: "<REDACTED>".to_string(),
            expires_in: 3600,
            refresh_token: "refresh_token".to_string(),
        };

        let permissions = UserPermissions::default();
        let server_url = "http://localhost:8080".to_string();

        let machine = machine
            .verify_password(user, token, permissions, server_url)
            .unwrap();

        // Verify we're authenticated
        assert!(machine.is::<Authenticated>());
    }

    #[test]
    fn test_invalid_transitions_dont_compile() {
        // This test demonstrates that invalid transitions won't compile
        // Uncomment any of these to see compilation errors:

        // let machine = AuthStateMachine::<Unauthenticated, 3, 300>::new();
        // machine.verify_password(...); // Error: no such method
        // machine.start_refresh(); // Error: no such method

        // let machine = AuthStateMachine::<Authenticated, 3, 300>::new();
        // machine.select_user(...); // Error: no such method
    }
}
