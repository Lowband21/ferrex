//! Authentication domain types

use crate::domains::auth::dto::UserListItemDto;
use crate::domains::auth::security::secure_credential::SecureCredential;
use chrono::{DateTime, Utc};
use ferrex_core::player_prelude::User;
use uuid::Uuid;

/// Status of the secure setup claim workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupClaimStatus {
    Idle,
    Pending,
    Confirmed,
    Expired,
}

/// UI state for the secure claim wizard during first-run setup.
#[derive(Debug, Clone)]
pub struct SetupClaimUi {
    pub device_name: String,
    pub claim_id: Option<Uuid>,
    pub claim_code: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub claim_token: Option<String>,
    pub lan_only: bool,
    pub last_error: Option<String>,
    pub status: SetupClaimStatus,
    pub is_requesting: bool,
    pub is_confirming: bool,
}

impl Default for SetupClaimUi {
    fn default() -> Self {
        Self {
            device_name: String::new(),
            claim_id: None,
            claim_code: None,
            expires_at: None,
            claim_token: None,
            lan_only: true,
            last_error: None,
            status: SetupClaimStatus::Idle,
            is_requesting: false,
            is_confirming: false,
        }
    }
}

impl SetupClaimUi {
    /// Reset the claim state back to idle.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Check whether the active claim has expired.
    pub fn is_expired(&self) -> bool {
        if !matches!(self.status, SetupClaimStatus::Pending) {
            return false;
        }

        self.expires_at.is_some_and(|expiry| expiry <= Utc::now())
    }

    /// Mark the claim as expired.
    pub fn mark_expired(&mut self) {
        self.status = SetupClaimStatus::Expired;
    }
}

/// Authentication credential type
#[derive(Debug, Clone)]
pub enum CredentialType {
    Password,
    Pin { max_length: usize },
}

/// Authentication mode for offline support
#[derive(Debug, Clone)]
pub enum AuthenticationMode {
    Online,
    Cached,    // Offline with cached credentials
    Limited,   // Read-only mode when auth fails
    AutoLogin, // Automatic login with saved credentials
}

/// Authentication flow state
#[derive(Debug, Clone, Default)]
pub enum AuthenticationFlow {
    /// Initial state, checking if server needs setup
    #[default]
    CheckingSetup,

    /// First-run admin setup
    FirstRunSetup {
        username: String,
        password: SecureCredential,
        confirm_password: SecureCredential,
        display_name: String,
        setup_token: String,
        claim_token: String,
        show_password: bool,
        error: Option<String>,
        loading: bool,
        claim: SetupClaimUi,
    },

    /// Checking for cached auth and auto-login
    CheckingAutoLogin,

    /// Loading users from server
    LoadingUsers,

    /// User selection screen
    SelectingUser {
        users: Vec<UserListItemDto>,
        error: Option<String>,
    },

    /// Checking device status after user selection
    CheckingDevice { user: User },

    /// Credential input (unified for password/PIN)
    EnteringCredentials {
        user: User,
        input_type: CredentialType,
        input: SecureCredential,
        show_password: bool,
        remember_device: bool,
        error: Option<String>,
        attempts_remaining: Option<u8>,
        loading: bool,
    },

    /// Setting up PIN after first login
    SettingUpPin {
        user: User,
        pin: SecureCredential,
        confirm_pin: SecureCredential,
        error: Option<String>,
    },

    /// Successfully authenticated
    Authenticated {
        user: User,
        mode: AuthenticationMode,
    },
}

/// Legacy compatibility - will be removed
pub type AuthViewState = AuthenticationFlow;
