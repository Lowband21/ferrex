//! Profile section view
//!
//! Renders the profile settings section content including:
//! - Account: Display name, email, avatar
//! - Authentication: Logout, switch user

use iced::widget::{Space, button, column, container, row, text, text_input};
use iced::{Alignment, Element, Length};

use crate::domains::ui::messages::UiMessage;
use crate::domains::ui::settings_ui::SettingsUiMessage;
use crate::domains::ui::theme::{self, MediaServerTheme};
use crate::infra::design_tokens::FontTokens;
use crate::state::State;

/// Render the profile settings section
pub fn view_profile_section<'a>(state: &'a State) -> Element<'a, UiMessage> {
    let profile = &state.domains.settings.profile;
    let fonts = state.domains.ui.state.size_provider.font;

    let mut content = column![].spacing(24).padding(20).max_width(600);

    // Header
    content = content.push(
        text("Profile")
            .size(fonts.title_lg)
            .color(MediaServerTheme::TEXT_PRIMARY),
    );

    // Account subsection
    content = content.push(section_header("Account", fonts));

    // Display name field
    content = content.push(
        column![
            field_label("Display Name", fonts),
            text_input("Enter your display name", &profile.display_name)
                .padding(12)
                .size(fonts.caption)
                .on_input(|_| UiMessage::NoOp), // TODO: Wire to ProfileMessage::UpdateDisplayName
        ]
        .spacing(6),
    );

    // Email field
    content = content.push(
        column![
            field_label("Email (optional)", fonts),
            text_input("Enter your email", &profile.email)
                .padding(12)
                .size(fonts.caption)
                .on_input(|_| UiMessage::NoOp), // TODO: Wire to ProfileMessage::UpdateEmail
        ]
        .spacing(6),
    );

    // Success/Error messages
    if let Some(success) = &profile.success_message {
        content = content.push(
            container(
                text(success)
                    .size(fonts.caption)
                    .color(MediaServerTheme::SUCCESS),
            )
            .padding([8, 12])
            .style(theme::Container::SuccessBox.style()),
        );
    }

    if let Some(error) = &profile.error {
        content = content.push(
            container(
                text(error)
                    .size(fonts.caption)
                    .color(MediaServerTheme::ERROR),
            )
            .padding([8, 12])
            .style(theme::Container::ErrorBox.style()),
        );
    }

    // Save button
    content = content.push(
        row![
            button(
                text("Save Changes")
                    .size(fonts.caption)
                    .color(MediaServerTheme::TEXT_PRIMARY),
            )
            .padding([10, 20])
            .style(theme::Button::Primary.style())
            .on_press(UiMessage::NoOp), // TODO: Wire to ProfileMessage::SaveProfile
        ]
        .align_y(Alignment::Center),
    );

    content = content.push(Space::new().height(16));

    // Session subsection
    content = content.push(section_header("Session", fonts));

    // Current user info
    let current_user_display = match &state.domains.auth.state.auth_flow {
        crate::domains::auth::types::AuthenticationFlow::Authenticated {
            user,
            ..
        } => user.display_name.clone(),
        _ => "Unknown".to_string(),
    };

    content = content.push(
        row![
            text("Logged in as: ")
                .size(fonts.caption)
                .color(MediaServerTheme::TEXT_SECONDARY),
            text(current_user_display)
                .size(fonts.caption)
                .color(MediaServerTheme::TEXT_PRIMARY),
        ]
        .spacing(4),
    );

    // Logout button
    content = content.push(
        button(
            row![text("Log Out").size(fonts.caption),]
                .align_y(Alignment::Center)
                .spacing(8),
        )
        .padding([10, 20])
        .style(theme::Button::Destructive.style())
        .on_press(SettingsUiMessage::Logout.into()),
    );

    container(content)
        .width(Length::Fill)
        .style(theme::Container::Default.style())
        .into()
}

/// Create a section header
fn section_header(title: &str, fonts: FontTokens) -> Element<'_, UiMessage> {
    column![
        text(title)
            .size(fonts.body_lg)
            .color(MediaServerTheme::TEXT_PRIMARY),
        container(Space::new().height(1))
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(iced::Background::Color(
                    MediaServerTheme::BORDER_COLOR
                )),
                ..Default::default()
            }),
    ]
    .spacing(8)
    .into()
}

/// Create a field label
fn field_label(label: &str, fonts: FontTokens) -> Element<'_, UiMessage> {
    text(label)
        .size(fonts.caption)
        .color(MediaServerTheme::TEXT_SECONDARY)
        .into()
}
