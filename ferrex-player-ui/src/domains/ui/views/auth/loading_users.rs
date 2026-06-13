//! Loading users view

use super::components::{auth_card, auth_container};
use crate::common::messages::DomainMessage;
use crate::state::State;
use iced::{
    Element, Length,
    widget::{Space, column, container, text},
};

/// Shows a loading screen while fetching users
pub fn view_loading_users<'a>(state: &'a State) -> Element<'a, DomainMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;

    let content = auth_card(
        column![
            // Loading spinner placeholder
            container(
                text("⏳")
                    .size(fonts.display)
                    .align_x(iced::alignment::Horizontal::Center)
            )
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center),
            Space::new().height(Length::Fixed(20.0)),
            text("Loading users...")
                .size(fonts.body_lg)
                .align_x(iced::alignment::Horizontal::Center),
        ]
        .align_x(iced::Alignment::Center)
        .spacing(12),
    );

    auth_container(content).into()
}
