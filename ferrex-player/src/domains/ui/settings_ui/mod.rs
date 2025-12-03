pub mod update;

use ferrex_core::player_prelude::UserScale;
use ferrex_model::{Library, LibraryId};
pub use update::update_settings_ui;

use crate::domains::{
    library::media_root_browser,
    settings::{
        sections::{
            display::messages::DisplayMessage, theme::messages::ThemeMessage,
        },
        state::SettingsSection,
    },
    ui::{messages::UiMessage, views::settings::device_management::UserDevice},
};
use crate::infra::design_tokens::ScalePreset;

use uuid::Uuid;

/// Sub-message for RuntimeConfig adjustments
#[derive(Clone, Debug)]
pub enum RuntimeConfigMessage {
    // Grid Scrolling
    ScrollDebounce(u64),
    ScrollBaseVelocity(f32),
    ScrollMaxVelocity(f32),
    ScrollDecayTau(u64),
    ScrollRamp(u64),
    ScrollBoost(f32),

    // Carousel Motion
    CarouselBaseVelocity(f32),
    CarouselMaxVelocity(f32),
    CarouselDecayTau(u64),
    CarouselRamp(u64),
    CarouselBoost(f32),

    // Snap Animations
    SnapItemDuration(u64),
    SnapPageDuration(u64),
    SnapHoldThreshold(u64),
    SnapEpsilon(f32),

    // Animation Effects
    HoverScale(f32),
    HoverTransition(u64),
    AnimationDuration(u64),
    TextureFadeInitial(u64),
    TextureFade(u64),

    // GPU/Memory
    TextureUploads(u32),
    PrefetchRowsAbove(usize),
    PrefetchRowsBelow(usize),
    CarouselPrefetch(usize),
    CarouselBackground(usize),
    KeepAlive(u64),

    // Player Seeking
    SeekForwardCoarse(f64),
    SeekBackwardCoarse(f64),
    SeekForwardFine(f64),
    SeekBackwardFine(f64),
}

impl From<RuntimeConfigMessage> for SettingsUiMessage {
    fn from(msg: RuntimeConfigMessage) -> Self {
        SettingsUiMessage::RuntimeConfig(msg)
    }
}

impl From<RuntimeConfigMessage> for UiMessage {
    fn from(msg: RuntimeConfigMessage) -> Self {
        UiMessage::Settings(SettingsUiMessage::RuntimeConfig(msg))
    }
}

#[derive(Clone)]
pub enum SettingsUiMessage {
    // Unified settings navigation (new sidebar)
    NavigateToSection(SettingsSection),

    // Admin views
    ShowAdminDashboard,
    HideAdminDashboard,
    ShowLibraryManagement,
    HideLibraryManagement,
    ShowUserManagement,
    HideUserManagement,

    // Proxies to user management domain
    UserAdminDelete(Uuid),
    #[cfg(feature = "demo")]
    DemoMoviesTargetChanged(String),
    #[cfg(feature = "demo")]
    DemoSeriesTargetChanged(String),
    #[cfg(feature = "demo")]
    DemoApplySizing,
    #[cfg(feature = "demo")]
    DemoRefreshStatus,

    // Database maintenance UI
    ShowClearDatabaseConfirm,
    HideClearDatabaseConfirm,
    ClearDatabase,
    DatabaseCleared(Result<(), String>),

    // User settings navigation
    ShowProfile, // Redundant?
    ShowUserProfile,
    ShowUserPreferences,
    ShowUserSecurity,
    ShowDeviceManagement,
    BackToSettings,
    Logout,

    // Security settings
    ShowChangePassword,
    UpdatePasswordCurrent(String),
    UpdatePasswordNew(String),
    UpdatePasswordConfirm(String),
    TogglePasswordVisibility,
    SubmitPasswordChange,
    PasswordChangeResult(Result<(), String>),
    CancelPasswordChange,

    ShowSetPin,
    ShowChangePin,
    UpdatePinCurrent(String),
    UpdatePinNew(String),
    UpdatePinConfirm(String),
    SubmitPinChange,
    PinChangeResult(Result<(), String>),
    CancelPinChange,

    // Admin PIN unlock (UI proxy to Auth domain)
    EnableAdminPinUnlock,
    DisableAdminPinUnlock,

    // Device management - now proxies to cross-domain events
    LoadDevices,
    DevicesLoaded(Result<Vec<UserDevice>, String>),
    RevokeDevice(String),                  // device_id
    DeviceRevoked(Result<String, String>), // device_id or error
    RefreshDevices,

