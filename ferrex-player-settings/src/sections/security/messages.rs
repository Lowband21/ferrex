//! Security section messages

/// Messages for the security settings section
#[derive(Debug, Clone)]
pub enum SecurityMessage {
    // Password subsection
    /// Show password change form
    ShowChangePassword,
    /// Update current password field
    UpdatePasswordCurrent(String),
    /// Update new password field
    UpdatePasswordNew(String),
    /// Update confirm password field
    UpdatePasswordConfirm(String),
    /// Toggle password visibility
    TogglePasswordVisibility,
    /// Submit password change
    SubmitPasswordChange,
    /// Handle password change result
    PasswordChangeResult(Result<(), String>),
    /// Cancel password change
    CancelPasswordChange,

    // PIN subsection
    /// Check if user has PIN set
    CheckUserHasPin,
    /// Result of PIN check
    UserHasPinResult(bool),
    /// Show set PIN form (new PIN)
    ShowSetPin,
    /// Show change PIN form (existing PIN)
    ShowChangePin,
    /// Update current PIN field
    UpdatePinCurrent(String),
    /// Update new PIN field
    UpdatePinNew(String),
    /// Update confirm PIN field
    UpdatePinConfirm(String),
    /// Submit PIN change
    SubmitPinChange,
    /// Handle PIN change result
    PinChangeResult(Result<(), String>),
    /// Cancel PIN change
    CancelPinChange,
}

impl SecurityMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::ShowChangePassword => "Security::ShowChangePassword",
            Self::UpdatePasswordCurrent(_) => "Security::UpdatePasswordCurrent",
            Self::UpdatePasswordNew(_) => "Security::UpdatePasswordNew",
            Self::UpdatePasswordConfirm(_) => "Security::UpdatePasswordConfirm",
            Self::TogglePasswordVisibility => {
                "Security::TogglePasswordVisibility"
            }
            Self::SubmitPasswordChange => "Security::SubmitPasswordChange",
            Self::PasswordChangeResult(_) => "Security::PasswordChangeResult",
            Self::CancelPasswordChange => "Security::CancelPasswordChange",
            Self::CheckUserHasPin => "Security::CheckUserHasPin",
            Self::UserHasPinResult(_) => "Security::UserHasPinResult",
            Self::ShowSetPin => "Security::ShowSetPin",
            Self::ShowChangePin => "Security::ShowChangePin",
            Self::UpdatePinCurrent(_) => "Security::UpdatePinCurrent",
            Self::UpdatePinNew(_) => "Security::UpdatePinNew",
            Self::UpdatePinConfirm(_) => "Security::UpdatePinConfirm",
            Self::SubmitPinChange => "Security::SubmitPinChange",
            Self::PinChangeResult(_) => "Security::PinChangeResult",
            Self::CancelPinChange => "Security::CancelPinChange",
        }
    }
}
