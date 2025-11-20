use crate::security::SecureCredential;

/// Commands that can be sent to the auth domain to modify authentication state
/// This pattern breaks the circular dependency between Auth and Settings domains
#[derive(Clone, Debug)]
pub enum AuthCommand {
    /// Change user password
    ChangePassword {
        old_password: SecureCredential,
        new_password: SecureCredential,
    },
    
    /// Set a new PIN for the current device
    SetDevicePin {
        pin: SecureCredential,
    },
    
    /// Remove PIN from current device (requires password authentication)
    RemoveDevicePin,
    
    /// Enable admin PIN unlock feature (requires admin permissions)
    EnableAdminPinUnlock,
    
    /// Change existing device PIN
    ChangeDevicePin {
        current_pin: SecureCredential,
        new_pin: SecureCredential,
    },
}

impl AuthCommand {
    /// Returns a sanitized display string that hides sensitive credential data
    pub fn sanitized_display(&self) -> String {
        match self {
            Self::ChangePassword { .. } => "AuthCommand::ChangePassword { old_password: ***, new_password: *** }".to_string(),
            Self::SetDevicePin { .. } => "AuthCommand::SetDevicePin { pin: *** }".to_string(),
            Self::RemoveDevicePin => "AuthCommand::RemoveDevicePin".to_string(),
            Self::EnableAdminPinUnlock => "AuthCommand::EnableAdminPinUnlock".to_string(),
            Self::ChangeDevicePin { .. } => "AuthCommand::ChangeDevicePin { current_pin: ***, new_pin: *** }".to_string(),
        }
    }

    /// Get command name for logging and debugging
    pub fn name(&self) -> &'static str {
        match self {
            Self::ChangePassword { .. } => "AuthCommand::ChangePassword",
            Self::SetDevicePin { .. } => "AuthCommand::SetDevicePin",
            Self::RemoveDevicePin => "AuthCommand::RemoveDevicePin",
            Self::EnableAdminPinUnlock => "AuthCommand::EnableAdminPinUnlock",
            Self::ChangeDevicePin { .. } => "AuthCommand::ChangeDevicePin",
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