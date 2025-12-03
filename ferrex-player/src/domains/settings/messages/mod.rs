use ferrex_core::player_prelude::UserScale;

use crate::domains::settings::sections::{
    display::DisplayMessage, performance::PerformanceMessage,
    playback::PlaybackMessage, theme::ThemeMessage,
};
use crate::domains::settings::state::SettingsSection;
use crate::infra::design_tokens::ScalePreset;

#[derive(Debug, Clone)]
pub enum SettingsMessage {
    // Navigation (new unified sidebar)
    NavigateToSection(SettingsSection),

    // Sub-domain routing (new)
    Playback(PlaybackMessage),
    Display(DisplayMessage),
    Theme(ThemeMessage),
    Performance(PerformanceMessage),

    // Navigation (legacy - to be deprecated)
    ShowProfile,
    ShowPreferences,
    ShowSecurity,
    BackToMain,
    BackToHome,

    // Preferences - UI Scale
    SetUserScale(UserScale),
    SetScalePreset(ScalePreset),

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

impl SettingsMessage {
    pub fn name(&self) -> &'static str {
        match self {
            // Navigation (new)
            Self::NavigateToSection(_) => "Settings::NavigateToSection",

            // Sub-domain routing (new)
            Self::Playback(msg) => msg.name(),
            Self::Display(msg) => msg.name(),
            Self::Theme(msg) => msg.name(),
            Self::Performance(msg) => msg.name(),

            // Navigation (legacy)
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
            Self::TogglePasswordVisibility => {
                "Settings::TogglePasswordVisibility"
            }
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
            Self::SetUserScale(_) => "Settings::SetUserScale",
            Self::SetScalePreset(_) => "Settings::SetScalePreset",

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

impl std::fmt::Display for SettingsMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

// Conversion to DomainMessage is already handled in messages/mod.rs
