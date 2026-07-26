//! Search overlay and detached window component

use iced::widget::{
    Id as TextInputId, Space, Stack, button, column, container, row,
    scrollable, text, text_input,
};
use iced::{Alignment, Color, Element, Length, Padding, Theme};

use crate::common::messages::DomainMessage;
use crate::domains::search::{
    keyboard::{TenFootKeyboardKey, TenFootKeyboardState},
    messages::SearchMessage,
    types::{SearchMode, SearchResponse},
};
use crate::domains::ui::messages::UiMessage;
use crate::domains::ui::shell_ui::UiShellMessage;
use crate::domains::ui::theme::{Button as ButtonStyle, MediaServerTheme};
use crate::domains::ui::widgets::image_for;
use crate::domains::ui::windows::focus::SEARCH_WINDOW_INPUT_ID;
use crate::infra::api_types::{ImageSize, Media, Priority};
use crate::infra::constants::layout;
use crate::infra::theme::{accent, accent_glow};
use crate::state::State;
use ferrex_core::query::types::SearchField;
use lucide_icons::Icon;
use uuid::Uuid;

type Message = DomainMessage;

#[derive(Clone, Copy)]
enum SearchSurface {
    Overlay,
    Detached,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_search_overlay(state: &State) -> Option<Element<'_, Message>> {
    if state.interface_mode.is_tenfoot() {
        return Some(view_tenfoot_search_overlay(state));
    }

    let panel = view_search_panel(state, SearchSurface::Overlay);

    let backdrop = button(
        container(Space::new().width(Length::Fill).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .on_press(DomainMessage::Ui(UiShellMessage::CloseSearch.into()))
    .style(|_theme: &Theme, _status: button::Status| button::Style {
        background: Some(iced::Background::Color(Color::from_rgba(
            0.0, 0.0, 0.0, 0.55,
        ))),
        border: iced::Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 0.0.into(),
        },
        shadow: iced::Shadow::default(),
        text_color: Color::TRANSPARENT,
        snap: false,
    })
    .width(Length::Fill)
    .height(Length::Fill);

    let panel_container = container(panel)
        .width(Length::Fixed(layout::search::WINDOW_WIDTH))
        .height(Length::Fixed(layout::search::WINDOW_HEIGHT));

    let positioned_panel = column![
        Space::new().height(Length::Fixed(
            layout::header::HEIGHT + layout::search::WINDOW_VERTICAL_OFFSET
        )),
        container(panel_container)
            .width(Length::Fill)
            .center_x(Length::Fill),
        Space::new().height(Length::Fill),
    ]
    .width(Length::Fill)
    .height(Length::Fill);

    Some(
        Stack::new()
            .push(backdrop)
            .push(positioned_panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    )
}

pub fn view_search_window(state: &State) -> Element<'_, Message> {
    view_search_panel(state, SearchSurface::Detached)
}

fn view_tenfoot_search_overlay(state: &State) -> Element<'_, Message> {
    let search_state = &state.domains.search.state;
    let result_count = search_state.results.len();
    let subtitle = if search_state.query.is_empty() {
        "Use the on-screen keyboard below or a connected keyboard to search your library.".to_string()
    } else if search_state.is_searching {
        format!("Searching for \"{}\"…", search_state.query)
    } else if search_state.total_results > 0 {
        format!(
            "{} result{} available",
            search_state.total_results,
            if search_state.total_results == 1 {
                ""
            } else {
                "s"
            }
        )
    } else {
        "No matching items yet.".to_string()
    };

    let header = row![
        column![
            text("FERREX SEARCH")
                .size(18)
                .color(MediaServerTheme::ACCENT),
            text("Find something to watch")
                .size(48)
                .color(MediaServerTheme::TEXT_PRIMARY),
            text(subtitle)
                .size(22)
                .color(MediaServerTheme::TEXT_SECONDARY),
        ]
        .spacing(6)
        .width(Length::Fill),
        button(text("Close · Esc").size(22))
            .on_press(DomainMessage::Ui(UiShellMessage::CloseSearch.into()))
            .style(ButtonStyle::Secondary.style())
            .height(Length::Fixed(58.0)),
    ]
    .spacing(28)
    .align_y(Alignment::Center);

    let keyboard_open = search_state.tenfoot_keyboard.is_open();
    let keyboard_toggle = if keyboard_open {
        button(text("Hide keys").size(20))
            .on_press(DomainMessage::Search(SearchMessage::HideTenFootKeyboard))
    } else {
        button(text("Keyboard").size(20))
            .on_press(DomainMessage::Search(SearchMessage::ShowTenFootKeyboard))
    };
    let search_button_message = if keyboard_open {
        DomainMessage::Search(SearchMessage::TenFootKeyboardPress(
            TenFootKeyboardKey::Search,
        ))
    } else {
        DomainMessage::Ui(UiShellMessage::ExecuteSearch.into())
    };

