use crate::{common::ui_utils::icon_text, domains::ui::messages::Message, domains::ui::theme};
use iced::{
    Element, Length,
    widget::{Space, button, column, container, row, text},
};
use lucide_icons::Icon;

pub fn view_video_error(error_message: &str) -> Element<'_, Message> {
    let mut content = column![].spacing(20).align_x(iced::Alignment::Center);

    // Back button
    content = content.push(
        container(
            button(
                row![icon_text(Icon::ArrowLeft), text(" Back to Library")]
                    .spacing(5)
                    .align_y(iced::Alignment::Center),
            )
            .on_press(Message::NavigateBack)
            .style(theme::Button::Secondary.style()),
        )
        .padding(20),
    );

    content = content.push(Space::new().height(Length::Fill));

    // Error message
    content = content.push(
        column![
            text("Video Error")
                .size(24)
                .color(theme::MediaServerTheme::ERROR),
            Space::new().height(20),
            container(
                text(error_message)
                    .size(16)
                    .color(theme::MediaServerTheme::ERROR)
            )
            .width(Length::Fixed(600.0))
            .align_x(iced::alignment::Horizontal::Center),
            Space::new().height(40),
            button("Back to Library")
                .on_press(Message::NavigateBack)
                .style(theme::Button::Primary.style()),
        ]
        .align_x(iced::Alignment::Center),
    );

    content = content.push(Space::new().height(Length::Fill));

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(theme::Container::Default.style())
        .into()
}