    // User preferences
    ToggleAutoLogin(bool),
    AutoLoginToggled(Result<bool, String>), // Proxy for Auth::Logout
    SetUserScale(UserScale),                // Proxy for Settings::SetUserScale
    SetScalePreset(ScalePreset), // Proxy for Settings::SetScalePreset
    ScaleSliderPreview(f32),     // Preview during slider drag (UI-only)
    ScaleTextInput(String),      // Text input field update (UI-only)

    // Playback settings (seeking - String for domain validation)
    SetSeekForwardCoarse(String),
    SetSeekBackwardCoarse(String),
    SetSeekForwardFine(String),
    SetSeekBackwardFine(String),

    // Display settings (String for domain validation)
    SetPosterWidth(String),
    SetPosterHeight(String),
    SetCornerRadius(String),
    SetGridSpacing(String),
    SetRowSpacing(String),
    SetHoverScale(String),
    SetAnimationDuration(String),

    // Performance settings (String for domain validation - legacy)
    SetScrollDebounce(String),
    SetScrollMaxVelocity(String),
    SetScrollDecay(String),

    // RuntimeConfig-based settings (sub-router)
    RuntimeConfig(RuntimeConfigMessage),

    // Display settings (sub-router)
    Display(DisplayMessage),

    // Theme settings (sub-router)
    Theme(ThemeMessage),

    // Library management proxies
    ShowLibraryForm(Option<Library>), // Proxy for Library::ShowLibraryForm
    HideLibraryForm,                  // Proxy for Library::HideLibraryForm
    ScanLibrary(LibraryId),           // Proxy for Library::ScanLibrary_
    DeleteLibrary(LibraryId),         // Proxy for Library::DeleteLibrary
    UpdateLibraryFormName(String), // Proxy for Library::UpdateLibraryFormName
    UpdateLibraryFormType(String), // Proxy for Library::UpdateLibraryFormType
    UpdateLibraryFormPaths(String), // Proxy for Library::UpdateLibraryFormPaths
    UpdateLibraryFormScanInterval(String), // Proxy for Library::UpdateLibraryFormScanInterval
    ToggleLibraryFormEnabled, // Proxy for Library::ToggleLibraryFormEnabled
    ToggleLibraryFormStartScan, // Proxy for Library::ToggleLibraryFormStartScan
    SubmitLibraryForm,        // Proxy for Library::SubmitLibraryForm
    LibraryMediaRoot(media_root_browser::Message), // Proxy collection for media root browser actions
    PauseLibraryScan(LibraryId, Uuid), // Proxy for Library::PauseScan
    ResumeLibraryScan(LibraryId, Uuid), // Proxy for Library::ResumeScan
    CancelLibraryScan(LibraryId, Uuid), // Proxy for Library::CancelScan
    // Scanner metrics + admin actions
    FetchScanMetrics, // Proxy for Library::FetchScanMetrics
    ResetLibrary(LibraryId), // Proxy for Library::ResetLibrary
}

impl From<SettingsUiMessage> for UiMessage {
    fn from(msg: SettingsUiMessage) -> Self {
        UiMessage::Settings(msg)
    }
}