    let input_row = row![
        container(text("/").size(34).color(MediaServerTheme::ACCENT))
            .width(Length::Fixed(44.0))
            .center_x(Length::Fixed(44.0))
            .center_y(Length::Fixed(64.0)),
        text_input(
            "Type a title, show, season, or episode…",
            &search_state.query
        )
        .id(TextInputId::new(SEARCH_WINDOW_INPUT_ID))
        .on_input(|value| DomainMessage::Ui(
            UiShellMessage::UpdateSearchQuery(value).into(),
        ))
        .on_submit(DomainMessage::Ui(UiShellMessage::ExecuteSearch.into()))
        .padding(Padding::from([14.0, 18.0]))
        .size(30)
        .width(Length::Fill),
        keyboard_toggle
            .style(ButtonStyle::Secondary.style())
            .width(Length::Fixed(142.0))
            .height(Length::Fixed(64.0)),
        button(text("Search").size(22))
            .on_press(search_button_message)
            .style(ButtonStyle::Primary.style())
            .width(Length::Fixed(150.0))
            .height(Length::Fixed(64.0)),
    ]
    .spacing(16)
    .align_y(Alignment::Center);

    let input_panel = container(input_row)
        .width(Length::Fill)
        .padding(Padding::from([16.0, 18.0]))
        .style(|_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(Color::from_rgba(
                0.10, 0.10, 0.14, 0.96,
            ))),
            border: iced::Border {
                color: accent(),
                width: 2.0,
                radius: 18.0.into(),
            },
            shadow: iced::Shadow {
                color: accent_glow(),
                offset: iced::Vector::default(),
                blur_radius: 18.0,
            },
            ..Default::default()
        });

    let guidance = if keyboard_open {
        row![
            tenfoot_guidance_chip("D-pad moves keys"),
            tenfoot_guidance_chip("Enter adds key"),
            tenfoot_guidance_chip("Search/Done browses results"),
            tenfoot_guidance_chip("Esc hides keys"),
            Space::new().width(Length::Fill),
            text("Remote/D-pad text entry is available here")
                .size(18)
                .color(MediaServerTheme::TEXT_DIMMED),
        ]
    } else {
        row![
            tenfoot_guidance_chip("↑/↓ choose result"),
            tenfoot_guidance_chip("Enter opens details"),
            tenfoot_guidance_chip("←/→ show keyboard"),
            tenfoot_guidance_chip("Esc closes search"),
            Space::new().width(Length::Fill),
            text("Connected hardware keyboard typing still works")
                .size(18)
                .color(MediaServerTheme::TEXT_DIMMED),
        ]
    }
    .spacing(10)
    .align_y(Alignment::Center);

    let results = tenfoot_results_content(state);
    let keyboard_panel = tenfoot_keyboard_panel(&search_state.tenfoot_keyboard);
    let result_header = row![
        text("Results")
            .size(28)
            .color(MediaServerTheme::TEXT_PRIMARY),
        Space::new().width(Length::Fill),
        text(format!("{} shown", result_count))
            .size(20)
            .color(MediaServerTheme::TEXT_SECONDARY),
    ]
    .align_y(Alignment::Center);

    let lower_content: Element<'_, Message> = if keyboard_open {
        row![
            container(results)
                .width(Length::FillPortion(3))
                .height(Length::Fill),
            container(keyboard_panel)
                .width(Length::FillPortion(2))
                .height(Length::Fill),
        ]
        .spacing(18)
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
    } else {
        column![results, keyboard_panel]
            .spacing(14)
            .height(Length::Fill)
            .width(Length::Fill)
            .into()
    };

    let content =
        column![header, input_panel, guidance, result_header, lower_content]
            .spacing(14)
            .padding(Padding::from([36.0, 56.0]))
            .width(Length::Fill)
            .height(Length::Fill);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_theme: &Theme| container::Style {
            text_color: Some(MediaServerTheme::TEXT_PRIMARY),
            background: Some(iced::Background::Color(Color::from_rgba(
                0.02, 0.02, 0.03, 0.98,
            ))),
            border: iced::Border::default(),
            shadow: iced::Shadow::default(),
            snap: false,
        })
        .into()
}

