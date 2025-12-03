//! Profile section messages
//!
//! All messages related to profile settings. These are routed through
//! the main SettingsMessage::Profile variant.

/// Messages for the profile settings section
#[derive(Debug, Clone)]
pub enum ProfileMessage {
    // Account subsection
    /// Update display name field
    UpdateDisplayName(String),
    /// Update email field
    UpdateEmail(String),
    /// Update avatar (future)
    UpdateAvatar(String),

    // Actions
    /// Submit profile changes to server
    SubmitChanges,
    /// Result of profile change submission
    ChangeResult(Result<(), String>),
    /// Cancel and revert changes
    Cancel,

    // Authentication actions
    /// Trigger logout
    Logout,
    /// Switch to different user (for multi-user households)
    SwitchUser,
}

impl ProfileMessage {
    /// Get a static name for logging/debugging
    pub fn name(&self) -> &'static str {
        match self {
            Self::UpdateDisplayName(_) => "Profile::UpdateDisplayName",
            Self::UpdateEmail(_) => "Profile::UpdateEmail",
            Self::UpdateAvatar(_) => "Profile::UpdateAvatar",
            Self::SubmitChanges => "Profile::SubmitChanges",
            Self::ChangeResult(_) => "Profile::ChangeResult",
            Self::Cancel => "Profile::Cancel",
            Self::Logout => "Profile::Logout",
            Self::SwitchUser => "Profile::SwitchUser",
        }
    }
}
