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

/// Setup wizard step in first-run flow
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SetupStep {
    #[default]
    Welcome, // Brief intro
    Account,     // Username, display name, password, confirm (combined)
    SetupToken,  // If required by server (conditional)
    DeviceClaim, // Secure claim verification (show code, wait for server confirmation)
    Pin,         // Optional 4-digit PIN
    Complete,    // Success message
}

impl SetupStep {
    /// Get the next step in the wizard flow
    pub fn next(&self, setup_token_required: bool) -> Option<Self> {
        match self {
            Self::Welcome => Some(Self::Account),
            Self::Account => {
                if setup_token_required {
                    Some(Self::SetupToken)
                } else {
                    Some(Self::DeviceClaim)
                }
            }
            Self::SetupToken => Some(Self::DeviceClaim),
            Self::DeviceClaim => Some(Self::Pin),
            Self::Pin => Some(Self::Complete),
            Self::Complete => None,
        }
    }

    /// Get the previous step in the wizard flow
    pub fn previous(&self, setup_token_required: bool) -> Option<Self> {
        match self {
            Self::Welcome => None,
            Self::Account => Some(Self::Welcome),
            Self::SetupToken => Some(Self::Account),
            Self::DeviceClaim => {
                if setup_token_required {
                    Some(Self::SetupToken)
                } else {
                    Some(Self::Account)
                }
            }
            Self::Pin => Some(Self::DeviceClaim),
            Self::Complete => None, // Can't go back after completion
        }
    }

    /// Get zero-based index for progress indicator
    pub fn index(&self, setup_token_required: bool) -> usize {
        match self {
            Self::Welcome => 0,
            Self::Account => 1,
            Self::SetupToken => 2,
            Self::DeviceClaim => {
                if setup_token_required {
                    3
                } else {
                    2
                }
            }
            Self::Pin => {
                if setup_token_required {
                    4
                } else {
                    3
                }
            }
            Self::Complete => {
                if setup_token_required {
                    5
                } else {
                    4
                }
            }
        }
    }

    /// Total number of steps (varies based on setup token requirement)
    pub fn total_steps(setup_token_required: bool) -> usize {
        if setup_token_required {
            6 // Welcome, Account, SetupToken, DeviceClaim, Pin, Complete
        } else {
            5 // Welcome, Account, DeviceClaim, Pin, Complete
        }
    }

    /// Display label for progress indicator
    pub fn label(&self) -> &'static str {
        match self {
            Self::Welcome => "Welcome",
            Self::Account => "Account",
            Self::SetupToken => "Token",
            Self::DeviceClaim => "Verify",
            Self::Pin => "PIN",
            Self::Complete => "Complete",
        }
    }
}

/// Direction of carousel transition animation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransitionDirection {
    #[default]
    None,
    Forward,  // Sliding left (next step)
    Backward, // Sliding right (previous step)
}

/// Authentication flow state
#[derive(Debug, Clone, Default)]
pub enum AuthenticationFlow {
    /// Initial state, checking if server needs setup
    #[default]
    CheckingSetup,

    /// First-run admin setup wizard
    FirstRunSetup {
        // Wizard step tracking
        current_step: SetupStep,

        // Account fields
        username: String,
        password: SecureCredential,
        confirm_password: SecureCredential,
        display_name: String,
        setup_token: String,
        show_password: bool,

        // Device claim fields
        claim_code: Option<String>,
        claim_token: Option<String>,
        claim_status: SetupClaimStatus,
        claim_loading: bool,

        // PIN fields (embedded in wizard)
        pin: SecureCredential,
        confirm_pin: SecureCredential,

        // State
        error: Option<String>,
        loading: bool,
        setup_token_required: bool,

        // Animation
        transition_direction: TransitionDirection,
        transition_progress: f32,
    },

    /// Checking for cached auth and auto-login
    CheckingAutoLogin,

    /// Loading users from server
    LoadingUsers,

    /// Pre-auth login screen collecting username/password when no user list is available
    PreAuthLogin {
        username: String,
        password:
            crate::domains::auth::security::secure_credential::SecureCredential,
        show_password: bool,
        remember_device: bool,
        error: Option<String>,
        loading: bool,
    },

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