fn tenfoot_results_content(state: &State) -> Element<'_, Message> {
    let search_state = &state.domains.search.state;

    if search_state.query.is_empty()
        && search_state.results.is_empty()
        && !search_state.is_searching
    {
        return tenfoot_status_card(
            "Start typing to search",
            "Use the on-screen keyboard or a connected keyboard. Search/Done hides the keyboard so ↑/↓ and Enter can browse results.",
        );
    }

    if search_state.is_searching && search_state.results.is_empty() {
        return tenfoot_status_card(
            "Searching…",
            format!(
                "Looking for \"{}\" in your Ferrex library.",
                search_state.query
            ),
        );
    }

    if let Some(error) = &search_state.error {
        return tenfoot_status_card(
            "Search unavailable",
            format!("Ferrex could not complete this search: {error}"),
        );
    }

    if search_state.results.is_empty() {
        return tenfoot_status_card(
            format!("No results for \"{}\"", search_state.query),
            "Try another title, show name, season, or episode.",
        );
    }

    let mut results_column = column![].spacing(12);
    if search_state.is_searching {
        results_column = results_column.push(
            container(
                row![
                    text("Updating results…")
                        .size(22)
                        .color(MediaServerTheme::TEXT_SECONDARY),
                    Space::new().width(Length::Fill),
                    text("⌛").size(24),
                ]
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([12.0, 20.0]))
            .width(Length::Fill),
        );
    }

    for (index, result) in search_state.results.iter().enumerate() {
        results_column = results_column.push(view_tenfoot_search_result(
            result,
            search_state.selected_index == Some(index),
            index,
        ));
    }

    if search_state.total_results > 0 {
        results_column = results_column.push(
            container(
                text(format!(
                    "Showing {} of {} results",
                    search_state.results.len(),
                    search_state.total_results
                ))
                .size(18)
                .color(MediaServerTheme::TEXT_DIMMED),
            )
            .padding(Padding::from([10.0, 18.0]))
            .width(Length::Fill)
            .center_x(Length::Fill),
        );
    }

    container(
        scrollable(results_column)
            .id(crate::domains::search::types::SEARCH_RESULTS_SCROLL_ID)
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::default(),
            ))
            .height(Length::Fill)
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(|_theme: &Theme| container::Style {
        background: Some(iced::Background::Color(Color::from_rgba(
            0.06, 0.06, 0.09, 0.92,
        ))),
        border: iced::Border {
            color: Color::from_rgba(0.24, 0.29, 0.39, 0.8),
            width: 1.0,
            radius: 18.0.into(),
        },
        shadow: iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            offset: iced::Vector::new(0.0, 12.0),
            blur_radius: 24.0,
        },
        ..Default::default()
    })
    .into()
}

fn tenfoot_keyboard_panel(
    keyboard: &TenFootKeyboardState,
) -> Element<'_, Message> {
    if !keyboard.is_open() {
        return container(
            row![
                column![
                    text("Keyboard hidden")
                        .size(20)
                        .color(MediaServerTheme::TEXT_PRIMARY),
                    text("Use ↑/↓ to choose results, Enter to open one, or press ←/→ to bring the on-screen keyboard back.")
                        .size(17)
                        .color(MediaServerTheme::TEXT_SECONDARY),
                ]
                .spacing(4)
                .width(Length::Fill),
                button(text("Show keyboard").size(20))
                    .on_press(DomainMessage::Search(
                        SearchMessage::ShowTenFootKeyboard,
                    ))
                    .style(ButtonStyle::Secondary.style())
                    .height(Length::Fixed(52.0)),
            ]
            .spacing(18)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding(Padding::from([14.0, 18.0]))
        .style(tenfoot_keyboard_panel_style)
        .into();
    }

    let mut key_rows = column![].spacing(8).width(Length::Fill);
    for row_keys in TenFootKeyboardState::rows() {
        let mut key_row = row![].spacing(8).width(Length::Fill);
        for key in row_keys.iter().copied() {
            key_row = key_row.push(tenfoot_keyboard_key_button(
                key,
                keyboard.is_focused(key),
            ));
        }
        key_rows = key_rows.push(key_row);
    }

    container(
        column![
            row![
                text("On-screen keyboard")
                    .size(22)
                    .color(MediaServerTheme::TEXT_PRIMARY),
                Space::new().width(Length::Fill),
                text("A-Z · 0-9 · Space · Backspace · Clear")
                    .size(17)
                    .color(MediaServerTheme::TEXT_SECONDARY),
            ]
            .align_y(Alignment::Center),
            key_rows,
        ]
        .spacing(10),
    )
    .width(Length::Fill)
    .padding(Padding::from([14.0, 18.0]))
    .style(tenfoot_keyboard_panel_style)
    .into()
}

fn tenfoot_keyboard_key_button(
    key: TenFootKeyboardKey,
    is_focused: bool,
) -> Element<'static, Message> {
    let label = key.label();
    let key_size = if key.is_action() { 19 } else { 23 };

    button(
        container(text(label).size(key_size))
            .width(Length::Fill)
            .height(Length::Fixed(46.0))
            .center_x(Length::Fill)
            .center_y(Length::Fixed(46.0)),
    )
    .on_press(DomainMessage::Search(SearchMessage::TenFootKeyboardPress(
        key,
    )))
    .style(tenfoot_keyboard_key_style(is_focused, key.is_action()))
    .width(Length::FillPortion(tenfoot_keyboard_key_portion(key)))
    .height(Length::Fixed(48.0))
    .into()
}

fn tenfoot_keyboard_key_portion(key: TenFootKeyboardKey) -> u16 {
    match key {
        TenFootKeyboardKey::Space => 3,
        TenFootKeyboardKey::Backspace
        | TenFootKeyboardKey::Clear
        | TenFootKeyboardKey::Search
        | TenFootKeyboardKey::Done => 2,
        TenFootKeyboardKey::Character(_) => 1,
    }
}

