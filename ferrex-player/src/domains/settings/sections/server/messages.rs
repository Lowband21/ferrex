//! Server section messages (Admin)

use super::state::PasswordPolicy;

/// Messages for the server settings section
#[derive(Debug, Clone)]
pub enum ServerMessage {
    /// Load server settings from API
    LoadSettings,
    /// Settings loaded result
    SettingsLoaded(Result<super::state::ServerState, String>),
    /// Save all settings
    SaveSettings,
    /// Save result
    SaveResult(Result<(), String>),

    // Session Policies subsection
    /// Set access token lifetime
    SetSessionAccessTokenLifetime(u32),
    /// Set refresh token lifetime
    SetSessionRefreshTokenLifetime(u32),
    /// Set max concurrent sessions
    SetSessionMaxConcurrent(Option<u32>),

    // Device Policies subsection
    /// Set device trust duration
    SetDeviceTrustDuration(u32),
    /// Set max trusted devices
    SetDeviceMaxTrusted(Option<u32>),
    /// Set require PIN for new device
    SetDeviceRequirePinForNew(bool),

    // Password Policies subsection
    /// Set admin password policy
    SetAdminPasswordPolicy(PasswordPolicy),
    /// Set user password policy
    SetUserPasswordPolicy(PasswordPolicy),

    // Individual password policy field updates (admin)
    SetAdminPolicyEnforce(bool),
    SetAdminPolicyMinLength(u16),
    SetAdminPolicyRequireUppercase(bool),
    SetAdminPolicyRequireLowercase(bool),
    SetAdminPolicyRequireNumber(bool),
    SetAdminPolicyRequireSpecial(bool),

    // Individual password policy field updates (user)
    SetUserPolicyEnforce(bool),
    SetUserPolicyMinLength(u16),
    SetUserPolicyRequireUppercase(bool),
    SetUserPolicyRequireLowercase(bool),
    SetUserPolicyRequireNumber(bool),
    SetUserPolicyRequireSpecial(bool),

    // Curated Content subsection
    /// Set max carousel items
    SetCuratedMaxCarouselItems(usize),
    /// Set head window
    SetCuratedHeadWindow(usize),
}

impl ServerMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::LoadSettings => "Server::LoadSettings",
            Self::SettingsLoaded(_) => "Server::SettingsLoaded",
            Self::SaveSettings => "Server::SaveSettings",
            Self::SaveResult(_) => "Server::SaveResult",
            Self::SetSessionAccessTokenLifetime(_) => {
                "Server::SetSessionAccessTokenLifetime"
            }
            Self::SetSessionRefreshTokenLifetime(_) => {
                "Server::SetSessionRefreshTokenLifetime"
            }
            Self::SetSessionMaxConcurrent(_) => {
                "Server::SetSessionMaxConcurrent"
            }
            Self::SetDeviceTrustDuration(_) => "Server::SetDeviceTrustDuration",
            Self::SetDeviceMaxTrusted(_) => "Server::SetDeviceMaxTrusted",
            Self::SetDeviceRequirePinForNew(_) => {
                "Server::SetDeviceRequirePinForNew"
            }
            Self::SetAdminPasswordPolicy(_) => "Server::SetAdminPasswordPolicy",
            Self::SetUserPasswordPolicy(_) => "Server::SetUserPasswordPolicy",
            Self::SetAdminPolicyEnforce(_) => "Server::SetAdminPolicyEnforce",
            Self::SetAdminPolicyMinLength(_) => {
                "Server::SetAdminPolicyMinLength"
            }
            Self::SetAdminPolicyRequireUppercase(_) => {
                "Server::SetAdminPolicyRequireUppercase"
            }
            Self::SetAdminPolicyRequireLowercase(_) => {
                "Server::SetAdminPolicyRequireLowercase"
            }
            Self::SetAdminPolicyRequireNumber(_) => {
                "Server::SetAdminPolicyRequireNumber"
            }
            Self::SetAdminPolicyRequireSpecial(_) => {
                "Server::SetAdminPolicyRequireSpecial"
            }
            Self::SetUserPolicyEnforce(_) => "Server::SetUserPolicyEnforce",
            Self::SetUserPolicyMinLength(_) => "Server::SetUserPolicyMinLength",
            Self::SetUserPolicyRequireUppercase(_) => {
                "Server::SetUserPolicyRequireUppercase"
            }
            Self::SetUserPolicyRequireLowercase(_) => {
                "Server::SetUserPolicyRequireLowercase"
            }
            Self::SetUserPolicyRequireNumber(_) => {
                "Server::SetUserPolicyRequireNumber"
            }
            Self::SetUserPolicyRequireSpecial(_) => {
                "Server::SetUserPolicyRequireSpecial"
            }
            Self::SetCuratedMaxCarouselItems(_) => {
                "Server::SetCuratedMaxCarouselItems"
            }
            Self::SetCuratedHeadWindow(_) => "Server::SetCuratedHeadWindow",
        }
    }
}
