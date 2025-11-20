use ferrex_core::{rbac::UserPermissions, user::User};
use uuid::Uuid;

use crate::domains::auth::manager::{DeviceAuthStatus, PlayerAuthResult};
use crate::domains::auth::dto::UserListItemDto;

pub mod commands;
pub mod subscriptions;

pub use commands::{AuthCommand, AuthCommandResult};

/// Setup field for first-run admin setup
#[derive(Clone, Debug)]
pub enum SetupField {
    Username(String),
    Password(String),
    ConfirmPassword(String),
    DisplayName(String),
    SetupToken(String),
}

#[derive(Clone, Debug)]
pub enum Message {
    // Core auth flow
    CheckAuthStatus,
    AuthStatusConfirmedWithPin,
    CheckSetupStatus,
    SetupStatusChecked(bool), // needs_setup
    AutoLoginCheckComplete,
    AutoLoginSuccessful(User),

    // User management
    LoadUsers,
    UsersLoaded(Result<Vec<UserListItemDto>, String>),
    SelectUser(Uuid),
    ShowCreateUser,
    BackToUserSelection,

    // PIN authentication
    ShowPinEntry(User),
    PinDigitPressed(char),
    PinBackspace,
    PinClear,
    PinSubmit,

    // Login results
    LoginSuccess(User, UserPermissions),
    LoginError(String),
    WatchStatusLoaded(Result<ferrex_core::watch_status::UserWatchState, String>),
    Logout,
    LogoutComplete,

    // Password login
    ShowPasswordLogin(String), // username
    PasswordLoginUpdateUsername(String),
    PasswordLoginUpdatePassword(String),
    PasswordLoginToggleVisibility,
    PasswordLoginToggleRemember,
    PasswordLoginSubmit,

    // Device auth flow
    DeviceStatusChecked(User, Result<DeviceAuthStatus, String>),
    UpdateCredential(String),
    SubmitCredential,
    TogglePasswordVisibility,
    ToggleRememberDevice,
    AuthResult(Result<PlayerAuthResult, String>),
    SetupPin,
    UpdatePin(String),
    UpdateConfirmPin(String),
    SubmitPin,
    PinSet(Result<(), String>),
    Retry,
    Back,

    // First-run setup
    FirstRunUpdateUsername(String),
    FirstRunUpdateDisplayName(String),
    FirstRunUpdatePassword(String),
    FirstRunUpdateConfirmPassword(String),
    FirstRunTogglePasswordVisibility,
    FirstRunSubmit,
    FirstRunSuccess,
    FirstRunError(String),

    // Admin setup flow  
    UpdateSetupField(SetupField),
    ToggleSetupPasswordVisibility,
    SubmitSetup,
    SetupComplete(String, String), // access_token, refresh_token
    SetupError(String),

    // Admin PIN unlock management
    EnableAdminPinUnlock,
    DisableAdminPinUnlock,
    AdminPinUnlockToggled(Result<bool, String>), // enabled, error
    
    // Command execution
    ExecuteCommand(AuthCommand),
    CommandResult(AuthCommand, AuthCommandResult),
}

