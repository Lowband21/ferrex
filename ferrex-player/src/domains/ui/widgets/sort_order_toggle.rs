use crate::{
    common::ui_utils::icon_text_with_size,
    domains::ui::{
        SortOrder, library_ui::LibraryUiMessage, messages::UiMessage, theme,
    },
};
use iced::{
    Element, Length,
    widget::{button, container},
};
use lucide_icons::Icon;

/// Creates a sort order toggle button with consistent styling
pub fn sort_order_toggle<'a>(
    current_order: SortOrder,
) -> Element<'a, UiMessage> {
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
            container(icon_text_with_size(icon, 16.0))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
        .on_press(LibraryUiMessage::ToggleSortOrder.into())
        .style(theme::Button::HeaderIcon.style())
        .width(Length::Fixed(36.0))
        .height(Length::Fixed(36.0)),
    )
    .height(Length::Fixed(36.0))
    .align_y(iced::alignment::Vertical::Center)
    .into()
}
