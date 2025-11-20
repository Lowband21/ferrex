//! Loading users view

use super::components::{auth_card, auth_container};
use crate::common::messages::DomainMessage;
use iced::{
    widget::{column, container, text, Space},
    Element, Length,
};

/// Shows a loading screen while fetching users
pub fn view_loading_users<'a>() -> Element<'a, DomainMessage> {
    let content = auth_card(
        column![
            // Loading spinner placeholder
            container(
                text("⏳")
                    .size(48)
                    .align_x(iced::alignment::Horizontal::Center)
            )
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center),
            Space::with_height(Length::Fixed(20.0)),
            text("Loading users...")
                .size(18)
                .align_x(iced::alignment::Horizontal::Center),
        ]
        .align_x(iced::Alignment::Center)
        .spacing(12),
    );

    auth_container(content).into()
}
