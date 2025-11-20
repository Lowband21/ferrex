use crate::{domains::ui::messages::Message, domains::ui::theme};
use iced::{
    widget::{button, column, container, row, text, Space},
    Element, Length,
};
use lucide_icons::Icon;

// Helper function to create icon text
fn icon_text(icon: Icon) -> text::Text<'static> {
    text(icon.unicode()).font(lucide_font()).size(20)
}

// Get the lucide font
fn lucide_font() -> iced::Font {
    iced::Font::with_name("lucide")
}

pub fn view_video_error(error_message: &str) -> Element<Message> {
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

    content = content.push(Space::with_height(Length::Fill));

    // Error message
    content = content.push(
        column![
            text("Video Error")
                .size(24)
                .color(theme::MediaServerTheme::ERROR),
            Space::with_height(20),
            container(
                text(error_message)
                    .size(16)
                    .color(theme::MediaServerTheme::ERROR)
            )
            .width(Length::Fixed(600.0))
            .align_x(iced::alignment::Horizontal::Center),
            Space::with_height(40),
            button("Back to Library")
                .on_press(Message::NavigateBack)
                .style(theme::Button::Primary.style()),
        ]
        .align_x(iced::Alignment::Center),
    );

    content = content.push(Space::with_height(Length::Fill));

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(theme::Container::Default.style())
        .into()
}
