//! User profile settings
//!
//! Allows users to edit their display name, email, and avatar

use crate::domains::ui::messages::Message;
use crate::domains::ui::theme;
use crate::state_refactored::State;
use iced::widget::{Space, button, column, container, row, text, text_input};
use iced::{Element, Length};

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_user_profile<'a>(_state: &'a State) -> Element<'a, Message> {
    let content = column![
        text("Profile Settings")
            .size(24)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        Space::new().height(20),
        text("Display Name")
            .size(16)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        text_input("Enter display name", "").padding(10).size(16),
        Space::new().height(15),
        text("Email (Optional)")
            .size(16)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        text_input("Enter email", "").padding(10).size(16),
        Space::new().height(30),
        row![
            button("Cancel")
                .on_press(Message::BackToSettings)
                .style(theme::Button::Secondary.style())
                .padding([10, 20]),
            Space::new().width(10),
            button("Save Changes")
                .on_press(Message::NoOp) // TODO: Implement save
                .style(theme::Button::Primary.style())
                .padding([10, 20]),
        ],
    ]
    .spacing(10)
    .padding(20)
    .max_width(600);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}
