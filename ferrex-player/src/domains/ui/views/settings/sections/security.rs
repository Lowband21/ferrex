//! Security section view
//!
//! Renders the security settings section content including:
//! - PIN: Set/change PIN for quick access
//! - Password: Change account password

use iced::widget::{Space, button, column, container, row, text};
use iced::{Alignment, Element, Length};

use crate::domains::ui::messages::UiMessage;
use crate::domains::ui::settings_ui::SettingsUiMessage;
use crate::domains::ui::theme::{self, MediaServerTheme};
use crate::infra::design_tokens::FontTokens;
use crate::state::State;

/// Render the security settings section
pub fn view_security_section<'a>(state: &'a State) -> Element<'a, UiMessage> {
    let has_pin = state.domains.settings.security.has_pin;
    let fonts = state.domains.ui.state.size_provider.font;

    let mut content = column![].spacing(24).padding(20).max_width(600);

    // Header
    content = content.push(
        text("Security")
            .size(fonts.title_lg)
            .color(MediaServerTheme::TEXT_PRIMARY),
    );

    // PIN subsection
    content = content.push(section_header("PIN", fonts));

    content = content.push(
        container(
            column![
                row![
                    column![
                        text("Quick Access PIN")
                            .size(fonts.body)
                            .color(MediaServerTheme::TEXT_PRIMARY),
                        text("Use a PIN for faster login on trusted devices")
                            .size(fonts.small)
                            .color(MediaServerTheme::TEXT_SUBDUED),
                    ]
                    .spacing(4)
                    .width(Length::Fill),
                    if has_pin {
                        text("Enabled")
                            .size(fonts.caption)
                            .color(MediaServerTheme::SUCCESS)
                    } else {
                        text("Not Set")
                            .size(fonts.caption)
                            .color(MediaServerTheme::TEXT_SUBDUED)
                    },
                ]
                .align_y(Alignment::Center)
                .spacing(16),
                Space::new().height(12),
                if has_pin {
                    button(text("Change PIN").size(fonts.caption))
                        .padding([10, 20])
                        .style(theme::Button::Secondary.style())
                        .on_press(SettingsUiMessage::ShowChangePin.into())
                } else {
                    button(text("Set PIN").size(fonts.caption))
                        .padding([10, 20])
                        .style(theme::Button::Primary.style())
                        .on_press(SettingsUiMessage::ShowSetPin.into())
                },
            ]
            .spacing(8),
        )
        .padding(16)
        .style(theme::Container::Card.style()),
    );

    // Password subsection
    content = content.push(section_header("Password", fonts));

    content = content.push(
        container(
            column![
                text("Account Password")
                    .size(fonts.body)
                    .color(MediaServerTheme::TEXT_PRIMARY),
                text("Change your account password")
                    .size(fonts.small)
                    .color(MediaServerTheme::TEXT_SUBDUED),
                Space::new().height(12),
                button(text("Change Password").size(fonts.caption))
                    .padding([10, 20])
                    .style(theme::Button::Secondary.style())
                    .on_press(SettingsUiMessage::ShowChangePassword.into()),
            ]
            .spacing(4),
        )
        .padding(16)
        .style(theme::Container::Card.style()),
    );

    container(content)
        .width(Length::Fill)
        .style(theme::Container::Default.style())
        .into()
}

/// Create a section header with divider
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
