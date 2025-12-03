//! User settings views
//!
//! This module provides views for user settings management.
//!
//! ## Architecture
//!
//! There are two view systems:
//! - **Legacy**: `view_user_settings` - uses `SettingsView` enum for navigation
//! - **Unified**: `view_unified_settings` - uses sidebar with `SettingsSection` enum
//!
//! The unified view is the new architecture with a sidebar for navigation.

use iced::{
    Element, Length,
    widget::{
        Space, button, column, container, row, scrollable, text, toggler,
    },
};

use crate::{
    common::ui_utils::{Icon, icon_text},
    domains::{
        settings::state::{SettingsSection, SettingsView},
        ui::{
            messages::UiMessage, settings_ui::SettingsUiMessage,
            shell_ui::UiShellMessage, theme,
        },
    },
    state::State,
};
use ferrex_core::player_prelude::User;

pub mod device_management;
pub mod preferences;
pub mod profile;
pub mod sections;
pub mod security;
pub mod sidebar;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_user_settings<'a>(state: &'a State) -> Element<'a, UiMessage> {
    match &state.domains.settings.current_view {
        SettingsView::Main => view_main_settings(state),
        SettingsView::Profile => profile::view_user_profile(state),
        SettingsView::Preferences => preferences::view_user_preferences(state),
        SettingsView::Security => security::view_user_security(state),
        SettingsView::DeviceManagement => {
            device_management::view_device_management(state)
        }
    }
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
fn view_main_settings<'a>(state: &'a State) -> Element<'a, UiMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;

    // RUS-136: Get current user from auth domain state instead of auth_manager
    let current_user: Option<User> = match &state.domains.auth.state.auth_flow {
        crate::domains::auth::types::AuthenticationFlow::Authenticated {
            user,
            ..
        } => Some(user.clone()),
        _ => None,
    };

    let mut content = column![].spacing(20).padding(20);

    // Header
    content = content.push(
        row![
            button(
                row![
                    icon_text(Icon::ChevronLeft).size(fonts.subtitle),
                    Space::new().width(5),
                    text("Back").size(fonts.body)
                ]
                .align_y(iced::Alignment::Center)
            )
            .on_press(UiShellMessage::NavigateHome.into())
            .style(theme::Button::Secondary.style())
            .padding([8, 16]),
            Space::new().width(20),
            text("User Settings")
                .size(fonts.title_lg)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            Space::new().width(Length::Fill),
            if let Some(user) = &current_user {
                Element::new(
                    text(format!("Logged in as: {}", user.display_name))
                        .size(fonts.body)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                )
            } else {
                Space::new().width(0).into()
            }
        ]
        .align_y(iced::Alignment::Center),
    );

    // Settings sections
    let sections = column![
        // Profile section
        create_settings_section(
            fonts,
            "👤",
            "Profile",
            "Manage your display name and avatar",
            SettingsUiMessage::ShowUserProfile.into(),
        ),
        // Preferences section
        create_settings_section(
            fonts,
            "⚙️",
            "Preferences",
            "Customize your viewing experience",
            SettingsUiMessage::ShowUserPreferences.into(),
        ),
        // Security section
        create_settings_section(
            fonts,
            "🔐",
            "Security",
            "Change PIN, manage devices",
            SettingsUiMessage::ShowUserSecurity.into(),
        ),
        // Device management section
        create_settings_section(
            fonts,
            "📱",
            "Device Management",
            "View and manage trusted devices",
            SettingsUiMessage::ShowDeviceManagement.into(),
        ),
        // Theme section (inline toggle)
        container(
            column![
                row![
                    text("🎨").size(fonts.title),
                    Space::new().width(15),
                    column![
                        text("Dark Mode")
                            .size(fonts.body_lg)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                        text("Toggle between light and dark themes")
                            .size(fonts.caption)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    ]
                    .spacing(5),
                    Space::new().width(Length::Fill),
                    toggler(false) // TODO: Connect to actual theme state
                        .on_toggle(|_| UiMessage::NoOp), // TODO: Implement theme toggle
                ]
                .align_y(iced::Alignment::Center),
            ]
            .padding(20),
        )
        .style(theme::Container::Card.style())
        .width(Length::Fill),
        // Auto-login section (inline toggle)
        container(
            column![
                row![
                    text("🔓").size(fonts.title),
                    Space::new().width(15),
                    column![
                        text("Auto-login")
                            .size(fonts.body_lg)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                        text("Automatically log in on this device")
                            .size(fonts.caption)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    ]
                    .spacing(5),
                    Space::new().width(Length::Fill),
                    toggler(
                        state.domains.settings.preferences.auto_login_enabled
                    )
                    .on_toggle(|enabled| {
                        SettingsUiMessage::ToggleAutoLogin(enabled).into()
                    }),
                ]
                .align_y(iced::Alignment::Center),
            ]
            .padding(20),
        )
        .style(theme::Container::Card.style())
        .width(Length::Fill),
        Space::new().height(20),
        // Logout button
        button(
            row![
                icon_text(Icon::LogOut),
                Space::new().width(10),
                text("Logout").size(fonts.body),
            ]
            .align_y(iced::Alignment::Center),
        )
        .on_press(SettingsUiMessage::Logout.into())
        .style(theme::Button::Danger.style())
        .padding([12, 20])
        .width(Length::Fixed(150.0)),
    ]
    .spacing(15)
    .width(Length::FillPortion(2));

    content = content.push(sections);

    // Wrap in scrollable container
    scrollable(
        container(content)
            .width(Length::Fill)
            .max_width(800.0)
            .center_x(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Helper to create a settings section card
fn create_settings_section<'a>(
    fonts: &'a crate::infra::design_tokens::FontTokens,
    icon: &'a str,
    title: &'a str,
    description: &'a str,
    message: UiMessage,
) -> Element<'a, UiMessage> {
    button(
        container(
            row![
                text(icon).size(fonts.title),
                Space::new().width(15),
                column![
                    text(title)
                        .size(fonts.body_lg)
                        .color(theme::MediaServerTheme::TEXT_PRIMARY),
                    text(description)
                        .size(fonts.caption)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                ]
                .spacing(5),
                Space::new().width(Length::Fill),
                icon_text(Icon::ChevronRight)
                    .size(fonts.subtitle)
                    .color(theme::MediaServerTheme::TEXT_SUBDUED),
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding(20),
    )
    .on_press(message)
    .style(theme::Button::MediaCard.style())
    .width(Length::Fill)
    .into()
}

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
        SettingsSection::Devices => sections::view_devices_section(state),
        SettingsSection::Libraries => sections::view_libraries_section(state),
        SettingsSection::Users => sections::view_users_section(state),
        SettingsSection::Server => sections::view_server_section(state),
    };

    // Wrap in sidebar layout
    sidebar::build_settings_layout(state, current_section, content)
}