fn tenfoot_keyboard_key_style(
    is_focused: bool,
    is_action: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style + Copy {
    move |_theme: &Theme, status: button::Status| {
        let is_hot =
            matches!(status, button::Status::Hovered | button::Status::Pressed);
        let background = if is_focused {
            Color::from_rgba(0.20, 0.34, 0.52, 0.98)
        } else if is_hot {
            Color::from_rgba(0.18, 0.22, 0.31, 0.96)
        } else if is_action {
            Color::from_rgba(0.14, 0.15, 0.22, 0.94)
        } else {
            Color::from_rgba(0.10, 0.10, 0.15, 0.94)
        };
        let border_color = if is_focused {
            accent()
        } else if is_action {
            Color::from_rgba(0.38, 0.45, 0.60, 0.82)
        } else {
            Color::from_rgba(0.27, 0.33, 0.45, 0.72)
        };

        button::Style {
            background: Some(iced::Background::Color(background)),
            border: iced::Border {
                color: border_color,
                width: if is_focused { 2.5 } else { 1.0 },
                radius: 12.0.into(),
            },
            shadow: if is_focused {
                iced::Shadow {
                    color: accent_glow(),
                    offset: iced::Vector::default(),
                    blur_radius: 16.0,
                }
            } else {
                iced::Shadow::default()
            },
            text_color: if is_focused {
                Color::WHITE
            } else {
                MediaServerTheme::TEXT_PRIMARY
            },
            snap: false,
        }
    }
}

fn tenfoot_keyboard_panel_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(Color::from_rgba(
            0.055, 0.055, 0.085, 0.94,
        ))),
        border: iced::Border {
            color: Color::from_rgba(0.26, 0.34, 0.48, 0.82),
            width: 1.0,
            radius: 18.0.into(),
        },
        shadow: iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.30),
            offset: iced::Vector::new(0.0, 10.0),
            blur_radius: 22.0,
        },
        text_color: Some(MediaServerTheme::TEXT_PRIMARY),
        snap: false,
    }
}

fn tenfoot_status_card(
    title: impl Into<String>,
    body: impl Into<String>,
) -> Element<'static, Message> {
    container(
        column![
            text(title.into())
                .size(34)
                .color(MediaServerTheme::TEXT_PRIMARY),
            text(body.into())
                .size(22)
                .color(MediaServerTheme::TEXT_SECONDARY),
        ]
        .spacing(12)
        .align_x(Alignment::Center)
        .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .padding(Padding::from([32.0, 42.0]))
    .style(|_theme: &Theme| container::Style {
        background: Some(iced::Background::Color(Color::from_rgba(
            0.06, 0.06, 0.09, 0.92,
        ))),
        border: iced::Border {
            color: Color::from_rgba(0.24, 0.29, 0.39, 0.8),
            width: 1.0,
            radius: 18.0.into(),
        },
        shadow: iced::Shadow::default(),
        ..Default::default()
    })
    .into()
}

fn view_tenfoot_search_result(
    result: &SearchResponse,
    is_selected: bool,
    index: usize,
) -> Element<'_, Message> {
    let media_type = media_type_label(&result.media_ref);
    let year = result
        .year
        .or_else(|| media_year(&result.media_ref))
        .map(|year| year.to_string());
    let subtitle = result.subtitle.as_deref().unwrap_or(media_type).to_string();

    let mut metadata = row![tenfoot_metadata_badge(media_type.to_string())]
        .spacing(10)
        .align_y(Alignment::Center);
    if let Some(year) = year {
        metadata = metadata.push(tenfoot_metadata_badge(year));
    }
    metadata = metadata.push(tenfoot_metadata_badge(
        match_field_label(result.match_field).to_string(),
    ));

    let trailing = if is_selected {
        "Enter".to_string()
    } else {
        format!("{:02}", index + 1)
    };

    let row_content = row![
        tenfoot_result_art(&result.media_ref),
        column![
            text(&result.title)
                .size(if is_selected { 31 } else { 29 })
                .color(MediaServerTheme::TEXT_PRIMARY),
            text(subtitle)
                .size(21)
                .color(MediaServerTheme::TEXT_SECONDARY),
            metadata,
        ]
        .spacing(8)
        .width(Length::Fill),
        container(text(trailing).size(22).color(if is_selected {
            Color::WHITE
        } else {
            MediaServerTheme::TEXT_DIMMED
        }))
        .width(Length::Fixed(92.0))
        .center_x(Length::Fixed(92.0)),
    ]
    .spacing(20)
    .align_y(Alignment::Center);

    let background = if is_selected {
        Color::from_rgba(0.18, 0.21, 0.30, 0.98)
    } else {
        Color::from_rgba(0.10, 0.10, 0.14, 0.92)
    };
    let border_color = if is_selected {
        accent()
    } else {
        Color::from_rgba(0.24, 0.29, 0.39, 0.62)
    };

    button(
        container(row_content)
            .padding(Padding::from([12.0, 18.0]))
            .width(Length::Fill)
            .height(Length::Fixed(128.0))
            .style(move |_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(background)),
                border: iced::Border {
                    color: border_color,
                    width: if is_selected { 2.0 } else { 1.0 },
                    radius: 16.0.into(),
                },
                shadow: if is_selected {
                    iced::Shadow {
                        color: accent_glow(),
                        offset: iced::Vector::default(),
                        blur_radius: 18.0,
                    }
                } else {
                    iced::Shadow::default()
                },
                ..Default::default()
            }),
    )
    .on_press(DomainMessage::Search(
        crate::domains::search::messages::SearchMessage::SelectResult(
            result.media_ref.clone(),
        ),
    ))
    .style(ButtonStyle::Text.style())
    .width(Length::Fill)
    .into()
}

