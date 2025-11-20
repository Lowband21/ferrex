use crate::domains::ui::{
    SortBy, SortOrder,
    messages::Message,
    theme::{self, FerrexTheme},
    widgets::sort_dropdown::SortOption,
};
use iced::widget::Row;
use iced::{
    Alignment, Element, Length,
    widget::{Space, button, container, row, text},
};
use iced_aw::menu::{Item, Menu, MenuBar};
use lucide_icons::Icon;

/// Build the menu bar used in the library controls subheader.
pub fn library_sort_filter_menu<'a>(
    current_sort: SortBy,
    current_order: SortOrder,
    active_filter_count: usize,
    is_filter_panel_open: bool,
) -> Element<'a, Message> {
    let sort_item = build_sort_menu(current_sort, current_order);
    let filter_item = build_filter_menu(active_filter_count, is_filter_panel_open);

    let menu_bar = MenuBar::new(vec![sort_item, filter_item])
        .spacing(8.0)
        .padding([0.0, 4.0])
        .height(Length::Fixed(36.0))
        .close_on_item_click(true);

    container(row![menu_bar, Space::new().width(Length::Fill)])
        .align_y(Alignment::Center)
        .height(Length::Fixed(36.0))
        .into()
}

fn build_sort_menu(
    current_sort: SortBy,
    current_order: SortOrder,
) -> Item<'static, Message, iced::Theme, iced::Renderer> {
    let summary_label = sort_summary_label(current_sort, current_order);

    let mut sort_items = Vec::with_capacity(SortOption::OPTIONS.len() + 1);

    sort_items.push(menu_item(
        "Toggle sort order",
        Some(Icon::ArrowUpDown),
        Message::ToggleSortOrder,
    ));

    for option in SortOption::OPTIONS {
        let icon = if option.value == current_sort {
            Some(Icon::Check)
        } else {
            None
        };

        sort_items.push(menu_item(
            option.label,
            icon,
            Message::SetSortBy(option.value),
        ));
    }

    let sort_button = button(
        container(
            row![
                text("Sort").size(14),
                Space::new().width(6),
                text(summary_label)
                    .size(14)
                    .color(FerrexTheme::SubduedText.text_color()),
            ]
            .align_y(Alignment::Center),
        )
        .padding([0, 12])
        .height(Length::Fill),
    )
    .style(theme::Button::Secondary.style())
    .height(Length::Fixed(36.0));

    Item::with_menu(
        sort_button,
        Menu::new(sort_items)
            .max_width(220.0)
            .spacing(4.0)
            .offset(8.0),
    )
}

fn build_filter_menu(
    active_filter_count: usize,
    is_filter_panel_open: bool,
) -> Item<'static, Message, iced::Theme, iced::Renderer> {
    let filter_summary = if active_filter_count > 0 {
        format!("{} active", active_filter_count)
    } else {
        "None".to_string()
    };

    let button_style = if is_filter_panel_open || active_filter_count > 0 {
        theme::Button::Primary.style()
    } else {
        theme::Button::Secondary.style()
    };

    let filter_items = vec![
        menu_item("Open filters", Some(Icon::ListFilter), Message::NoOp),
        menu_item("Clear filters", Some(Icon::CircleX), Message::NoOp),
    ];

    let filter_button = button(
        container(
            row![
                text("Filters").size(14),
                Space::new().width(6),
                text(filter_summary)
                    .size(14)
                    .color(FerrexTheme::SubduedText.text_color()),
            ]
            .align_y(Alignment::Center),
        )
        .padding([0, 12])
        .height(Length::Fill),
    )
    .style(button_style)
    .height(Length::Fixed(36.0));

    Item::with_menu(
        filter_button,
        Menu::new(filter_items)
            .max_width(200.0)
            .spacing(4.0)
            .offset(8.0),
    )
}

fn menu_item(
    label: &'static str,
    icon: Option<Icon>,
    message: Message,
) -> Item<'static, Message, iced::Theme, iced::Renderer> {
    Item::new(
        button(menu_row(label, icon))
            .on_press(message)
            .style(theme::Button::Secondary.style()),
    )
}

fn menu_row(label: &'static str, icon: Option<Icon>) -> container::Container<'static, Message> {
    let mut content: Row<'static, Message> = Row::new().align_y(Alignment::Center);

    if let Some(icon) = icon {
        content = content
            .push(
                text(icon.unicode())
                    .font(lucide_font())
                    .size(16)
                    .color(FerrexTheme::SubduedText.text_color()),
            )
            .push(Space::new().width(8));
    }

    content = content.push(text(label).size(14));

    container(content).padding([4, 12]).width(Length::Fill)
}

fn sort_summary_label(sort_by: SortBy, order: SortOrder) -> String {
    let sort_label = SortOption::OPTIONS
        .iter()
        .find(|opt| opt.value == sort_by)
        .map(|opt| opt.label)
        .unwrap_or("Custom");

    let order_suffix = match order {
        SortOrder::Ascending => "↑",
        SortOrder::Descending => "↓",
    };

    format!("{} {}", sort_label, order_suffix)
}

fn lucide_font() -> iced::Font {
    iced::Font::with_name("lucide")
}
