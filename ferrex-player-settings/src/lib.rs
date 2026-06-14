//! Settings domain surfaces for Ferrex player clients.
//!
//! This crate owns settings state, settings/admin-section messages, settings
//! view-model DTOs, and reducer helpers. UI crates render these types and keep
//! app-specific task/cross-domain routing at the application boundary.

#![forbid(unsafe_code)]

pub mod color;
pub mod messages;
pub mod scale;
pub mod sections;
pub mod state;
pub mod update;

use std::sync::Arc;

use ferrex_core::player_prelude::UserPermissions;
use ferrex_player_api::services::{api::ApiService, settings::SettingsService};
use ferrex_player_auth::AuthService;

pub use color::{AccentColorConfig, ColorPoint, HarmonyMode, HsluvColor};
pub use messages::SettingsMessage;
pub use scale::ScalePreset;
pub use state::SettingsSection;

use sections::devices::state::DeviceManagementState;
use sections::display::DisplayState;
use sections::performance::PerformanceState;
use sections::playback::PlaybackState;
use sections::theme::ThemeState;

/// Runtime settings domain state shared by player frontends.
pub struct SettingsDomainState {
    /// Current section in the unified settings sidebar.
    pub current_section: state::SettingsSection,
    pub security: state::SecurityState,
    pub profile: state::ProfileState,
    pub preferences: state::PreferencesState,
    pub device_management_state: DeviceManagementState,

    /// Playback settings section state.
    pub playback: PlaybackState,
    /// Display settings section state.
    pub display: DisplayState,
    /// Theme settings section state.
    pub theme: ThemeState,
    /// Performance settings section state.
    pub performance: PerformanceState,

    /// Authenticated user's permissions, when available.
    pub user_permissions: Option<UserPermissions>,
    /// Authentication service used by settings commands.
    pub auth_service: Arc<dyn AuthService>,
    /// API service reference retained for settings integrations.
    pub api_service: Arc<dyn ApiService>,
    /// Settings API service used for device management.
    pub settings_service: Arc<dyn SettingsService>,
}

impl SettingsDomainState {
    /// Create settings domain state with the required service adapters.
    pub fn new(
        auth_service: Arc<dyn AuthService>,
        api_service: Arc<dyn ApiService>,
        settings_service: Arc<dyn SettingsService>,
    ) -> Self {
        Self {
            current_section: state::SettingsSection::default(),
            security: state::SecurityState::default(),
            profile: state::ProfileState::default(),
            preferences: state::PreferencesState::default(),
            device_management_state: DeviceManagementState::default(),
            playback: PlaybackState::default(),
            display: DisplayState::default(),
            theme: ThemeState::default(),
            performance: PerformanceState::default(),
            user_permissions: None,
            auth_service,
            api_service,
            settings_service,
        }
    }
}

impl std::fmt::Debug for SettingsDomainState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsDomainState")
            .field("current_section", &self.current_section)
            .field("security", &"<omitted>")
            .field("profile", &self.profile)
            .field("preferences", &self.preferences)
            .field("device_management_state", &"<omitted>")
            .field("playback", &self.playback)
            .field("display", &self.display)
            .field("theme", &self.theme)
            .field("performance", &self.performance)
            .field("user_permissions", &self.user_permissions)
            .field("auth_service", &"AuthService(..)")
            .field("api_service", &"ApiService(..)")
            .field("settings_service", &"SettingsService(..)")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{ScalePreset, SettingsSection};

    #[test]
    fn settings_sections_keep_user_and_admin_groups() {
        assert!(!SettingsSection::Security.is_admin());
        assert!(SettingsSection::Users.is_admin());
        assert!(
            SettingsSection::user_sections()
                .contains(&SettingsSection::Devices)
        );
        assert!(
            SettingsSection::admin_sections()
                .contains(&SettingsSection::Server)
        );
    }

    #[test]
    fn scale_presets_expose_stable_labels() {
        assert_eq!(ScalePreset::TV.display_name(), "TV");
        assert!((ScalePreset::Large.scale_factor() - 1.2).abs() < 0.001);
    }
}