fn tenfoot_result_art(media_ref: &Media) -> Element<'_, Message> {
    let art = media_art(media_ref);
    let image: Element<'_, UiMessage> = image_for(art.media_uuid)
        .iid(art.image_iid)
        .skip_request(art.image_iid.is_none())
        .request_size(art.image_size)
        .display_size(art.width, art.height)
        .radius(8.0)
        .placeholder(Icon::Film)
        .priority(Priority::Visible)
        .tight_bounds()
        .no_animation()
        .into();

    container(image.map(DomainMessage::Ui))
        .width(Length::Fixed(116.0))
        .height(Length::Fixed(112.0))
        .center_x(Length::Fixed(116.0))
        .center_y(Length::Fixed(112.0))
        .style(|_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(Color::from_rgba(
                0.03, 0.03, 0.05, 0.75,
            ))),
            border: iced::Border {
                color: Color::from_rgba(0.32, 0.36, 0.48, 0.55),
                width: 1.0,
                radius: 12.0.into(),
            },
            shadow: iced::Shadow::default(),
            ..Default::default()
        })
        .into()
}

struct MediaArt {
    media_uuid: Uuid,
    image_iid: Option<Uuid>,
    image_size: ImageSize,
    width: f32,
    height: f32,
}

fn media_art(media_ref: &Media) -> MediaArt {
    match media_ref {
        Media::Movie(movie) => MediaArt {
            media_uuid: movie.id.to_uuid(),
            image_iid: movie.details.primary_poster_iid,
            image_size: ImageSize::poster(),
            width: 74.0,
            height: 112.0,
        },
        Media::Series(series) => MediaArt {
            media_uuid: series.id.to_uuid(),
            image_iid: series.details.primary_poster_iid,
            image_size: ImageSize::poster(),
            width: 74.0,
            height: 112.0,
        },
        Media::Season(season) => MediaArt {
            media_uuid: season.id.to_uuid(),
            image_iid: season.details.primary_poster_iid,
            image_size: ImageSize::poster(),
            width: 74.0,
            height: 112.0,
        },
        Media::Episode(episode) => MediaArt {
            media_uuid: episode.id.to_uuid(),
            image_iid: episode.details.primary_still_iid,
            image_size: ImageSize::thumbnail(),
            width: 112.0,
            height: 64.0,
        },
    }
}

fn media_type_label(media_ref: &Media) -> &'static str {
    match media_ref {
        Media::Movie(_) => "Movie",
        Media::Series(_) => "Series",
        Media::Season(_) => "Season",
        Media::Episode(_) => "Episode",
    }
}

fn media_year(media_ref: &Media) -> Option<i32> {
    match media_ref {
        Media::Movie(movie) => movie.details.release_date.as_deref(),
        Media::Series(series) => series.details.first_air_date.as_deref(),
        Media::Season(season) => season.details.air_date.as_deref(),
        Media::Episode(episode) => episode.details.air_date.as_deref(),
    }
    .and_then(year_from_date)
}

fn year_from_date(date: &str) -> Option<i32> {
    date.get(0..4)?.parse().ok()
}

fn tenfoot_guidance_chip(label: &'static str) -> Element<'static, Message> {
    container(text(label).size(18).color(MediaServerTheme::TEXT_PRIMARY))
        .padding(Padding::from([8.0, 12.0]))
        .style(|_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(Color::from_rgba(
                0.14, 0.14, 0.20, 0.92,
            ))),
            border: iced::Border {
                color: Color::from_rgba(0.32, 0.42, 0.58, 0.7),
                width: 1.0,
                radius: 999.0.into(),
            },
            text_color: Some(MediaServerTheme::TEXT_PRIMARY),
            ..Default::default()
        })
        .into()
}

fn tenfoot_metadata_badge(label: String) -> Element<'static, Message> {
    container(text(label).size(17).color(MediaServerTheme::TEXT_SECONDARY))
        .padding(Padding::from([5.0, 10.0]))
        .style(|_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(Color::from_rgba(
                0.16, 0.16, 0.23, 0.82,
            ))),
            border: iced::Border {
                color: Color::from_rgba(0.34, 0.44, 0.58, 0.62),
                width: 1.0,
                radius: 999.0.into(),
            },
            text_color: Some(MediaServerTheme::TEXT_SECONDARY),
            ..Default::default()
        })
        .into()
}

