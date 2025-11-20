//! Security settings
//!
//! Allows users to manage their PIN, password, and trusted devices

use crate::domains::ui::messages::Message;
use crate::domains::ui::theme;
use crate::state_refactored::State;
use iced::widget::{Space, button, column, container, text};
use iced::{Element, Length};

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_user_security<'a>(_state: &'a State) -> Element<'a, Message> {
    let content = column![
        text("Security Settings")
            .size(24)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        Space::with_height(20),
        // PIN management
        container(
            column![
                text("PIN Settings")
                    .size(20)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                Space::with_height(10),
                text("Your PIN is used for quick access on trusted devices")
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                Space::with_height(15),
                button("Change PIN")
                    .on_press(Message::NoOp) // TODO: Implement PIN change
                    .style(theme::Button::Primary.style())
                    .padding([10, 20]),
            ]
            .spacing(5)
            .padding(20),
        )
        .style(theme::Container::Card.style())
        .width(Length::Fill),
        // Password management
        container(
            column![
                text("Password")
                    .size(20)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                Space::with_height(10),
                text("Change your account password")
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                Space::with_height(15),
                button("Change Password")
                    .on_press(Message::NoOp) // TODO: Implement password change
                    .style(theme::Button::Secondary.style())
                    .padding([10, 20]),
            ]
            .spacing(5)
            .padding(20),
        )
        .style(theme::Container::Card.style())
        .width(Length::Fill),
        // Trusted devices
        container(
            column![
                text("Trusted Devices")
                    .size(20)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                Space::with_height(10),
                text("Devices that can use PIN login")
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                Space::with_height(15),
                // TODO: List trusted devices
                text("This device: Trusted ✓")
                    .size(16)
                    .color(theme::MediaServerTheme::SUCCESS),
                Space::with_height(10),
                button("Manage Devices")
                    .on_press(Message::ShowDeviceManagement)
                    .style(theme::Button::Secondary.style())
                    .padding([10, 20]),
            ]
            .spacing(5)
            .padding(20),
        )
        .style(theme::Container::Card.style())
        .width(Length::Fill),
        Space::with_height(30),
        button("Back")
            .on_press(Message::BackToSettings)
            .style(theme::Button::Secondary.style())
            .padding([10, 20]),
    ]
    .spacing(15)
    .padding(20)
    .max_width(600);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .into()
}
