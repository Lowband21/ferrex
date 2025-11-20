//! User settings views
//!
//! This module provides views for user-specific settings and preferences
//! that don't require admin permissions.

use crate::messages::ui::Message;
use crate::state::State;
use crate::theme;
use iced::widget::{button, column, container, row, scrollable, text, toggler, Space};
use iced::{Element, Length};
use lucide_icons::{lucide_font_bytes, Icon};

pub mod device_management;
pub mod preferences;
pub mod profile;
pub mod security;

/// Helper function to create icon text
fn icon_text(icon: lucide_icons::Icon) -> text::Text<'static> {
    text(icon.unicode()).font(lucide_font()).size(20)
}

/// Get the lucide font
fn lucide_font() -> iced::Font {
    iced::Font::with_name("lucide")
}

/// User Settings View - Routes to appropriate settings subview
pub fn view_user_settings<'a>(state: &'a State) -> Element<'a, Message> {
    use crate::state::SettingsSubview;
    
    match &state.settings_subview {
        SettingsSubview::Main => view_main_settings(state),
        SettingsSubview::Profile => profile::view_user_profile(state),
        SettingsSubview::Preferences => preferences::view_user_preferences(state),
        SettingsSubview::Security => security::view_user_security(state),
        SettingsSubview::DeviceManagement => device_management::view_device_management(state),
    }
}

/// Main settings page
fn view_main_settings<'a>(state: &'a State) -> Element<'a, Message> {
    let current_user: Option<ferrex_core::user::User> = match state.auth_manager.as_ref() {
        Some(auth_manager) => {
            // We need to get the current user from auth manager
            // For now, use a placeholder until we implement the async getter
            None
        }
        None => None,
    };

    let mut content = column![].spacing(20).padding(20);

    // Header
    content = content.push(
        row![
            button(
                row![
                    icon_text(Icon::ChevronLeft)
                        .size(20),
                    Space::with_width(5),
                    text("Back")
                        .size(16)
                ]
                .align_y(iced::Alignment::Center)
            )
            .on_press(Message::NavigateHome)
            .style(theme::Button::Secondary.style())
            .padding([8, 16]),
            Space::with_width(20),
            text("User Settings")
                .size(28)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            Space::with_width(Length::Fill),
            if let Some(user) = &current_user {
                Element::new(text(format!("Logged in as: {}", user.display_name))
                    .size(16)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY))
            } else {
                Space::with_width(0).into()
            }
        ]
        .align_y(iced::Alignment::Center),
    );

    // Settings sections
    let sections = column![
        // Profile section
        create_settings_section(
            "👤",
            "Profile",
            "Manage your display name and avatar",
            Message::ShowUserProfile,
        ),
        // Preferences section
        create_settings_section(
            "⚙️",
            "Preferences",
            "Customize your viewing experience",
            Message::ShowUserPreferences,
        ),
        // Security section
        create_settings_section(
            "🔐",
            "Security",
            "Change PIN, manage devices",
            Message::ShowUserSecurity,
        ),
        // Device management section
        create_settings_section(
            "📱",
            "Device Management",
            "View and manage trusted devices",
            Message::ShowDeviceManagement,
        ),
        // Theme section (inline toggle)
        container(
            column![
                row![
                    text("🎨").size(24),
                    Space::with_width(15),
                    column![
                        text("Dark Mode")
                            .size(18)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                        text("Toggle between light and dark themes")
                            .size(14)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    ]
                    .spacing(5),
                    Space::with_width(Length::Fill),
                    toggler(false) // TODO: Connect to actual theme state
                        .on_toggle(|_| Message::NoOp), // TODO: Implement theme toggle
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
                    text("🔓").size(24),
                    Space::with_width(15),
                    column![
                        text("Auto-login")
                            .size(18)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                        text("Automatically log in on this device")
                            .size(14)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    ]
                    .spacing(5),
                    Space::with_width(Length::Fill),
                    toggler(state.auto_login_enabled)
                        .on_toggle(Message::ToggleAutoLogin),
                ]
                .align_y(iced::Alignment::Center),
            ]
            .padding(20),
        )
        .style(theme::Container::Card.style())
        .width(Length::Fill),
        Space::with_height(20),
        // Logout button
        button(
            row![
                icon_text(Icon::LogOut),
                Space::with_width(10),
                text("Logout").size(16),
            ]
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::Logout)
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
    icon: &'a str,
    title: &'a str,
    description: &'a str,
    message: Message,
) -> Element<'a, Message> {
    button(
        container(
            row![
                text(icon).size(24),
                Space::with_width(15),
                column![
                    text(title)
                        .size(18)
                        .color(theme::MediaServerTheme::TEXT_PRIMARY),
                    text(description)
                        .size(14)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                ]
                .spacing(5),
                Space::with_width(Length::Fill),
                icon_text(Icon::ChevronRight)
                    .size(20)
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