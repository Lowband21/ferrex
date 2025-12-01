//! User profile settings
//!
//! Allows users to edit their display name, email, and avatar

use crate::domains::ui::{
    messages::UiMessage, settings_ui::SettingsUiMessage, theme,
};
use crate::state::State;
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
pub fn view_user_profile<'a>(state: &'a State) -> Element<'a, UiMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;

    let content = column![
        text("Profile Settings")
            .size(fonts.title)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        Space::new().height(20),
        text("Display Name")
            .size(fonts.body)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        text_input("Enter display name", "")
            .padding(10)
            .size(fonts.body),
        Space::new().height(15),
        text("Email (Optional)")
            .size(fonts.body)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        text_input("Enter email", "").padding(10).size(fonts.body),
        Space::new().height(30),
        row![
            button("Cancel")
                .on_press(SettingsUiMessage::BackToSettings.into())
                .style(theme::Button::Secondary.style())
                .padding([10, 20]),
            Space::new().width(10),
            button("Save Changes")
                .on_press(UiMessage::NoOp) // TODO: Implement save
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
