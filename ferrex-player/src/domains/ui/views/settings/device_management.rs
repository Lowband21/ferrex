//! Device management UI
//!
//! Allows users to view and manage their authenticated devices

use crate::{
    common::ui_utils::icon_text,
    domains::ui::{messages::UiMessage, settings_ui::SettingsUiMessage, theme},
    state::State,
};
use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Border, Element, Length, Theme};
use lucide_icons::Icon;

/// Device management state
#[derive(Debug, Clone, Default)]
pub struct DeviceManagementState {
    pub devices: Vec<UserDevice>,
    pub loading: bool,
    pub error_message: Option<String>,
}

/// User device information
#[derive(Debug, Clone)]
pub struct UserDevice {
    pub device_id: String,
    pub device_name: String,
    pub device_type: String,
    pub last_active: chrono::DateTime<chrono::Utc>,
    pub is_current_device: bool,
    pub location: Option<String>,
}

/// Device management messages
#[derive(Debug, Clone)]
pub enum DeviceManagementMessage {
    LoadDevices,
    DevicesLoaded(Result<Vec<UserDevice>, String>),
    RevokeDevice(String),                  // device_id
    DeviceRevoked(Result<String, String>), // device_id or error
    RefreshDevices,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl DeviceManagementState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn has_error(&self) -> bool {
        self.error_message.is_some()
    }

    pub fn device_count(&self) -> usize {
        self.devices.len()
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
pub fn view_device_management<'a>(state: &'a State) -> Element<'a, UiMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;
    let device_state = &state.domains.settings.device_management_state;

    // Handle device list content
    let device_list_content = if device_state.is_loading() {
        create_loading_view(fonts)
    } else if let Some(error_msg) = &device_state.error_message {
        create_error_view(fonts, error_msg)
    } else if device_state.devices.is_empty() {
        create_empty_view(fonts)
    } else {
        create_device_list(fonts, &device_state.devices)
    };

    let content = column![
        // Header
        row![
            text("Device Management")
                .size(fonts.title_lg)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            Space::new().width(Length::Fill),
            button(
                row![
                    icon_text(Icon::RefreshCw).size(fonts.body),
                    Space::new().width(5),
                    text("Refresh").size(fonts.caption)
                ]
                .align_y(iced::Alignment::Center)
            )
            .on_press(SettingsUiMessage::RefreshDevices.into())
            .style(theme::Button::Secondary.style())
            .padding([6, 12]),
        ]
        .align_y(iced::Alignment::Center),

        Space::new().height(20),

        // Description
        text("Manage devices that can access your account. You can revoke access for any device except the current one.")
            .size(fonts.body)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),

        Space::new().height(20),

        // Device list
        device_list_content
    ]
    .spacing(10)
    .padding(20)
    .max_width(800);

    scrollable(
        container(content)
            .width(Length::Fill)
            .center_x(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Create loading view
fn create_loading_view<'a>(
    fonts: &'a crate::infra::design_tokens::FontTokens,
) -> Element<'a, UiMessage> {
    container(
        column![
            row![
                icon_text(Icon::Loader)
                    .size(fonts.title)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                Space::new().width(10),
                text("Loading devices...")
                    .size(fonts.body)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            ]
            .align_y(iced::Alignment::Center),
        ]
        .padding(40)
        .align_x(iced::Alignment::Center),
    )
    .style(theme::Container::Card.style())
    .width(Length::Fill)
    .into()
}

fn create_error_view<'a>(
    fonts: &'a crate::infra::design_tokens::FontTokens,
    error: &'a str,
) -> Element<'a, UiMessage> {
    container(
        column![
            row![
                icon_text(Icon::X)
                    .size(fonts.title)
                    .color(theme::MediaServerTheme::ERROR),
                Space::new().width(10),
                text("Failed to load devices")
                    .size(fonts.body)
                    .color(theme::MediaServerTheme::ERROR),
            ]
            .align_y(iced::Alignment::Center),
            Space::new().height(10),
            text(error)
                .size(fonts.caption)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            Space::new().height(15),
            button("Retry")
                .on_press(SettingsUiMessage::RefreshDevices.into())
                .style(theme::Button::Primary.style())
                .padding([8, 16]),
        ]
        .padding(40)
        .align_x(iced::Alignment::Center),
    )
    .style(theme::Container::Card.style())
    .width(Length::Fill)
    .into()
}

/// Create empty view
fn create_empty_view<'a>(
    fonts: &'a crate::infra::design_tokens::FontTokens,
) -> Element<'a, UiMessage> {
    container(
        column![
            icon_text(Icon::Smartphone)
                .size(fonts.display)
                .color(theme::MediaServerTheme::TEXT_SUBDUED),
            Space::new().height(15),
            text("No devices found")
                .size(fonts.body_lg)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            Space::new().height(5),
            text("This is unusual. Try refreshing the list.")
                .size(fonts.caption)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        ]
        .padding(40)
        .align_x(iced::Alignment::Center),
    )
    .style(theme::Container::Card.style())
    .width(Length::Fill)
    .into()
}

