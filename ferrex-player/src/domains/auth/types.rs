//! Authentication domain types

use crate::domains::auth::dto::UserListItemDto;
use crate::domains::auth::security::secure_credential::SecureCredential;
use crate::domains::ui::views::first_run::FirstRunState;
use ferrex_core::user::User;

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
#[derive(Debug, Clone)]
pub enum AuthenticationFlow {
    /// Initial state, checking if server needs setup
    CheckingSetup,

    /// First-run admin setup
    FirstRunSetup {
        username: String,
        password: SecureCredential,
        confirm_password: SecureCredential,
        display_name: String,
        setup_token: String,
        show_password: bool,
        error: Option<String>,
        loading: bool,
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

impl Default for AuthenticationFlow {
    fn default() -> Self {
        AuthenticationFlow::CheckingSetup
    }
}

/// Legacy compatibility - will be removed
pub type AuthViewState = AuthenticationFlow;
