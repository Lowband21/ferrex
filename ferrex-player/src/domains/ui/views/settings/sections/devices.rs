//! Devices section view
//!
//! Renders the device management section content including:
//! - List of trusted devices
//! - Revoke device access

use iced::widget::{Space, button, column, container, row, text};
use iced::{Alignment, Element, Length};

use crate::domains::ui::messages::UiMessage;
use crate::domains::ui::settings_ui::SettingsUiMessage;
use crate::domains::ui::theme::{self, MediaServerTheme};
use crate::infra::design_tokens::FontTokens;
use crate::state::State;

/// Render the devices settings section
pub fn view_devices_section<'a>(state: &'a State) -> Element<'a, UiMessage> {
    let device_state = &state.domains.settings.device_management_state;
    let fonts = state.domains.ui.state.size_provider.font;

    let mut content = column![].spacing(24).padding(20).max_width(700);

    // Header
    content = content.push(
        text("Devices")
            .size(fonts.title_lg)
            .color(MediaServerTheme::TEXT_PRIMARY),
    );

    // Trusted Devices subsection
    content = content.push(section_header("Trusted Devices", fonts));

    content = content.push(
        text("Devices that can use PIN login for quick access")
            .size(fonts.small)
            .color(MediaServerTheme::TEXT_SUBDUED),
    );

    // Loading state
    if device_state.loading {
        content = content.push(
            container(
                text("Loading devices...")
                    .size(fonts.caption)
                    .color(MediaServerTheme::TEXT_SUBDUED),
            )
            .padding(20)
            .width(Length::Fill)
            .center_x(Length::Fill),
        );
    } else if let Some(error) = &device_state.error_message {
        // Error state
        content = content.push(
            container(
                column![
                    text("Failed to load devices")
                        .size(fonts.caption)
                        .color(MediaServerTheme::ERROR),
                    text(error)
                        .size(fonts.small)
                        .color(MediaServerTheme::TEXT_SUBDUED),
                    Space::new().height(12),
                    button(text("Retry").size(fonts.caption))
                        .padding([8, 16])
                        .style(theme::Button::Secondary.style())
                        .on_press(SettingsUiMessage::RefreshDevices.into()),
                ]
                .spacing(8),
            )
            .padding(16)
            .style(theme::Container::ErrorBox.style()),
        );
    } else if device_state.devices.is_empty() {
        // Empty state
        content = content.push(
            container(
                text("No trusted devices found")
                    .size(fonts.caption)
                    .color(MediaServerTheme::TEXT_SUBDUED),
            )
            .padding(20)
            .width(Length::Fill)
            .center_x(Length::Fill),
        );
    } else {
        // Device list
        for device in &device_state.devices {
            let device_card = container(
                row![
                    // Device info
                    column![
                        row![
                            text(&device.device_name)
                                .size(fonts.body)
                                .color(MediaServerTheme::TEXT_PRIMARY),
                            if device.is_current_device {
                                text(" (This device)")
                                    .size(fonts.small)
                                    .color(MediaServerTheme::SUCCESS)
                            } else {
                                text("").size(fonts.small)
                            },
                        ]
                        .spacing(4),
                        text(&device.device_type)
                            .size(fonts.small)
                            .color(MediaServerTheme::TEXT_SUBDUED),
                        text(format!(
                            "Last active: {}",
                            device.last_active.format("%b %d, %Y at %H:%M")
                        ))
                        .size(fonts.small)
                        .color(MediaServerTheme::TEXT_SUBDUED),
                    ]
                    .spacing(4)
                    .width(Length::Fill),
                    // Revoke button (not for current device)
                    if !device.is_current_device {
                        button(text("Revoke").size(fonts.small))
                            .padding([6, 12])
                            .style(theme::Button::Destructive.style())
                            .on_press(
                                SettingsUiMessage::RevokeDevice(
                                    device.device_id.clone(),
                                )
                                .into(),
                            )
                    } else {
                        button(text("Current").size(fonts.small))
                            .padding([6, 12])
                            .style(theme::Button::Disabled.style())
                    },
                ]
                .align_y(Alignment::Center)
                .spacing(16),
            )
            .padding(16)
            .style(theme::Container::Card.style());

            content = content.push(device_card);
        }
    }

    // Refresh button
    content = content.push(Space::new().height(8));
    content = content.push(
        button(text("Refresh Devices").size(fonts.caption))
            .padding([10, 20])
            .style(theme::Button::Secondary.style())
            .on_press(SettingsUiMessage::RefreshDevices.into()),
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
