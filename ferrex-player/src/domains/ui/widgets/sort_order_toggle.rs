use crate::domains::ui::{messages::Message, theme, SortOrder};
use iced::{
    widget::{button, container, text},
    Element, Length,
};
use lucide_icons::Icon;

/// Creates a sort order toggle button with consistent styling
pub fn sort_order_toggle<'a>(current_order: SortOrder) -> Element<'a, Message> {
    let icon = match current_order {
        SortOrder::Ascending => Icon::ArrowUp,
        SortOrder::Descending => Icon::ArrowDown,
    };

    let tooltip = match current_order {
        SortOrder::Ascending => "Sort ascending (click for descending)",
        SortOrder::Descending => "Sort descending (click for ascending)",
    };

    container(
        button(
            container(icon_text(icon))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
        .on_press(Message::ToggleSortOrder)
        .style(theme::Button::HeaderIcon.style())
        .width(Length::Fixed(36.0))
        .height(Length::Fixed(36.0)),
    )
    .height(Length::Fixed(36.0))
    .align_y(iced::alignment::Vertical::Center)
    .into()
}

/// Helper function to create icon text with consistent font
fn icon_text(icon: Icon) -> text::Text<'static> {
    text(icon.unicode()).font(lucide_font()).size(16)
}

/// Get the lucide font
fn lucide_font() -> iced::Font {
    iced::Font::with_name("lucide")
}