fn view_search_panel(
    state: &State,
    surface: SearchSurface,
) -> Element<'_, Message> {
    let search_state = &state.domains.search.state;

    let title = if search_state.query.is_empty() {
        "Search your library".to_owned()
    } else {
        format!("Results for \"{}\"", search_state.query)
    };

    let displayed_results = search_state.results.len();

    let subtitle = if search_state.is_searching {
        "Searching...".to_owned()
    } else if search_state.results.is_empty() && !search_state.query.is_empty()
    {
        "No matches yet - try a different phrase".to_owned()
    } else if search_state.total_results > 0 {
        format!(
            "Showing {} of {} results",
            displayed_results, search_state.total_results
        )
    } else {
        "Find movies, shows, and episodes instantly".to_owned()
    };

    let mut header_row = row![
        container(text("🔍").size(28))
            .width(Length::Fixed(36.0))
            .center_x(Length::Fixed(36.0))
            .center_y(Length::Fixed(36.0)),
        column![
            text(title).size(22),
            text(subtitle)
                .size(14)
                .color(Color::from_rgb(0.7, 0.7, 0.75)),
        ]
        .spacing(4)
        .width(Length::Fill),
    ]
    .spacing(12.0)
    .align_y(Alignment::Center);

    if matches!(surface, SearchSurface::Overlay)
        && crate::domains::ui::search_surface::detached_search_allowed(state)
    {
        header_row = header_row.push(
            button(text("Pop out").size(14))
                .on_press(DomainMessage::Ui(
                    UiShellMessage::PopOutSearch.into(),
                ))
                .style(ButtonStyle::Secondary.style()),
        );
    }

    let header = container(header_row)
        .padding(Padding::from([12.0, 16.0]))
        .width(Length::Fill)
        .style(|_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(
                MediaServerTheme::SOFT_GREY_DARK,
            )),
            border: iced::Border {
                color: Color::from_rgb(0.2, 0.2, 0.25),
                width: 1.0,
                radius: 8.0.into(),
            },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 12.0,
            },
            ..Default::default()
        });

    let input_row = row![
        text_input("Search...", &search_state.query)
            .id(TextInputId::new(SEARCH_WINDOW_INPUT_ID))
            .on_input(|value| DomainMessage::Ui(
                UiShellMessage::UpdateSearchQuery(value).into(),
            ))
            .on_submit(DomainMessage::Ui(UiShellMessage::ExecuteSearch.into(),))
            .padding(Padding::from([12.0, 16.0]))
            .size(16)
            .width(Length::FillPortion(4)),
        button(text("Search").size(15))
            .on_press(DomainMessage::Ui(UiShellMessage::ExecuteSearch.into(),))
            .style(ButtonStyle::Primary.style())
            .width(Length::FillPortion(1))
            .height(Length::Fixed(46.0)),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let input_panel = container(input_row)
        .padding(Padding::from([12.0, 16.0]))
        .width(Length::Fill)
        .style(|_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(
                MediaServerTheme::SOFT_GREY_MEDIUM,
            )),
            border: iced::Border {
                color: accent(),
                width: 1.0,
                radius: 10.0.into(),
            },
            shadow: iced::Shadow {
                color: accent_glow(),
                offset: iced::Vector::default(),
                blur_radius: 8.0,
            },
            ..Default::default()
        });

    let results = build_results_content(state).unwrap_or_else(|| {
        container(
            column![
                text("Start typing to search your library").size(18),
                text("We'll surface your best matches in real-time.")
                    .size(14)
                    .color(Color::from_rgb(0.7, 0.7, 0.75)),
            ]
            .spacing(8)
            .width(Length::Fill)
            .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(|_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(Color::from_rgba(
                0.1, 0.1, 0.13, 0.65,
            ))),
            border: iced::Border {
                color: Color::from_rgba(0.2, 0.2, 0.3, 0.4),
                width: 1.0,
                radius: 12.0.into(),
            },
            shadow: iced::Shadow::default(),
            ..Default::default()
        })
        .into()
    });

    container(
        column![header, input_panel, results]
            .spacing(16)
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .padding(Padding::from([16.0, 18.0]))
    .width(Length::Fill)
    .height(Length::Fill)
    .style(|_theme: &Theme| container::Style {
        background: Some(iced::Background::Color(
            MediaServerTheme::SURFACE_DIM,
        )),
        border: iced::Border {
            color: Color::from_rgb(0.08, 0.08, 0.1),
            width: 1.0,
            radius: 14.0.into(),
        },
        shadow: iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.55),
            offset: iced::Vector::new(0.0, 18.0),
            blur_radius: 28.0,
        },
        ..Default::default()
    })
    .into()
}

