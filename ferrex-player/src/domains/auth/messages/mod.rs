use ferrex_core::{
    api::types::setup::{ConfirmClaimResponse, StartClaimResponse},
    player_prelude::{User, UserPermissions, UserWatchState},
};
use uuid::Uuid;

use crate::domains::auth::dto::UserListItemDto;
use crate::domains::auth::manager::{DeviceAuthStatus, PlayerAuthResult};
use crate::infra::api_client::SetupStatus as ApiSetupStatus;

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
    ClaimToken(String),
}

#[derive(Clone)]
pub enum AuthMessage {
    // Core auth flow
    CheckAuthStatus,
    AuthStatusConfirmedWithPin,
    CheckSetupStatus,
    SetupStatusChecked(ApiSetupStatus),
    AutoLoginCheckComplete,
    AutoLoginSuccessful(User),

    // User management
    LoadUsers,
    UsersLoaded(Result<Vec<UserListItemDto>, String>),
    SelectUser(Uuid),
    ShowCreateUser,
    BackToUserSelection,

    // Pre-auth login
    PreAuthUpdateUsername(String),
    PreAuthTogglePasswordVisibility,
    PreAuthToggleRememberDevice,
    PreAuthSubmit,

    // Login results
    LoginSuccess(User, UserPermissions),
    WatchStatusLoaded(Result<UserWatchState, String>),
    Logout,
    LogoutComplete,

    // Device auth flow
    DeviceStatusChecked(User, Result<DeviceAuthStatus, String>),
    UpdateCredential(String),
    SubmitCredential,
    TogglePasswordVisibility,
    ToggleRememberDevice,
    RememberDeviceSynced(bool),
    AuthResult(Result<PlayerAuthResult, String>),
    SetupPin,
    UpdatePin(String),
    UpdateConfirmPin(String),
    SubmitPin,
    PinSet(Result<(), String>),
    Retry,
    Back,

    // Admin setup flow
    UpdateSetupField(SetupField),
    ToggleSetupPasswordVisibility,
    SubmitSetup,
    SetupComplete(String, String), // access_token, refresh_token
    SetupError(String),

    // Setup wizard navigation
    SetupNextStep,
    SetupPreviousStep,
    SkipPinSetup,
    SetupAnimationTick(f32), // delta for transition animation

    // Device claim flow (secure setup verification)
    StartSetupClaim,
    ClaimStarted(Result<StartClaimResponse, String>),
    ConfirmSetupClaim,
    ClaimConfirmed(Result<ConfirmClaimResponse, String>),

    // Admin PIN unlock management
    EnableAdminPinUnlock,
    DisableAdminPinUnlock,
    AdminPinUnlockToggled(Result<bool, String>), // enabled, error

    // Command execution
    ExecuteCommand(AuthCommand),
    CommandResult(AuthCommand, AuthCommandResult),
}

