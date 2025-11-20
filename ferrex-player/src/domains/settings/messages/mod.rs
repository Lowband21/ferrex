#[derive(Debug, Clone)]
pub enum Message {
    // Navigation
    ShowProfile,
    ShowPreferences,
    ShowSecurity,
    BackToMain,
    BackToHome,

    // Security - Password
    ShowChangePassword,
    UpdatePasswordCurrent(String),
    UpdatePasswordNew(String),
    UpdatePasswordConfirm(String),
    TogglePasswordVisibility,
    SubmitPasswordChange,
    PasswordChangeResult(Result<(), String>),
    CancelPasswordChange,

    // Security - PIN
    CheckUserHasPin,
    UserHasPinResult(bool),
    ShowSetPin,
    ShowChangePin,
    UpdatePinCurrent(String),
    UpdatePinNew(String),
    UpdatePinConfirm(String),
    SubmitPinChange,
    PinChangeResult(Result<(), String>),
    CancelPinChange,

    // Preferences - Auto-login
    ToggleAutoLogin(bool),
    AutoLoginToggled(Result<bool, String>),

    // Profile
    UpdateDisplayName(String),
    UpdateEmail(String),
    SubmitProfileChanges,
    ProfileChangeResult(Result<(), String>),

    // Device Management
    LoadDevices,
    DevicesLoaded(
        Result<Vec<crate::domains::ui::views::settings::device_management::UserDevice>, String>,
    ),
    RefreshDevices,
    RevokeDevice(String),                  // device_id
    DeviceRevoked(Result<String, String>), // device_id or error
}

impl Message {
    pub fn name(&self) -> &'static str {
        match self {
            // Navigation
            Self::ShowProfile => "Settings::ShowProfile",
            Self::ShowPreferences => "Settings::ShowPreferences",
            Self::ShowSecurity => "Settings::ShowSecurity",
            Self::BackToMain => "Settings::BackToMain",
            Self::BackToHome => "Settings::BackToHome",

            // Security - Password
            Self::ShowChangePassword => "Settings::ShowChangePassword",
            Self::UpdatePasswordCurrent(_) => "Settings::UpdatePasswordCurrent",
            Self::UpdatePasswordNew(_) => "Settings::UpdatePasswordNew",
            Self::UpdatePasswordConfirm(_) => "Settings::UpdatePasswordConfirm",
            Self::TogglePasswordVisibility => "Settings::TogglePasswordVisibility",
            Self::SubmitPasswordChange => "Settings::SubmitPasswordChange",
            Self::PasswordChangeResult(_) => "Settings::PasswordChangeResult",
            Self::CancelPasswordChange => "Settings::CancelPasswordChange",

            // Security - PIN
            Self::CheckUserHasPin => "Settings::CheckUserHasPin",
            Self::UserHasPinResult(_) => "Settings::UserHasPinResult",
            Self::ShowSetPin => "Settings::ShowSetPin",
            Self::ShowChangePin => "Settings::ShowChangePin",
            Self::UpdatePinCurrent(_) => "Settings::UpdatePinCurrent",
            Self::UpdatePinNew(_) => "Settings::UpdatePinNew",
            Self::UpdatePinConfirm(_) => "Settings::UpdatePinConfirm",
            Self::SubmitPinChange => "Settings::SubmitPinChange",
            Self::PinChangeResult(_) => "Settings::PinChangeResult",
            Self::CancelPinChange => "Settings::CancelPinChange",

            // Preferences
            Self::ToggleAutoLogin(_) => "Settings::ToggleAutoLogin",
            Self::AutoLoginToggled(_) => "Settings::AutoLoginToggled",

            // Profile
            Self::UpdateDisplayName(_) => "Settings::UpdateDisplayName",
            Self::UpdateEmail(_) => "Settings::UpdateEmail",
            Self::SubmitProfileChanges => "Settings::SubmitProfileChanges",
            Self::ProfileChangeResult(_) => "Settings::ProfileChangeResult",

            // Device Management
            Self::LoadDevices => "Settings::LoadDevices",
            Self::DevicesLoaded(_) => "Settings::DevicesLoaded",
            Self::RefreshDevices => "Settings::RefreshDevices",
            Self::RevokeDevice(_) => "Settings::RevokeDevice",
            Self::DeviceRevoked(_) => "Settings::DeviceRevoked",
        }
    }
}

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

// Conversion to DomainMessage is already handled in messages/mod.rs
