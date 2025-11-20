use crate::{profiling::PROFILER, theme, Message, State};
use iced::{
    widget::{
        column, container, text, Space,
    },
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

pub fn view_library(state: &State) -> Element<Message> {
    PROFILER.start("view_library");

    let result = if state.loading {
        // Loading state
        container(
            column![
                text("Media Library")
                    .size(28)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                Space::with_height(Length::Fixed(100.0)),
                text("Loading library...")
                    .size(20)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            ]
            .spacing(20)
            .align_x(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .padding(20)
        .style(theme::Container::Default.style())
        .into()
    } else {
        // TODO: Move the rest from main.rs
        container(text("Library view - content to be moved"))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    };

    PROFILER.end("view_library");
    result
}