fn build_results_content(state: &State) -> Option<Element<'_, Message>> {
    let search_state = &state.domains.search.state;

    if search_state.query.is_empty()
        && search_state.results.is_empty()
        && !search_state.is_searching
    {
        return None;
    }

    let mut results_column = column![].spacing(6);

    if search_state.is_searching {
        results_column = results_column.push(
            container(
                row![
                    text("Searching...").size(16),
                    Space::new().width(Length::Fill),
                    text("⏳").size(18),
                ]
                .align_y(Alignment::Center)
                .spacing(10),
            )
            .padding(Padding::from([12.0, 20.0]))
            .width(Length::Fill),
        );
    } else if let Some(error) = &search_state.error {
        results_column = results_column.push(
            container(
                text(format!("Search error: {}", error))
                    .size(16)
                    .color(MediaServerTheme::ERROR),
            )
            .padding(Padding::from([12.0, 20.0]))
            .width(Length::Fill),
        );
    } else if search_state.results.is_empty() {
        results_column = results_column.push(
            container(
                text(format!("No results for \"{}\"", search_state.query))
                    .size(16),
            )
            .padding(Padding::from([12.0, 20.0]))
            .width(Length::Fill),
        );
    } else {
        let displayed_count = search_state.results.len();

        for (index, result) in search_state
            .results
            .iter()
            .take(displayed_count)
            .enumerate()
        {
            let is_selected = search_state.selected_index == Some(index);
            results_column =
                results_column.push(view_search_result(result, is_selected));
        }

        if search_state.total_results > 0 {
            results_column = results_column.push(
                container(
                    text(format!(
                        "Showing {} of {} results",
                        displayed_count, search_state.total_results
                    ))
                    .size(13),
                )
                .padding(Padding::from([8.0, 18.0]))
                .width(Length::Fill)
                .center_x(Length::Fill),
            );
        }
    }

    Some(
        container(
            {
                let scrollable_view = scrollable(results_column);
                scrollable_view
                    .id(crate::domains::search::types::SEARCH_RESULTS_SCROLL_ID)
                    .direction(scrollable::Direction::Vertical(
                        scrollable::Scrollbar::default(),
                    ))
            }
            .height(Length::Fill)
            .width(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(Color::from_rgba(
                0.08, 0.08, 0.1, 0.88,
            ))),
            border: iced::Border {
                color: Color::from_rgba(0.2, 0.25, 0.35, 0.6),
                width: 1.0,
                radius: 12.0.into(),
            },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
                offset: iced::Vector::new(0.0, 10.0),
                blur_radius: 20.0,
            },
            ..Default::default()
        })
        .max_height(f32::MAX)
        .into(),
    )
}

/// Render an individual search result item
fn view_search_result(
    result: &SearchResponse,
    is_selected: bool,
) -> Element<'_, Message> {
    let background = if is_selected {
        MediaServerTheme::CARD_HOVER
    } else {
        MediaServerTheme::CARD_BG
    };

    let border_color = if is_selected {
        accent()
    } else {
        MediaServerTheme::BORDER_COLOR
    };

    let mut text_column = column![text(&result.title).size(17)].spacing(6);

    if let Some(subtitle) = &result.subtitle {
        text_column = text_column.push(
            text(subtitle)
                .size(14)
                .color(MediaServerTheme::TEXT_SECONDARY),
        );
    }

    let mut metadata_row = row![].spacing(8);

    if let Some(year) = result.year {
        metadata_row = metadata_row.push(metadata_badge(year.to_string()));
    }

    metadata_row = metadata_row.push(metadata_badge(
        match_field_label(result.match_field).to_owned(),
    ));
    metadata_row = metadata_row.push(metadata_badge(format!(
        "{:.0}% match",
        result.match_score * 100.0
    )));

    text_column = text_column.push(metadata_row);

    let content_row = row![
        container(text(get_media_icon(&result.media_ref)).size(26))
            .width(Length::Fixed(48.0))
            .height(Length::Fixed(48.0))
            .center_x(Length::Fixed(48.0))
            .center_y(Length::Fixed(48.0))
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(Color::from_rgba(
                    0.2, 0.2, 0.24, 0.65,
                ))),
                border: iced::Border {
                    color: Color::from_rgba(0.35, 0.35, 0.45, 0.4),
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            }),
        text_column,
    ]
    .spacing(14)
    .align_y(Alignment::Center);

    button(
        container(content_row)
            .padding(Padding::from([14.0, 18.0]))
            .width(Length::Fill)
            .style(move |_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(background)),
                border: iced::Border {
                    color: border_color,
                    width: if is_selected { 1.5 } else { 1.0 },
                    radius: 12.0.into(),
                },
                shadow: if is_selected {
                    iced::Shadow {
                        color: accent_glow(),
                        offset: iced::Vector::default(),
                        blur_radius: 14.0,
                    }
                } else {
                    iced::Shadow::default()
                },
                ..Default::default()
            }),
    )
    .on_press(DomainMessage::Search(
        crate::domains::search::messages::SearchMessage::SelectResult(
            result.media_ref.clone(),
        ),
    ))
    .style(ButtonStyle::Text.style())
    .width(Length::Fill)
    .into()
}

/// Get an icon for the media type
fn get_media_icon(media_ref: &Media) -> &'static str {
    match media_ref {
        Media::Movie(_) => "🎬",
        Media::Series(_) => "📺",
        Media::Season(_) => "📅",
        Media::Episode(_) => "📹",
    }
}

fn metadata_badge(label: String) -> Element<'static, Message> {
    container(text(label).size(12))
        .padding(Padding::from([4.0, 8.0]))
        .style(|_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(Color::from_rgba(
                0.2, 0.2, 0.28, 0.7,
            ))),
            border: iced::Border {
                color: Color::from_rgba(0.35, 0.45, 0.55, 0.6),
                width: 1.0,
                radius: 999.0.into(),
            },
            text_color: Some(MediaServerTheme::TEXT_SECONDARY),
            ..Default::default()
        })
        .into()
}