impl SettingsUiMessage {
    pub fn name(&self) -> &'static str {
        match self {
            // Unified settings navigation
            Self::NavigateToSection(_) => "UI::NavigateToSection",

            // Admin views
            Self::ShowAdminDashboard => "UI::ShowAdminDashboard",
            Self::HideAdminDashboard => "UI::HideAdminDashboard",
            Self::ShowLibraryManagement => "UI::ShowLibraryManagement",
            Self::HideLibraryManagement => "UI::HideLibraryManagement",
            Self::ShowUserManagement => "UI::ShowUserManagement",
            Self::HideUserManagement => "UI::HideUserManagement",

            Self::UserAdminDelete(_) => "UI::UserAdminDelete",
            #[cfg(feature = "demo")]
            Self::DemoMoviesTargetChanged(_) => "UI::DemoMoviesTargetChanged",
            #[cfg(feature = "demo")]
            Self::DemoSeriesTargetChanged(_) => "UI::DemoSeriesTargetChanged",
            #[cfg(feature = "demo")]
            Self::DemoApplySizing => "UI::DemoApplySizing",
            #[cfg(feature = "demo")]
            Self::DemoRefreshStatus => "UI::DemoRefreshStatus",

            // Database maintenance UI
            Self::ShowClearDatabaseConfirm => "UI::ShowClearDatabaseConfirm",
            Self::HideClearDatabaseConfirm => "UI::HideClearDatabaseConfirm",
            Self::ClearDatabase => "UI::ClearDatabase",
            Self::DatabaseCleared(_) => "UI::DatabaseCleared",

            // User settings navigation
            Self::ShowProfile => "UI::ShowProfile",
            Self::ShowUserProfile => "UI::ShowUserProfile",
            Self::ShowUserPreferences => "UI::ShowUserPreferences",
            Self::ShowUserSecurity => "UI::ShowUserSecurity",
            Self::ShowDeviceManagement => "UI::ShowDeviceManagement",
            Self::BackToSettings => "UI::BackToSettings",
            Self::Logout => "UI::Logout",

            // Security settings
            Self::ShowChangePassword => "UI::ShowChangePassword",
            Self::UpdatePasswordCurrent(_) => "UI::UpdatePasswordCurrent",
            Self::UpdatePasswordNew(_) => "UI::UpdatePasswordNew",
            Self::UpdatePasswordConfirm(_) => "UI::UpdatePasswordConfirm",
            Self::TogglePasswordVisibility => "UI::TogglePasswordVisibility",
            Self::SubmitPasswordChange => "UI::SubmitPasswordChange",
            Self::PasswordChangeResult(_) => "UI::PasswordChangeResult",
            Self::CancelPasswordChange => "UI::CancelPasswordChange",

            Self::ShowSetPin => "UI::ShowSetPin",
            Self::ShowChangePin => "UI::ShowChangePin",
            Self::UpdatePinCurrent(_) => "UI::UpdatePinCurrent",
            Self::UpdatePinNew(_) => "UI::UpdatePinNew",
            Self::UpdatePinConfirm(_) => "UI::UpdatePinConfirm",
            Self::SubmitPinChange => "UI::SubmitPinChange",
            Self::PinChangeResult(_) => "UI::PinChangeResult",
            Self::CancelPinChange => "UI::CancelPinChange",

            // Admin PIN unlock
            Self::EnableAdminPinUnlock => "UI::EnableAdminPinUnlock",
            Self::DisableAdminPinUnlock => "UI::DisableAdminPinUnlock",

            // Device management
            Self::LoadDevices => "UI::LoadDevices",
            Self::DevicesLoaded(_) => "UI::DevicesLoaded",
            Self::RevokeDevice(_) => "UI::RevokeDevice",
            Self::DeviceRevoked(_) => "UI::DeviceRevoked",
            Self::RefreshDevices => "UI::RefreshDevices",

            // User preferences
            Self::ToggleAutoLogin(_) => "UI::ToggleAutoLogin",
            Self::AutoLoginToggled(_) => "UI::AutoLoginToggled",
            Self::SetUserScale(_) => "UI::SetUserScale",
            Self::SetScalePreset(_) => "UI::SetScalePreset",
            Self::ScaleSliderPreview(_) => "UI::ScaleSliderPreview",
            Self::ScaleTextInput(_) => "UI::ScaleTextInput",

            // Playback settings
            Self::SetSeekForwardCoarse(_) => "UI::SetSeekForwardCoarse",
            Self::SetSeekBackwardCoarse(_) => "UI::SetSeekBackwardCoarse",
            Self::SetSeekForwardFine(_) => "UI::SetSeekForwardFine",
            Self::SetSeekBackwardFine(_) => "UI::SetSeekBackwardFine",

            // Display settings
            Self::SetPosterWidth(_) => "UI::SetPosterWidth",
            Self::SetPosterHeight(_) => "UI::SetPosterHeight",
            Self::SetCornerRadius(_) => "UI::SetCornerRadius",
            Self::SetGridSpacing(_) => "UI::SetGridSpacing",
            Self::SetRowSpacing(_) => "UI::SetRowSpacing",
            Self::SetHoverScale(_) => "UI::SetHoverScale",
            Self::SetAnimationDuration(_) => "UI::SetAnimationDuration",

            // Performance settings (legacy)
            Self::SetScrollDebounce(_) => "UI::SetScrollDebounce",
            Self::SetScrollMaxVelocity(_) => "UI::SetScrollMaxVelocity",
            Self::SetScrollDecay(_) => "UI::SetScrollDecay",

            // RuntimeConfig sub-router
            Self::RuntimeConfig(_) => "UI::RuntimeConfig",

            // Display settings sub-router
            Self::Display(msg) => msg.name(),

            // Theme settings sub-router
            Self::Theme(msg) => msg.name(),

            // Library management proxies
            Self::ShowLibraryForm(_) => "UI::ShowLibraryForm",
            Self::HideLibraryForm => "UI::HideLibraryForm",
            Self::ScanLibrary(_) => "UI::ScanLibrary_",
            Self::DeleteLibrary(_) => "UI::DeleteLibrary",
            Self::UpdateLibraryFormName(_) => "UI::UpdateLibraryFormName",
            Self::UpdateLibraryFormType(_) => "UI::UpdateLibraryFormType",
            Self::UpdateLibraryFormPaths(_) => "UI::UpdateLibraryFormPaths",
            Self::UpdateLibraryFormScanInterval(_) => {
                "UI::UpdateLibraryFormScanInterval"
            }
            Self::ToggleLibraryFormEnabled => "UI::ToggleLibraryFormEnabled",
            Self::ToggleLibraryFormStartScan => {
                "UI::ToggleLibraryFormStartScan"
            }
            Self::SubmitLibraryForm => "UI::SubmitLibraryForm",
            Self::LibraryMediaRoot(_) => "UI::LibraryMediaRoot",
            Self::PauseLibraryScan(_, _) => "UI::PauseLibraryScan",
            Self::ResumeLibraryScan(_, _) => "UI::ResumeLibraryScan",
            Self::CancelLibraryScan(_, _) => "UI::CancelLibraryScan",
            Self::FetchScanMetrics => "UI::FetchScanMetrics",
            Self::ResetLibrary(_) => "UI::ResetLibrary",
        }
    }
}