impl Message {
    /// Returns a sanitized display string that hides sensitive credential data
    pub fn sanitized_display(&self) -> String {
        match self {
            // Sensitive credential messages - hide the actual values
            Self::UpdateCredential(_) => "UpdateCredential(***)".to_string(),
            Self::UpdatePin(_) => "UpdatePin(***)".to_string(),
            Self::UpdateConfirmPin(_) => "UpdateConfirmPin(***)".to_string(),
            Self::PasswordLoginUpdatePassword(_) => "PasswordLoginUpdatePassword(***)".to_string(),
            Self::FirstRunUpdatePassword(_) => "FirstRunUpdatePassword(***)".to_string(),
            Self::FirstRunUpdateConfirmPassword(_) => "FirstRunUpdateConfirmPassword(***)".to_string(),
            Self::UpdateSetupField(SetupField::Password(_)) => "UpdateSetupField(Password(***)".to_string(),
            Self::UpdateSetupField(SetupField::ConfirmPassword(_)) => "UpdateSetupField(ConfirmPassword(***)".to_string(),
            
            // Non-sensitive messages - show full debug representation
            _ => format!("{:?}", self),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            // Core auth flow
            Self::CheckAuthStatus => "Auth::CheckAuthStatus",
            Self::AuthStatusConfirmedWithPin => "Auth::AuthStatusConfirmedWithPin",
            Self::CheckSetupStatus => "Auth::CheckSetupStatus",
            Self::SetupStatusChecked(_) => "Auth::SetupStatusChecked",
            Self::AutoLoginCheckComplete => "Auth::AutoLoginCheckComplete",
            Self::AutoLoginSuccessful(_) => "Auth::AutoLoginSuccessful",

            // User management
            Self::LoadUsers => "Auth::LoadUsers",
            Self::UsersLoaded(_) => "Auth::UsersLoaded",
            Self::SelectUser(_) => "Auth::SelectUser",
            Self::ShowCreateUser => "Auth::ShowCreateUser",
            Self::BackToUserSelection => "Auth::BackToUserSelection",

            // PIN authentication
            Self::ShowPinEntry(_) => "Auth::ShowPinEntry",
            Self::PinDigitPressed(_) => "Auth::PinDigitPressed",
            Self::PinBackspace => "Auth::PinBackspace",
            Self::PinClear => "Auth::PinClear",
            Self::PinSubmit => "Auth::PinSubmit",

            // Login results
            Self::LoginSuccess(_, _) => "Auth::LoginSuccess",
            Self::LoginError(_) => "Auth::LoginError",
            Self::WatchStatusLoaded(_) => "Auth::WatchStatusLoaded",
            Self::Logout => "Auth::Logout",
            Self::LogoutComplete => "Auth::LogoutComplete",

            // Password login
            Self::ShowPasswordLogin(_) => "Auth::ShowPasswordLogin",
            Self::PasswordLoginUpdateUsername(_) => "Auth::PasswordLoginUpdateUsername",
            Self::PasswordLoginUpdatePassword(_) => "Auth::PasswordLoginUpdatePassword",
            Self::PasswordLoginToggleVisibility => "Auth::PasswordLoginToggleVisibility",
            Self::PasswordLoginToggleRemember => "Auth::PasswordLoginToggleRemember",
            Self::PasswordLoginSubmit => "Auth::PasswordLoginSubmit",

            // Device auth flow
            Self::DeviceStatusChecked(_, _) => "Auth::DeviceStatusChecked",
            Self::UpdateCredential(_) => "Auth::UpdateCredential",
            Self::SubmitCredential => "Auth::SubmitCredential",
            Self::TogglePasswordVisibility => "Auth::TogglePasswordVisibility",
            Self::ToggleRememberDevice => "Auth::ToggleRememberDevice",
            Self::AuthResult(_) => "Auth::AuthResult",
            Self::SetupPin => "Auth::SetupPin",
            Self::UpdatePin(_) => "Auth::UpdatePin",
            Self::UpdateConfirmPin(_) => "Auth::UpdateConfirmPin",
            Self::SubmitPin => "Auth::SubmitPin",
            Self::PinSet(_) => "Auth::PinSet",
            Self::Retry => "Auth::Retry",
            Self::Back => "Auth::Back",

            // First-run setup
            Self::FirstRunUpdateUsername(_) => "Auth::FirstRunUpdateUsername",
            Self::FirstRunUpdateDisplayName(_) => "Auth::FirstRunUpdateDisplayName",
            Self::FirstRunUpdatePassword(_) => "Auth::FirstRunUpdatePassword",
            Self::FirstRunUpdateConfirmPassword(_) => "Auth::FirstRunUpdateConfirmPassword",
            Self::FirstRunTogglePasswordVisibility => "Auth::FirstRunTogglePasswordVisibility",
            Self::FirstRunSubmit => "Auth::FirstRunSubmit",
            Self::FirstRunSuccess => "Auth::FirstRunSuccess",
            Self::FirstRunError(_) => "Auth::FirstRunError",

            // Admin setup flow
            Self::UpdateSetupField(_) => "Auth::UpdateSetupField",
            Self::ToggleSetupPasswordVisibility => "Auth::ToggleSetupPasswordVisibility",
            Self::SubmitSetup => "Auth::SubmitSetup",
            Self::SetupComplete(_, _) => "Auth::SetupComplete",
            Self::SetupError(_) => "Auth::SetupError",

            // Admin PIN unlock
            Self::EnableAdminPinUnlock => "Auth::EnableAdminPinUnlock",
            Self::DisableAdminPinUnlock => "Auth::DisableAdminPinUnlock",
            Self::AdminPinUnlockToggled(_) => "Auth::AdminPinUnlockToggled",
            
            // Command execution
            Self::ExecuteCommand(_) => "Auth::ExecuteCommand",
            Self::CommandResult(_, _) => "Auth::CommandResult",
        }
    }
}

/// Cross-domain events that auth domain can emit
#[derive(Clone, Debug)]
pub enum AuthEvent {
    UserAuthenticated(User, UserPermissions),
    UserLoggedOut,
    WatchStatusUpdated(ferrex_core::watch_status::UserWatchState),
    RequireSetup,
}
