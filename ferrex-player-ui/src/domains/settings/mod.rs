//! Settings management domain integration for the desktop player.
//!
//! Settings state, messages, section DTOs, and reducer source live in
//! `ferrex-player-settings`. This module keeps the desktop app's task and
//! cross-domain routing glue while preserving historical
//! `ferrex_player::domains::settings::*` imports for UI code and tests.

pub mod messages;
pub mod sections;
pub mod state;
pub mod update;

pub use ferrex_player_settings::{
    AccentColorConfig, ColorPoint, HarmonyMode, HsluvColor, ScalePreset,
    SettingsDomainState, SettingsMessage, SettingsSection,
};
pub use sections::{
    DevicesSection, DisplaySection, PerformanceSection, PlaybackSection,
    ProfileSection, SecuritySection, ThemeSection,
};

use std::sync::Arc;

use crate::common::messages::{CrossDomainEvent, DomainMessage};
use crate::infra::services::api::ApiService;
use crate::infra::services::auth::AuthService;
use crate::infra::services::settings::SettingsService;
use ferrex_core::player_prelude::UserPermissions;
use iced::Task;

use sections::devices::state::DeviceManagementState;
use sections::display::DisplayState;
use sections::performance::PerformanceState;
use sections::playback::PlaybackState;
use sections::theme::ThemeState;

/// Desktop settings domain wrapper.
pub struct SettingsDomain {
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

impl SettingsDomain {
    pub fn new(state: SettingsDomainState) -> Self {
        Self {
            current_section: state.current_section,
            security: state.security,
            profile: state.profile,
            preferences: state.preferences,
            device_management_state: state.device_management_state,
            playback: state.playback,
            display: state.display,
            theme: state.theme,
            performance: state.performance,
            user_permissions: state.user_permissions,
            auth_service: state.auth_service,
            api_service: state.api_service,
            settings_service: state.settings_service,
        }
    }

    pub fn handle_event(
        &mut self,
        event: &CrossDomainEvent,
    ) -> Task<DomainMessage> {
        match event {
            CrossDomainEvent::UserAuthenticated(_user, permissions) => {
                self.user_permissions = Some(permissions.clone());
                Task::none()
            }
            _ => Task::none(),
        }
    }
}

impl std::fmt::Debug for SettingsDomain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsDomain")
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
            .field("api_service", &"ApiClientAdapter(..)")
            .field("settings_service", &"SettingsService(..)")
            .finish()
    }
}
