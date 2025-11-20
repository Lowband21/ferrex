use crate::domains::ui::{
    SortBy, SortOrder,
    messages::UiMessage,
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
    item_count: usize,
) -> Element<'a, UiMessage> {
    let sort_item = build_sort_menu(current_sort, current_order);
    let filter_item =
        build_filter_menu(active_filter_count, is_filter_panel_open);

    let menu_bar = MenuBar::new(vec![sort_item, filter_item])
        .spacing(0.0)
        .height(Length::Fill)
        .close_on_item_click(true);

    let count_button = button(
        container(
            row![text(item_count.to_string()).size(14),]
                .align_y(Alignment::Center),
        )
        .padding(0)
        .center_y(Length::Fill),
    )
    .on_press(UiMessage::NoOp)
    .style(theme::Button::HeaderMenuSecondary.style())
    .height(Length::Fill);

    container(row![
        menu_bar,
        Space::new().width(Length::Fill),
        count_button,
    ])
    .align_y(Alignment::Center)
    .height(Length::Fill)
    .into()
}

fn build_sort_menu(
    current_sort: SortBy,
    current_order: SortOrder,
) -> Item<'static, UiMessage, iced::Theme, iced::Renderer> {
    let summary_label = sort_summary_label(current_sort, current_order);

    let mut sort_items = Vec::with_capacity(SortOption::OPTIONS.len() + 1);

    sort_items.push(menu_item(
        "Toggle sort order",
        Some(Icon::ArrowUpDown),
        UiMessage::ToggleSortOrder,
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
            UiMessage::SetSortBy(option.value),
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
        .padding(0)
        .center_y(Length::Fill),
    )
    .style(theme::Button::HeaderMenuSecondary.style())
    .height(Length::Fill);

    Item::with_menu(
        sort_button,
        Menu::new(sort_items)
            .max_width(220.0)
            .spacing(0.0)
            .offset(0.0),
    )
}

fn build_filter_menu(
    active_filter_count: usize,
    is_filter_panel_open: bool,
) -> Item<'static, UiMessage, iced::Theme, iced::Renderer> {
    let filter_summary = if active_filter_count > 0 {
        format!("{} active", active_filter_count)
    } else {
        "None".to_string()
    };

    let button_style = if is_filter_panel_open || active_filter_count > 0 {
        theme::Button::HeaderMenuPrimary.style()
    } else {
        theme::Button::HeaderMenuSecondary.style()
    };

    let filter_items = vec![
        menu_item("Open filters", Some(Icon::ListFilter), UiMessage::NoOp),
        menu_item("Clear filters", Some(Icon::CircleX), UiMessage::NoOp),
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
        .padding(0)
        .center_y(Length::Fill),
    )
    .style(button_style)
    .height(Length::Fill);

    Item::with_menu(
        filter_button,
        Menu::new(filter_items)
            .max_width(200.0)
            .spacing(0.0)
            .offset(0.0),
    )
}

fn menu_item(
    label: &'static str,
    icon: Option<Icon>,
    message: UiMessage,
) -> Item<'static, UiMessage, iced::Theme, iced::Renderer> {
    Item::new(
        button(menu_row(label, icon))
            .on_press(message)
            .style(theme::Button::HeaderMenuSecondary.style()),
    )
}

fn menu_row(
    label: &'static str,
    icon: Option<Icon>,
) -> container::Container<'static, UiMessage> {
    let mut content: Row<'static, UiMessage> =
        Row::new().align_y(Alignment::Center);

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
