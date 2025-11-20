use iced::border::Radius;
use iced::{
    Alignment, Background, Border, Element, Length, Shadow,
    widget::{Space, button, column, container, pick_list, row, text},
};
use lucide_icons::Icon;

use crate::{
    common::ui_utils::icon_text_with_size,
    domains::ui::{
        messages::Message,
        theme::{self, MediaServerTheme},
    },
    state_refactored::State,
};

const GENRES_PER_ROW: usize = 4;

pub fn library_filter_panel<'a>(state: &'a State) -> Element<'a, Message> {
    let ui_state = &state.domains.ui.state;

    let mut genre_groups = column![].spacing(8).width(Length::Fill);

    for chunk in ferrex_core::UiGenre::all().chunks(GENRES_PER_ROW) {
        let mut chunk_row = row![].spacing(8).align_y(Alignment::Center);

        for genre in chunk {
            let is_selected = ui_state.selected_genres.iter().any(|x| x == genre);
            let chip = button(text(genre.to_string()).size(14).color(if is_selected {
                MediaServerTheme::TEXT_PRIMARY
            } else {
                MediaServerTheme::TEXT_SECONDARY
            }))
            .padding([6, 12])
            .style(filter_chip_style(is_selected))
            .on_press(Message::ToggleFilterGenre(*genre));
            chunk_row = chunk_row.push(chip);
        }

        genre_groups = genre_groups.push(chunk_row);
    }

    // Decade dropdown
    let decades = ferrex_core::UiDecade::all();
    let selected_decade = ui_state.selected_decade;
    let decade_pick = pick_list(decades, selected_decade, |opt| {
        Message::SetFilterDecade(opt)
    })
    .placeholder("Decade")
    .width(Length::Fixed(140.0));

    // Resolution dropdown
    let resolutions = ferrex_core::UiResolution::all();
    let res_pick = pick_list(resolutions, Some(ui_state.selected_resolution), |opt| {
        Message::SetFilterResolution(opt)
    })
    .placeholder("Resolution")
    .width(Length::Fixed(140.0));

    // Watch status dropdown
    let watch_statuses = ferrex_core::UiWatchStatus::all();
    let ws_pick = pick_list(
        watch_statuses,
        Some(ui_state.selected_watch_status),
        Message::SetFilterWatchStatus,
    )
    .placeholder("Watch Status")
    .width(Length::Fixed(160.0));

    let selects = row![
        column![
            text("Decade")
                .size(12)
                .color(MediaServerTheme::TEXT_SECONDARY),
            decade_pick,
        ]
        .spacing(4),
        column![
            text("Resolution")
                .size(12)
                .color(MediaServerTheme::TEXT_SECONDARY),
            res_pick,
        ]
        .spacing(4),
        column![
            text("Watch Status")
                .size(12)
                .color(MediaServerTheme::TEXT_SECONDARY),
            ws_pick,
        ]
        .spacing(4),
    ]
    .spacing(12)
    .align_y(Alignment::Start);

    let actions = row![
        button(text("Clear"))
            .on_press(Message::ClearFilters)
            .style(theme::Button::Text.style()),
        button(text("Apply"))
            .on_press(Message::ApplyFilters)
            .style(theme::Button::Primary.style()),
    ]
    .spacing(12);

    let header = row![
        text("Filters")
            .size(18)
            .color(MediaServerTheme::TEXT_PRIMARY),
        Space::with_width(Length::Fill),
        button(icon_text_with_size(Icon::X, 16.0))
            .padding([6, 8])
            .style(theme::Button::Icon.style())
            .on_press(Message::ToggleFilterPanel),
    ]
    .align_y(Alignment::Center);

    let content = column![
        header,
        column![
            text("Genres")
                .size(12)
                .color(MediaServerTheme::TEXT_SECONDARY),
            genre_groups,
        ]
        .spacing(6),
        selects,
        row![Space::with_width(Length::Fill), actions]
            .align_y(Alignment::Center)
            .spacing(12),
    ]
    .spacing(16);

    row![
        Space::with_width(Length::Fill),
        container(content)
            .padding(16)
            .width(Length::Fixed(420.0))
            .style(theme::Container::Card.style()),
    ]
    .align_y(Alignment::Start)
    .width(Length::Fill)
    .into()
}

fn filter_chip_style(is_selected: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_, status| {
        let (background, border_color, text_color) = if is_selected {
            match status {
                button::Status::Hovered | button::Status::Pressed => (
                    MediaServerTheme::ACCENT_BLUE_HOVER,
                    MediaServerTheme::ACCENT_BLUE,
                    MediaServerTheme::TEXT_PRIMARY,
                ),
                _ => (
                    MediaServerTheme::ACCENT_BLUE,
                    MediaServerTheme::ACCENT_BLUE,
                    MediaServerTheme::TEXT_PRIMARY,
                ),
            }
        } else {
            match status {
                button::Status::Hovered | button::Status::Pressed => (
                    MediaServerTheme::CARD_HOVER,
                    MediaServerTheme::ACCENT_BLUE,
                    MediaServerTheme::TEXT_PRIMARY,
                ),
                _ => (
                    MediaServerTheme::CARD_BG,
                    MediaServerTheme::BORDER_COLOR,
                    MediaServerTheme::TEXT_SECONDARY,
                ),
            }
        };

        button::Style {
            text_color,
            background: Some(Background::Color(background)),
            border: Border {
                color: border_color,
                width: 1.0,
                radius: Radius::from(16.0),
            },
            shadow: Shadow::default(),
            snap: false,
        }
    }
}
