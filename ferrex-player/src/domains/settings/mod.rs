//! Settings management domain
//!
//! Contains all settings-related state and logic moved from the monolithic State

pub mod messages;
pub mod state;
pub mod update;

use self::messages::Message as SettingsMessage;
use crate::common::messages::{CrossDomainEvent, DomainMessage};
use crate::infrastructure::services::api::ApiService;
use ferrex_core::player_prelude::UserPermissions;
use iced::Task;

/// Settings domain state - moved from monolithic State
pub struct SettingsDomainState {
    // Settings fields moved directly here to avoid state.state nesting
    pub current_view: state::SettingsView,
    pub security: state::SecurityState,
    pub profile: state::ProfileState,
    pub preferences: state::PreferencesState,
    pub device_management_state:
        crate::domains::ui::views::settings::device_management::DeviceManagementState,

    // References needed by settings domain
    pub user_permissions: Option<UserPermissions>,
    pub auth_service: std::sync::Arc<dyn crate::infrastructure::services::auth::AuthService>,
    pub api_service: std::sync::Arc<dyn ApiService>,
    pub settings_service:
        std::sync::Arc<dyn crate::infrastructure::services::settings::SettingsService>,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl SettingsDomainState {
    /// Create a new SettingsDomainState with required services
    pub fn new(
        auth_service: std::sync::Arc<dyn crate::infrastructure::services::auth::AuthService>,
        api_service: std::sync::Arc<dyn ApiService>,
        settings_service: std::sync::Arc<
            dyn crate::infrastructure::services::settings::SettingsService,
        >,
    ) -> Self {
        Self {
            current_view: state::SettingsView::default(),
            security: state::SecurityState::default(),
            profile: state::ProfileState::default(),
            preferences: state::PreferencesState::default(),
            device_management_state: crate::domains::ui::views::settings::device_management::DeviceManagementState::default(),
            user_permissions: None,
            auth_service,
            api_service,
            settings_service,
        }
    }
}

pub struct SettingsDomain {
    // Settings fields moved directly here to avoid settings.state nesting
    pub current_view: state::SettingsView,
    pub security: state::SecurityState,
    pub profile: state::ProfileState,
    pub preferences: state::PreferencesState,
    pub device_management_state:
        crate::domains::ui::views::settings::device_management::DeviceManagementState,

    // References needed by settings domain
    pub user_permissions: Option<UserPermissions>,
    pub auth_service: std::sync::Arc<dyn crate::infrastructure::services::auth::AuthService>,
    pub api_service: std::sync::Arc<dyn ApiService>,
    pub settings_service:
        std::sync::Arc<dyn crate::infrastructure::services::settings::SettingsService>,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl SettingsDomain {
    pub fn new(state: SettingsDomainState) -> Self {
        Self {
            current_view: state.current_view,
            security: state.security,
            profile: state.profile,
            preferences: state.preferences,
            device_management_state: state.device_management_state,
            user_permissions: state.user_permissions,
            auth_service: state.auth_service,
            api_service: state.api_service,
            settings_service: state.settings_service,
        }
    }

    /// Update function - delegates to existing settings update logic
    pub fn update(&mut self, message: SettingsMessage) -> Task<DomainMessage> {
        // This will call the existing settings update function
        // For now, we return Task::none() to make it compile
        Task::none()
    }

    pub fn handle_event(&mut self, event: &CrossDomainEvent) -> Task<DomainMessage> {
        match event {
            CrossDomainEvent::UserAuthenticated(_user, permissions) => {
                self.user_permissions = Some(permissions.clone());
                Task::none()
            }
            _ => Task::none(),
        }
    }
}

impl std::fmt::Debug for SettingsDomainState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsDomainState")
            .field("current_view", &self.current_view)
            .field("security", &"<omitted>")
            .field("profile", &self.profile)
            .field("preferences", &self.preferences)
            .field("device_management_state", &"<omitted>")
            .field("user_permissions", &self.user_permissions)
            .field("auth_service", &"AuthService(..)")
            .field("api_service", &"ApiClientAdapter(..)")
            .field("settings_service", &"SettingsService(..)")
            .finish()
    }
}

impl std::fmt::Debug for SettingsDomain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsDomain")
            .field("current_view", &self.current_view)
            .field("security", &"<omitted>")
            .field("profile", &self.profile)
            .field("preferences", &self.preferences)
            .field("device_management_state", &"<omitted>")
            .field("user_permissions", &self.user_permissions)
            .field("auth_service", &"AuthService(..)")
            .field("api_service", &"ApiClientAdapter(..)")
            .field("settings_service", &"SettingsService(..)")
            .finish()
    }
}
