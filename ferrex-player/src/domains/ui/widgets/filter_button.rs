use crate::{
    common::ui_utils::icon_text_with_size,
    domains::ui::{messages::UiMessage, theme},
};
use iced::{
    Element, Length,
    widget::{Space, button, container, row, text},
};
use lucide_icons::Icon;

/// Creates a filter button that shows the number of active filters
pub fn filter_button<'a>(
    active_filter_count: usize,
    is_open: bool,
) -> Element<'a, UiMessage> {
    let icon = Icon::ListFilter;

    let button_content = if active_filter_count > 0 {
        row![
            icon_text_with_size(icon, 16.0),
            Space::new().width(6),
            text(active_filter_count.to_string())
                .size(14)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
        ]
        .align_y(iced::Alignment::Center)
    } else {
        row![icon_text_with_size(icon, 16.0)].align_y(iced::Alignment::Center)
    };

    let button_style = if is_open || active_filter_count > 0 {
        theme::Button::Primary.style()
    } else {
        theme::Button::Secondary.style()
    };

    container(
        button(
            container(button_content)
                .padding([0, 12])
                .center_y(Length::Fill),
        )
        .on_press(UiMessage::ToggleFilterPanel)
        .style(button_style)
        .height(Length::Fixed(36.0)),
    )
    .height(Length::Fixed(36.0))
    .align_y(iced::alignment::Vertical::Center)
    .into()
}
