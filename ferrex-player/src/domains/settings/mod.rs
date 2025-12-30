//! Settings management domain
//!
//! Contains all settings-related state and logic moved from the monolithic State.
//!
//! ## Architecture
//!
//! The settings domain is organized into sub-domains (sections), each handling
//! a specific category of settings:
//!
//! - **profile**: User profile (display name, email, avatar)
//! - **playback**: Playback settings (seeking, subtitles, quality)
//! - **display**: UI/display settings (theme, grid, poster, animation)
//! - **performance**: Performance tuning (scrolling, texture, prefetch)
//! - **security**: Security settings (PIN, password)
//! - **devices**: Device management (trusted devices)
//! - **libraries**: Library management (admin)
//! - **users**: User management (admin)
//! - **server**: Server settings (admin)
//!
//! Each section has its own state, messages, and update handlers for isolated
//! message routing.

pub mod messages;
pub mod sections;
pub mod state;
pub mod update;

// Re-export section types for convenience
pub use sections::{
    DevicesSection, DisplaySection, LibrariesSection, PerformanceSection,
    PlaybackSection, ProfileSection, SecuritySection, ServerSection,
    ThemeSection, UsersSection,
};

// Re-export the new SettingsSection enum
pub use state::SettingsSection;

use std::sync::Arc;

use crate::common::messages::{CrossDomainEvent, DomainMessage};
use crate::infra::services::api::ApiService;
use crate::infra::services::auth::AuthService;
use crate::infra::services::settings::SettingsService;
use ferrex_core::player_prelude::UserPermissions;
use iced::Task;

// Import section state types
use sections::display::DisplayState;
use sections::performance::PerformanceState;
use sections::playback::PlaybackState;
use sections::theme::ThemeState;

/// Settings domain state - moved from monolithic State
pub struct SettingsDomainState {
    /// Current section in the unified settings sidebar
    pub current_section: state::SettingsSection,
    pub security: state::SecurityState,
    pub profile: state::ProfileState,
    pub preferences: state::PreferencesState,
    pub device_management_state:
        crate::domains::ui::views::settings::device_management::DeviceManagementState,

    // New section states (from sections/*/state.rs)
    pub playback: PlaybackState,
    pub display: DisplayState,
    pub theme: ThemeState,
    pub performance: PerformanceState,

    // References needed by settings domain
    pub user_permissions: Option<UserPermissions>,
    pub auth_service: Arc<dyn AuthService>,
    pub api_service: Arc<dyn ApiService>,
    pub settings_service:
        Arc<dyn SettingsService>,
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
        auth_service: Arc<dyn AuthService>,
        api_service: Arc<dyn ApiService>,
        settings_service: Arc<dyn SettingsService>,
    ) -> Self {
        Self {
            current_section: state::SettingsSection::default(),
            security: state::SecurityState::default(),
            profile: state::ProfileState::default(),
            preferences: state::PreferencesState::default(),
            device_management_state: crate::domains::ui::views::settings::device_management::DeviceManagementState::default(),
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

pub struct SettingsDomain {
    // Settings fields moved directly here to avoid settings.state nesting
    /// Current section in the unified settings sidebar (new)
    pub current_section: state::SettingsSection,
    pub security: state::SecurityState,
    pub profile: state::ProfileState,
    pub preferences: state::PreferencesState,
    pub device_management_state:
        crate::domains::ui::views::settings::device_management::DeviceManagementState,

    // New section states (from sections/*/state.rs)
    pub playback: PlaybackState,
    pub display: DisplayState,
    pub theme: ThemeState,
    pub performance: PerformanceState,

    // References needed by settings domain
    pub user_permissions: Option<UserPermissions>,
    pub auth_service: Arc<dyn AuthService>,
    pub api_service: Arc<dyn ApiService>,
    pub settings_service:
        Arc<dyn SettingsService>,
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
            .field("api_service", &"ApiClientAdapter(..)")
            .field("settings_service", &"SettingsService(..)")
            .finish()
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