fn match_field_label(field: SearchField) -> &'static str {
    match field {
        SearchField::Title => "Title",
        SearchField::Overview => "Overview",
        SearchField::Cast => "Cast",
        SearchField::Crew => "Crew",
        SearchField::Genre => "Genre",
        SearchField::All => "All Fields",
    }
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_search_fullscreen(state: &State) -> Element<'_, Message> {
    let search_state = &state.domains.search.state;

    // Header with back button
    let header = row![
        button(text("← Back").size(14))
            .on_press(DomainMessage::Search(
                crate::domains::search::messages::SearchMessage::SetMode(
                    SearchMode::Dropdown
                )
            ))
            .style(ButtonStyle::Text.style()),
        Space::new().width(Length::Fixed(20.0)),
        text(format!("Search Results for \"{}\"", search_state.query)).size(18),
        Space::new().width(Length::Fill),
        text(format!("{} results", search_state.total_results)).size(14)
    ]
    .padding(20)
    .align_y(Alignment::Center);

    // Search input for the dedicated search window
    let input_row = row![
        text_input("Search...", &search_state.query)
            .id(TextInputId::new(SEARCH_WINDOW_INPUT_ID))
            .on_input(|v| DomainMessage::Ui(
                UiShellMessage::UpdateSearchQuery(v).into()
            ))
            .on_submit(DomainMessage::Ui(UiShellMessage::ExecuteSearch.into()))
            .padding(Padding::from([12.0, 14.0]))
            .size(14)
            .width(Length::Fill),
        button(text("Search").size(14))
            .on_press(DomainMessage::Ui(UiShellMessage::ExecuteSearch.into()))
            .style(ButtonStyle::Primary.style()),
    ]
    .spacing(8)
    .padding(Padding::from([0.0, 20.0]))
    .align_y(Alignment::Center);

    // Results grid/list
    let mut results_column = column![].spacing(4);

    if search_state.is_searching {
        results_column = results_column.push(
            container(text("Searching...").size(16))
                .padding(40)
                .width(Length::Fill)
                .center_x(Length::Fill),
        );
    } else if search_state.results.is_empty() {
        results_column = results_column.push(
            container(column![
                text("No results found").size(20),
                Space::new().height(Length::Fixed(10.0)),
                text(format!(
                    "Try adjusting your search query \"{}\"",
                    search_state.query
                ))
                .size(14)
            ])
            .center_x(Length::Fill)
            .align_y(Alignment::Center)
            .padding(40)
            .width(Length::Fill),
        );
    } else {
        // Show all results in a grid
        for result in &search_state.results {
            results_column =
                results_column.push(view_search_result_fullscreen(result));
        }

        // Load more button
        if search_state.total_results > search_state.results.len() {
            results_column = results_column.push(
                container(
                    button(text("Load More Results").size(14))
                        .on_press(DomainMessage::Search(
                            crate::domains::search::messages::SearchMessage::LoadMore,
                        ))
                        .style(ButtonStyle::Primary.style()),
                )
                .padding(20)
                .width(Length::Fill)
                .center_x(Length::Fill),
            );
        }
    }

    column![
        header,
        input_row,
        container(scrollable(results_column).direction(
            scrollable::Direction::Vertical(scrollable::Scrollbar::default())
        ))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding::from([0.0, 20.0]))
    ]
    .into()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
fn view_search_result_fullscreen(
    result: &SearchResponse,
) -> Element<'_, Message> {
    let mut content_row = row![].spacing(16).align_y(Alignment::Center);

    // Larger icon/poster area
    content_row = content_row.push(
        container(text(get_media_icon(&result.media_ref)).size(32))
            .width(Length::Fixed(80.0))
            .height(Length::Fixed(80.0))
            .center_x(Length::Fixed(80.0))
            .center_y(Length::Fixed(80.0))
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(Color::from_rgba(
                    0.2, 0.2, 0.2, 0.5,
                ))),
                border: iced::Border {
                    color: Color::from_rgb(0.3, 0.3, 0.3),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }),
    );

    // Text information
    let mut text_column = column![].spacing(4);
    text_column = text_column.push(text(&result.title).size(16));

    if let Some(subtitle) = &result.subtitle {
        text_column = text_column.push(text(subtitle).size(14));
    }

    if let Some(year) = result.year {
        text_column =
            text_column.push(text(format!("Year: {}", year)).size(12));
    }

    content_row = content_row.push(text_column);

    button(
        container(content_row)
            .padding(Padding::from([12.0, 20.0]))
            .width(Length::Fill)
            .style(|_theme: &Theme| {
                container::Style {
                    background: Some(iced::Background::Color(
                        Color::from_rgba(0.15, 0.15, 0.15, 0.8),
                    )),
                    border: iced::Border {
                        color: Color::from_rgb(0.25, 0.25, 0.25),
                        width: 1.0,
                        radius: 0.0.into(), // Sharp corners
                    },
                    ..Default::default()
                }
            }),
    )
    .on_press(DomainMessage::Search(
        crate::domains::search::messages::SearchMessage::SelectResult(
            result.media_ref.clone(),
        ),
    ))
    .style(ButtonStyle::Text.style())
    .width(Length::Fill)
    .into()
}