/// Create device list
fn create_device_list<'a>(
    fonts: &'a crate::infra::design_tokens::FontTokens,
    devices: &'a [UserDevice],
) -> Element<'a, UiMessage> {
    let mut device_elements = Vec::new();

    for device in devices {
        device_elements.push(create_device_card(fonts, device));
        device_elements.push(Space::new().height(10).into());
    }

    // Remove last spacer
    if !device_elements.is_empty() {
        device_elements.pop();
    }

    column(device_elements)
        .spacing(0)
        .width(Length::Fill)
        .into()
}

/// Create individual device card
fn create_device_card<'a>(
    fonts: &'a crate::infra::design_tokens::FontTokens,
    device: &'a UserDevice,
) -> Element<'a, UiMessage> {
    let device_icon = match device.device_type.as_str() {
        "desktop" => Icon::Monitor,
        "mobile" => Icon::Smartphone,
        "tablet" => Icon::Tablet,
        "tv" => Icon::Tv,
        _ => Icon::Laptop,
    };

    let last_active_text = format_last_active(&device.last_active);

    container(
        row![
            // Device icon and info
            row![
                icon_text(device_icon).size(fonts.display).color(
                    if device.is_current_device {
                        theme::MediaServerTheme::SUCCESS
                    } else {
                        theme::MediaServerTheme::TEXT_SECONDARY
                    }
                ),
                Space::new().width(15),
                column![
                    row![
                        text(&device.device_name)
                            .size(fonts.body_lg)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                        if device.is_current_device {
                            Element::from(Space::new().width(8))
                        } else {
                            Element::from(Space::new().width(0))
                        },
                        if device.is_current_device {
                            container(
                                text("Current Device")
                                    .size(fonts.small)
                                    .color(theme::MediaServerTheme::SUCCESS),
                            )
                            .style(|_theme: &Theme| container::Style {
                                background: Some(
                                    theme::MediaServerTheme::SUCCESS
                                        .scale_alpha(0.2)
                                        .into(),
                                ),
                                border: Border {
                                    color: theme::MediaServerTheme::SUCCESS,
                                    width: 1.0,
                                    radius: 4.0.into(),
                                },
                                text_color: Some(
                                    theme::MediaServerTheme::SUCCESS,
                                ),
                                ..Default::default()
                            })
                            .padding([2, 8])
                            .into()
                        } else {
                            Element::from(Space::new().width(0))
                        }
                    ]
                    .align_y(iced::Alignment::Center),
                    Space::new().height(2),
                    text(format!(
                        "{} • {}",
                        device.device_type, last_active_text
                    ))
                    .size(fonts.caption)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    if let Some(location) = &device.location {
                        Element::new(
                            text(format!("📍 {}", location))
                                .size(fonts.small)
                                .color(theme::MediaServerTheme::TEXT_SUBDUED),
                        )
                    } else {
                        Space::new().height(0).into()
                    }
                ]
                .spacing(2),
            ]
            .align_y(iced::Alignment::Center),
            Space::new().width(Length::Fill),
            // Actions
            if device.is_current_device {
                Element::from(
                    container(
                        text("Current")
                            .size(fonts.caption)
                            .color(theme::MediaServerTheme::TEXT_SUBDUED),
                    )
                    .padding([8, 16]),
                )
            } else {
                Element::from(
                    button(
                        row![
                            icon_text(Icon::X).size(fonts.caption),
                            Space::new().width(5),
                            text("Revoke").size(fonts.caption)
                        ]
                        .align_y(iced::Alignment::Center),
                    )
                    .on_press(
                        SettingsUiMessage::RevokeDevice(
                            device.device_id.clone(),
                        )
                        .into(),
                    )
                    .style(theme::Button::Danger.style())
                    .padding([6, 12]),
                )
            }
        ]
        .align_y(iced::Alignment::Center)
        .padding(20),
    )
    .style(theme::Container::Card.style())
    .width(Length::Fill)
    .into()
}

/// Format last active time in a human-readable way
fn format_last_active(last_active: &chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let duration = now.signed_duration_since(*last_active);

    if duration.num_minutes() < 1 {
        "Just now".to_string()
    } else if duration.num_minutes() < 60 {
        format!("{} minutes ago", duration.num_minutes())
    } else if duration.num_hours() < 24 {
        format!("{} hours ago", duration.num_hours())
    } else if duration.num_days() < 7 {
        format!("{} days ago", duration.num_days())
    } else if duration.num_weeks() < 4 {
        format!("{} weeks ago", duration.num_weeks())
    } else {
        last_active.format("%B %d, %Y").to_string()
    }
}