impl std::fmt::Debug for AuthMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Core auth flow
            Self::CheckAuthStatus => write!(f, "CheckAuthStatus"),
            Self::AuthStatusConfirmedWithPin => {
                write!(f, "AuthStatusConfirmedWithPin")
            }
            Self::CheckSetupStatus => write!(f, "CheckSetupStatus"),
            Self::SetupStatusChecked(status) => {
                write!(
                    f,
                    "SetupStatusChecked(needs_setup={}, requires_token={})",
                    status.needs_setup, status.requires_setup_token
                )
            }
            Self::AutoLoginCheckComplete => write!(f, "AutoLoginCheckComplete"),
            Self::AutoLoginSuccessful(_) => {
                write!(f, "AutoLoginSuccessful(...)")
            }

            // User management
            Self::LoadUsers => write!(f, "LoadUsers"),
            Self::UsersLoaded(result) => {
                write!(f, "UsersLoaded({:?})", result.as_ref().map(|v| v.len()))
            }
            Self::SelectUser(uuid) => write!(f, "SelectUser({})", uuid),
            Self::ShowCreateUser => write!(f, "ShowCreateUser"),
            Self::BackToUserSelection => write!(f, "BackToUserSelection"),

            // Pre-auth login messages
            Self::PreAuthUpdateUsername(_) => {
                write!(f, "PreAuthUpdateUsername(***)")
            }
            Self::PreAuthTogglePasswordVisibility => {
                write!(f, "PreAuthTogglePasswordVisibility")
            }
            Self::PreAuthToggleRememberDevice => {
                write!(f, "PreAuthToggleRememberDevice")
            }
            Self::PreAuthSubmit => write!(f, "PreAuthSubmit"),

            // Login results
            Self::LoginSuccess(_, _) => write!(f, "LoginSuccess(...)"),
            Self::WatchStatusLoaded(_) => write!(f, "WatchStatusLoaded(...)"),
            Self::Logout => write!(f, "Logout"),
            Self::LogoutComplete => write!(f, "LogoutComplete"),

            // Device auth flow - hide sensitive data
            Self::DeviceStatusChecked(_, _) => {
                write!(f, "DeviceStatusChecked(...)")
            }
            Self::UpdateCredential(_) => write!(f, "UpdateCredential(***)"),
            Self::SubmitCredential => write!(f, "SubmitCredential"),
            Self::TogglePasswordVisibility => {
                write!(f, "TogglePasswordVisibility")
            }
            Self::ToggleRememberDevice => write!(f, "ToggleRememberDevice"),
            Self::RememberDeviceSynced(value) => {
                write!(f, "RememberDeviceSynced({})", value)
            }
            Self::AuthResult(_) => write!(f, "AuthResult(...)"),
            Self::SetupPin => write!(f, "SetupPin"),
            Self::UpdatePin(_) => write!(f, "UpdatePin(***)"),
            Self::UpdateConfirmPin(_) => write!(f, "UpdateConfirmPin(***)"),
            Self::SubmitPin => write!(f, "SubmitPin"),
            Self::PinSet(result) => write!(f, "PinSet({:?})", result),
            Self::Retry => write!(f, "Retry"),
            Self::Back => write!(f, "Back"),

            // Admin setup flow - hide sensitive data
            Self::UpdateSetupField(field) => match field {
                SetupField::Username(u) => {
                    write!(f, "UpdateSetupField(Username({}))", u)
                }
                SetupField::DisplayName(d) => {
                    write!(f, "UpdateSetupField(DisplayName({}))", d)
                }
                SetupField::Password(_) => {
                    write!(f, "UpdateSetupField(Password(***)")
                }
                SetupField::ConfirmPassword(_) => {
                    write!(f, "UpdateSetupField(ConfirmPassword(***)")
                }
                SetupField::SetupToken(t) => {
                    write!(f, "UpdateSetupField(SetupToken({}))", t)
                }
                SetupField::ClaimToken(_) => {
                    write!(f, "UpdateSetupField(ClaimToken(***)")
                }
            },
            Self::ToggleSetupPasswordVisibility => {
                write!(f, "ToggleSetupPasswordVisibility")
            }
            Self::SubmitSetup => write!(f, "SubmitSetup"),
            Self::SetupComplete(_, _) => write!(f, "SetupComplete(***, ***)"),
            Self::SetupError(error) => write!(f, "SetupError({})", error),

            // Setup wizard navigation
            Self::SetupNextStep => write!(f, "SetupNextStep"),
            Self::SetupPreviousStep => write!(f, "SetupPreviousStep"),
            Self::SkipPinSetup => write!(f, "SkipPinSetup"),
            Self::SetupAnimationTick(delta) => {
                write!(f, "SetupAnimationTick({:.2})", delta)
            }

            // Device claim flow
            Self::StartSetupClaim => write!(f, "StartSetupClaim"),
            Self::ClaimStarted(result) => {
                write!(f, "ClaimStarted({:?})", result.is_ok())
            }
            Self::ConfirmSetupClaim => write!(f, "ConfirmSetupClaim"),
            Self::ClaimConfirmed(result) => {
                write!(f, "ClaimConfirmed({:?})", result.is_ok())
            }

            // Admin PIN unlock management
            Self::EnableAdminPinUnlock => write!(f, "EnableAdminPinUnlock"),
            Self::DisableAdminPinUnlock => write!(f, "DisableAdminPinUnlock"),
            Self::AdminPinUnlockToggled(result) => {
                write!(f, "AdminPinUnlockToggled({:?})", result)
            }

            // Command execution
            Self::ExecuteCommand(cmd) => write!(f, "ExecuteCommand({:?})", cmd),
            Self::CommandResult(cmd, result) => {
                write!(f, "CommandResult({:?}, {:?})", cmd, result)
            }
        }
    }
}

