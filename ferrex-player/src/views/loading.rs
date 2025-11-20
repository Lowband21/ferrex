use crate::{theme, Message};
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

pub fn view_loading_video(url: &str) -> Element<Message> {
    let mut content = column![].spacing(20).align_x(iced::Alignment::Center);

    // Back button
    content = content.push(
        container(
            button(
                row![icon_text(Icon::ArrowLeft), text(" Back to Library")]
                    .spacing(5)
                    .align_y(iced::Alignment::Center),
            )
            .on_press(Message::BackToLibrary)
            .style(theme::Button::Secondary.style()),
        )
        .padding(20),
    );

    content = content.push(Space::with_height(Length::Fill));

    // Loading indicator
    content = content.push(
        column![
            text("Loading Video")
                .size(24)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            Space::with_height(10),
            text("Please wait...")
                .size(16)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            Space::with_height(20),
            text(url)
                .size(12)
                .color(theme::MediaServerTheme::TEXT_DIMMED),
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