impl std::fmt::Debug for SettingsUiMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsUiMessage::NavigateToSection(section) => {
                write!(f, "UI::NavigateToSection({:?})", section)
            }
            SettingsUiMessage::ShowAdminDashboard => {
                write!(f, "UI::ShowAdminDashboard")
            }
            SettingsUiMessage::HideAdminDashboard => {
                write!(f, "UI::HideAdminDashboard")
            }
            SettingsUiMessage::ShowLibraryManagement => {
                write!(f, "UI::ShowLibraryManagement")
            }
            SettingsUiMessage::HideLibraryManagement => {
                write!(f, "UI::HideLibraryManagement")
            }
            SettingsUiMessage::ShowUserManagement => {
                write!(f, "UI::ShowUserManagement")
            }
            SettingsUiMessage::HideUserManagement => {
                write!(f, "UI::HideUserManagement")
            }
            SettingsUiMessage::UserAdminDelete(uuid) => {
                write!(f, "UI::UserAdminDelete({})", uuid)
            }
            #[cfg(feature = "demo")]
            SettingsUiMessage::DemoMoviesTargetChanged(value) => {
                write!(f, "UI::DemoMoviesTargetChanged({value})")
            }
            #[cfg(feature = "demo")]
            SettingsUiMessage::DemoSeriesTargetChanged(value) => {
                write!(f, "UI::DemoSeriesTargetChanged({value})")
            }
            #[cfg(feature = "demo")]
            SettingsUiMessage::DemoApplySizing => {
                write!(f, "UI::DemoApplySizing")
            }
            #[cfg(feature = "demo")]
            SettingsUiMessage::DemoRefreshStatus => {
                write!(f, "UI::DemoRefreshStatus")
            }
            SettingsUiMessage::ShowClearDatabaseConfirm => {
                write!(f, "UI::ShowClearDatabaseConfirm")
            }
            SettingsUiMessage::HideClearDatabaseConfirm => {
                write!(f, "UI::HideClearDatabaseConfirm")
            }
            SettingsUiMessage::ClearDatabase => write!(f, "UI::ClearDatabase"),
            SettingsUiMessage::DatabaseCleared(_) => {
                write!(f, "UI::DatabaseCleared")
            }
            SettingsUiMessage::ShowProfile => {
                write!(f, "UI::ShowUserProfile")
            }
            SettingsUiMessage::ShowUserProfile => {
                write!(f, "UI::ShowUserProfile")
            }
            SettingsUiMessage::ShowUserPreferences => {
                write!(f, "UI::ShowUserPreferences")
            }
            SettingsUiMessage::ShowUserSecurity => {
                write!(f, "UI::ShowUserSecurity")
            }
            SettingsUiMessage::ShowDeviceManagement => {
                write!(f, "UI::ShowDeviceManagement")
            }
            SettingsUiMessage::BackToSettings => {
                write!(f, "UI::BackToSettings")
            }
            SettingsUiMessage::Logout => write!(f, "UI::Logout"),
            SettingsUiMessage::ShowChangePassword => {
                write!(f, "UI::ShowChangePassword")
            }
            SettingsUiMessage::UpdatePasswordCurrent(_) => {
                write!(f, "UI::UpdatePasswordCurrent")
            }
            SettingsUiMessage::UpdatePasswordNew(_) => {
                write!(f, "UI::UpdatePasswordNew")
            }
            SettingsUiMessage::UpdatePasswordConfirm(_) => {
                write!(f, "UI::UpdatePasswordConfirm")
            }
            SettingsUiMessage::TogglePasswordVisibility => {
                write!(f, "UI::TogglePasswordVisibility")
            }
            SettingsUiMessage::SubmitPasswordChange => {
                write!(f, "UI::SubmitPasswordChange")
            }
            SettingsUiMessage::PasswordChangeResult(_) => {
                write!(f, "UI::PasswordChangeResult")
            }
            SettingsUiMessage::CancelPasswordChange => {
                write!(f, "UI::CancelPasswordChange")
            }
            SettingsUiMessage::ShowSetPin => write!(f, "UI::ShowSetPin"),
            SettingsUiMessage::ShowChangePin => write!(f, "UI::ShowChangePin"),
            SettingsUiMessage::UpdatePinCurrent(_) => {
                write!(f, "UI::UpdatePinCurrent")
            }
            SettingsUiMessage::UpdatePinNew(_) => write!(f, "UI::UpdatePinNew"),
            SettingsUiMessage::UpdatePinConfirm(_) => {
                write!(f, "UI::UpdatePinConfirm")
            }
            SettingsUiMessage::SubmitPinChange => {
                write!(f, "UI::SubmitPinChange")
            }
            SettingsUiMessage::PinChangeResult(_) => {
                write!(f, "UI::PinChangeResult")
            }
            SettingsUiMessage::CancelPinChange => {
                write!(f, "UI::CancelPinChange")
            }
            SettingsUiMessage::EnableAdminPinUnlock => {
                write!(f, "UI::EnableAdminPinUnlock")
            }
            SettingsUiMessage::DisableAdminPinUnlock => {
                write!(f, "UI::DisableAdminPinUnlock")
            }
            SettingsUiMessage::LoadDevices => write!(f, "UI::LoadDevices"),
            SettingsUiMessage::DevicesLoaded(result) => match result {
                Ok(devices) => write!(
                    f,
                    "UI::DevicesLoaded(Ok: {} devices)",
                    devices.len()
                ),
                Err(e) => write!(f, "UI::DevicesLoaded(Err: {})", e),
            },
            SettingsUiMessage::RevokeDevice(device_id) => {
                write!(f, "UI::RevokeDevice({})", device_id)
            }
            SettingsUiMessage::DeviceRevoked(result) => match result {
                Ok(device_id) => {
                    write!(f, "UI::DeviceRevoked(Ok: {})", device_id)
                }
                Err(e) => write!(f, "UI::DeviceRevoked(Err: {})", e),
            },
            SettingsUiMessage::RefreshDevices => {
                write!(f, "UI::RefreshDevices")
            }

            SettingsUiMessage::ToggleAutoLogin(_) => {
                write!(f, "UI::ToggleAutoLogin")
            }
            SettingsUiMessage::AutoLoginToggled(_) => {
                write!(f, "UI::AutoLoginToggled")
            }

            SettingsUiMessage::UpdateLibraryFormType(_) => {
                write!(f, "UI::UpdateLibraryFormType()")
            }
            SettingsUiMessage::UpdateLibraryFormPaths(_) => {
                write!(f, "UI::UpdateLibraryFormPaths()")
            }
            SettingsUiMessage::UpdateLibraryFormScanInterval(_) => {
                write!(f, "UI::UpdateLibraryFormScanInterval()")
            }
            SettingsUiMessage::ToggleLibraryFormEnabled => {
                write!(f, "UI::ToggleLibraryFormEnabled")
            }
            SettingsUiMessage::ToggleLibraryFormStartScan => {
                write!(f, "UI::ToggleLibraryFormStartScan")
            }
            SettingsUiMessage::SubmitLibraryForm => {
                write!(f, "UI::SubmitLibraryForm")
            }
            SettingsUiMessage::LibraryMediaRoot(message) => {
                write!(f, "UI::LibraryMediaRoot({:?})", message)
            }
            SettingsUiMessage::FetchScanMetrics => {
                write!(f, "UI::FetchScanMetrics")
            }
            SettingsUiMessage::ResetLibrary(id) => {
                write!(f, "UI::ResetLibrary({})", id)
            }
            SettingsUiMessage::ShowLibraryForm(lib) => {
                if let Some(l) = lib {
                    write!(f, "UI::ShowLibraryForm(Some: {})", l.name)
                } else {
                    write!(f, "UI::ShowLibraryForm(None)")
                }
            }
            SettingsUiMessage::UpdateLibraryFormName(name) => {
                write!(f, "UI::UpdateLibraryFormName({})", name)
            }
            SettingsUiMessage::HideLibraryForm => {
                write!(f, "UI::HideLibraryForm")
            }

            SettingsUiMessage::ScanLibrary(id) => {
                write!(f, "UI::ScanLibrary_({})", id)
            }
            SettingsUiMessage::DeleteLibrary(id) => {
                write!(f, "UI::DeleteLibrary({})", id)
            }
            SettingsUiMessage::PauseLibraryScan(library_id, scan_id) => {
                write!(f, "UI::PauseLibraryScan({}, {})", library_id, scan_id)
            }
            SettingsUiMessage::ResumeLibraryScan(library_id, scan_id) => {
                write!(f, "UI::ResumeLibraryScan({}, {})", library_id, scan_id)
            }
            SettingsUiMessage::CancelLibraryScan(library_id, scan_id) => {
                write!(f, "UI::CancelLibraryScan({}, {})", library_id, scan_id)
            }
            SettingsUiMessage::SetUserScale(size) => {
                write!(f, "UI::SetUserScale({:?})", size)
            }
            SettingsUiMessage::SetScalePreset(preset) => {
                write!(f, "UI::SetScalePreset({:?})", preset)
            }
            SettingsUiMessage::ScaleSliderPreview(v) => {
                write!(f, "UI::ScaleSliderPreview({:.2})", v)
            }
            SettingsUiMessage::ScaleTextInput(s) => {
                write!(f, "UI::ScaleTextInput({})", s)
            }

            // Playback settings
            SettingsUiMessage::SetSeekForwardCoarse(_) => {
                write!(f, "UI::SetSeekForwardCoarse")
            }
            SettingsUiMessage::SetSeekBackwardCoarse(_) => {
                write!(f, "UI::SetSeekBackwardCoarse")
            }
            SettingsUiMessage::SetSeekForwardFine(_) => {
                write!(f, "UI::SetSeekForwardFine")
            }
            SettingsUiMessage::SetSeekBackwardFine(_) => {
                write!(f, "UI::SetSeekBackwardFine")
            }

            // Display settings
            SettingsUiMessage::SetPosterWidth(_) => {
                write!(f, "UI::SetPosterWidth")
            }
            SettingsUiMessage::SetPosterHeight(_) => {
                write!(f, "UI::SetPosterHeight")
            }
            SettingsUiMessage::SetCornerRadius(_) => {
                write!(f, "UI::SetCornerRadius")
            }
            SettingsUiMessage::SetGridSpacing(_) => {
                write!(f, "UI::SetGridSpacing")
            }
            SettingsUiMessage::SetRowSpacing(_) => {
                write!(f, "UI::SetRowSpacing")
            }
            SettingsUiMessage::SetHoverScale(_) => {
                write!(f, "UI::SetHoverScale")
            }
            SettingsUiMessage::SetAnimationDuration(_) => {
                write!(f, "UI::SetAnimationDuration")
            }

            // Performance settings
            SettingsUiMessage::SetScrollDebounce(_) => {
                write!(f, "UI::SetScrollDebounce")
            }
            SettingsUiMessage::SetScrollMaxVelocity(_) => {
                write!(f, "UI::SetScrollMaxVelocity")
            }
            SettingsUiMessage::SetScrollDecay(_) => {
                write!(f, "UI::SetScrollDecay")
            }
            SettingsUiMessage::RuntimeConfig(msg) => {
                write!(f, "UI::RuntimeConfig({:?})", msg)
            }
            SettingsUiMessage::Display(msg) => {
                write!(f, "UI::Display({:?})", msg)
            }
            SettingsUiMessage::Theme(msg) => {
                write!(f, "UI::Theme({:?})", msg)
            }
        }
    }
}