impl AuthMessage {
    /// Returns a sanitized display string that hides sensitive credential data
    pub fn sanitized_display(&self) -> String {
        match self {
            // Sensitive credential messages - hide the actual values
            Self::UpdateCredential(_) => "UpdateCredential(***)".to_string(),
            Self::UpdatePin(_) => "UpdatePin(***)".to_string(),
            Self::UpdateConfirmPin(_) => "UpdateConfirmPin(***)".to_string(),
            Self::UpdateSetupField(SetupField::Password(_)) => {
                "UpdateSetupField(Password(***)".to_string()
            }
            Self::UpdateSetupField(SetupField::ConfirmPassword(_)) => {
                "UpdateSetupField(ConfirmPassword(***)".to_string()
            }
            Self::UpdateSetupField(SetupField::ClaimToken(_)) => {
                "UpdateSetupField(ClaimToken(***)".to_string()
            }

            // Non-sensitive messages - show full debug representation
            _ => format!("{:?}", self),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            // Core auth flow
            Self::CheckAuthStatus => "Auth::CheckAuthStatus",
            Self::AuthStatusConfirmedWithPin => {
                "Auth::AuthStatusConfirmedWithPin"
            }
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
            Self::PreAuthUpdateUsername(_) => "Auth::PreAuthUpdateUsername",
            Self::PreAuthTogglePasswordVisibility => {
                "Auth::PreAuthTogglePasswordVisibility"
            }
            Self::PreAuthToggleRememberDevice => {
                "Auth::PreAuthToggleRememberDevice"
            }
            Self::PreAuthSubmit => "Auth::PreAuthSubmit",

            // Login results
            Self::LoginSuccess(_, _) => "Auth::LoginSuccess",
            Self::WatchStatusLoaded(_) => "Auth::WatchStatusLoaded",
            Self::Logout => "Auth::Logout",
            Self::LogoutComplete => "Auth::LogoutComplete",

            // Device auth flow
            Self::DeviceStatusChecked(_, _) => "Auth::DeviceStatusChecked",
            Self::UpdateCredential(_) => "Auth::UpdateCredential",
            Self::SubmitCredential => "Auth::SubmitCredential",
            Self::TogglePasswordVisibility => "Auth::TogglePasswordVisibility",
            Self::ToggleRememberDevice => "Auth::ToggleRememberDevice",
            Self::RememberDeviceSynced(_) => "Auth::RememberDeviceSynced",
            Self::AuthResult(_) => "Auth::AuthResult",
            Self::SetupPin => "Auth::SetupPin",
            Self::UpdatePin(_) => "Auth::UpdatePin",
            Self::UpdateConfirmPin(_) => "Auth::UpdateConfirmPin",
            Self::SubmitPin => "Auth::SubmitPin",
            Self::PinSet(_) => "Auth::PinSet",
            Self::Retry => "Auth::Retry",
            Self::Back => "Auth::Back",

            // Admin setup flow
            Self::UpdateSetupField(_) => "Auth::UpdateSetupField",
            Self::ToggleSetupPasswordVisibility => {
                "Auth::ToggleSetupPasswordVisibility"
            }
            Self::SubmitSetup => "Auth::SubmitSetup",
            Self::SetupComplete(_, _) => "Auth::SetupComplete",
            Self::SetupError(_) => "Auth::SetupError",

            // Setup wizard navigation
            Self::SetupNextStep => "Auth::SetupNextStep",
            Self::SetupPreviousStep => "Auth::SetupPreviousStep",
            Self::SkipPinSetup => "Auth::SkipPinSetup",
            Self::SetupAnimationTick(_) => "Auth::SetupAnimationTick",

            // Device claim flow
            Self::StartSetupClaim => "Auth::StartSetupClaim",
            Self::ClaimStarted(_) => "Auth::ClaimStarted",
            Self::ConfirmSetupClaim => "Auth::ConfirmSetupClaim",
            Self::ClaimConfirmed(_) => "Auth::ClaimConfirmed",

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
    UserAuthenticated(Box<User>, UserPermissions),
    UserLoggedOut,
    WatchStatusUpdated(Box<UserWatchState>),
    RequireSetup,
}
