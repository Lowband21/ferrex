//! Legacy user settings view (pre-sidebar architecture).
//!
//! This module is kept behind the `legacy-settings` feature while the unified
//! settings sidebar is the primary UX. It can be used for reference, or
//! temporarily re-enabled during migrations.

use iced::{
    Element, Length,
    widget::{
        Space, button, column, container, row, scrollable, text, toggler,
    },
};

use ferrex_core::player_prelude::User;

use crate::{
    common::ui_utils::{Icon, icon_text},
    domains::ui::{
        messages::UiMessage, settings_ui::SettingsUiMessage,
        shell_ui::UiShellMessage, theme,
    },
    state::State,
};

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_main_settings<'a>(state: &'a State) -> Element<'a, UiMessage> {
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
            SettingsUiMessage::ShowSettings.into(),
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

/// Helper to create a settings section card.
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
