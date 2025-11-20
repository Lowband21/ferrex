use crate::domains::auth::security::secure_credential::SecureCredential;

/// Commands that can be sent to the auth domain to modify authentication state
/// This pattern breaks the circular dependency between Auth and Settings domains
#[derive(Clone, Debug)]
pub enum AuthCommand {
    /// Change user password
    ChangePassword {
        old_password: SecureCredential,
        new_password: SecureCredential,
    },

    /// Set a new PIN for the current user
    SetUserPin { pin: SecureCredential },

    /// Remove PIN from current user (requires password authentication)
    RemoveUserPin,

    /// Enable admin PIN unlock feature (requires admin permissions)
    EnableAdminPinUnlock,

    /// Change existing user PIN
    ChangeUserPin {
        current_pin: SecureCredential,
        new_pin: SecureCredential,
    },
}

impl AuthCommand {
    /// Returns a sanitized display string that hides sensitive credential data
    pub fn sanitized_display(&self) -> String {
        match self {
            Self::ChangePassword { .. } => {
                "AuthCommand::ChangePassword { old_password: ***, new_password: *** }".to_string()
            }
            Self::SetUserPin { .. } => "AuthCommand::SetUserPin { pin: *** }".to_string(),
            Self::RemoveUserPin => "AuthCommand::RemoveUserPin".to_string(),
            Self::EnableAdminPinUnlock => "AuthCommand::EnableAdminPinUnlock".to_string(),
            Self::ChangeUserPin { .. } => {
                "AuthCommand::ChangeUserPin { current_pin: ***, new_pin: *** }".to_string()
            }
        }
    }

    /// Get command name for logging and debugging
    pub fn name(&self) -> &'static str {
        match self {
            Self::ChangePassword { .. } => "AuthCommand::ChangePassword",
            Self::SetUserPin { .. } => "AuthCommand::SetUserPin",
            Self::RemoveUserPin => "AuthCommand::RemoveUserPin",
            Self::EnableAdminPinUnlock => "AuthCommand::EnableAdminPinUnlock",
            Self::ChangeUserPin { .. } => "AuthCommand::ChangeUserPin",
        }
    }
}

/// Result of executing an auth command
#[derive(Clone, Debug)]
pub enum AuthCommandResult {
    /// Command executed successfully
    Success,
    /// Command failed with error message
    Error(String),
}

impl AuthCommandResult {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success)
    }

    pub fn error_message(&self) -> Option<&str> {
        match self {
            Self::Error(msg) => Some(msg),
            Self::Success => None,
        }
    }
}

impl From<Result<(), String>> for AuthCommandResult {
    fn from(result: Result<(), String>) -> Self {
        match result {
            Ok(()) => Self::Success,
            Err(error) => Self::Error(error),
        }
    }
}

impl From<anyhow::Result<()>> for AuthCommandResult {
    fn from(result: anyhow::Result<()>) -> Self {
        match result {
            Ok(()) => Self::Success,
            Err(error) => Self::Error(error.to_string()),
        }
    }
}
