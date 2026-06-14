//! User settings views
//!
//! This module provides views for user settings management.
//!
//! ## Architecture
//!
//! There are two view systems:
//! - **Legacy**: feature-gated via `legacy` module
//! - **Unified**: `view_unified_settings` - uses sidebar with `SettingsSection` enum
//!
//! The unified view is the new architecture with a sidebar for navigation.

use iced::Element;

use crate::{
    domains::{
        settings::state::SettingsSection,
        ui::{
            messages::UiMessage,
            views::{
                admin::view_library_management,
                settings::device_management::view_device_management,
            },
        },
    },
    state::State,
};

pub mod device_management;
pub mod sections;
pub mod sidebar;

#[cfg(feature = "legacy-settings")]
pub mod legacy;

// =============================================================================
// Unified Settings View (New Architecture)
// =============================================================================

/// Render the unified settings view with sidebar navigation
///
/// This is the new settings architecture that uses a sidebar for navigation
/// between sections. Each section is rendered by its corresponding view function
/// in the `sections` module.
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_unified_settings<'a>(state: &'a State) -> Element<'a, UiMessage> {
    let current_section = state.domains.settings.current_section;

    // Render the content for the current section
    let content = match current_section {
        SettingsSection::Profile => sections::view_profile_section(state),
        SettingsSection::Playback => sections::view_playback_section(state),
        SettingsSection::Display => sections::view_display_section(state),
        SettingsSection::Theme => sections::view_theme_section(state),
        SettingsSection::Performance => {
            sections::view_performance_section(state)
        }
        SettingsSection::Security => sections::view_security_section(state),
        SettingsSection::Devices => view_device_management(state),
        SettingsSection::Libraries => view_library_management(state), //sections::view_libraries_section(state),
        SettingsSection::Users => sections::view_users_section(state),
        SettingsSection::Server => sections::view_server_section(state),
    };

    // Wrap in sidebar layout
    sidebar::build_settings_layout(state, current_section, content)
}
