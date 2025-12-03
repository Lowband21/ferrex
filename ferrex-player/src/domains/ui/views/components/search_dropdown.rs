//! Search dropdown overlay component

use iced::widget::{
    Id as TextInputId, Space, button, column, container, row, scrollable, text,
    text_input,
};
use iced::{Alignment, Color, Element, Length, Padding, Theme};

use crate::common::messages::DomainMessage;
use crate::domains::search::types::{SearchMode, SearchResult};
use crate::domains::ui::shell_ui::UiShellMessage;
use crate::domains::ui::theme::{Button as ButtonStyle, MediaServerTheme};
use crate::domains::ui::windows::focus::SEARCH_WINDOW_INPUT_ID;
use crate::infra::api_types::Media;
use crate::infra::theme::{accent, accent_glow};
use crate::state::State;
use ferrex_core::query::types::SearchField;

type Message = DomainMessage;

#[derive(Clone, Copy)]
enum ResultsLayout {
    Dropdown,
    Window,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_search_dropdown(state: &State) -> Option<Element<'_, Message>> {
    let dropdown_content =
        build_results_content(state, ResultsLayout::Dropdown)?;

    // Use Stack to layer backdrop and dropdown
    use iced::widget::Stack;

    // Create transparent backdrop button that clears search when clicked
    let backdrop = button(
        container(Space::new().width(Length::Fill).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .on_press(DomainMessage::Search(
        crate::domains::search::messages::SearchMessage::ClearSearch,
    ))
    .style(
        |_theme: &iced::Theme, _status: button::Status| button::Style {
            background: Some(iced::Background::Color(Color::TRANSPARENT)),
            border: iced::Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 0.0.into(),
            },
            shadow: iced::Shadow::default(),
            text_color: Color::TRANSPARENT,
            snap: false,
        },
    )
    .width(Length::Fill)
    .height(Length::Fill);

    // Stack with backdrop behind dropdown
    Some(
        Stack::new()
            .push(backdrop)
            .push(
                container(dropdown_content)
                    .padding(Padding::from([65.0, 0.0])) // Position below header
                    .width(Length::Fill)
                    .center_x(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    )
}

pub fn view_search_window(state: &State) -> Element<'_, Message> {
    let search_state = &state.domains.search.state;

    let title = if search_state.query.is_empty() {
        "Search your library".to_owned()
    } else {
        format!("Results for \"{}\"", search_state.query)
    };

    let subtitle = if search_state.is_searching {
        "Searching...".to_owned()
    } else if search_state.results.is_empty() && !search_state.query.is_empty()
    {
        "No matches yet - try a different phrase".to_owned()
    } else if search_state.total_results > 0 {
        format!(
            "Showing {} of {} results",
            search_state.displayed_results, search_state.total_results
        )
    } else {
        "Find movies, shows, and episodes instantly".to_owned()
    };

    let header = container(
        row![
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
        .align_y(Alignment::Center),
    )
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

    let results = build_results_content(state, ResultsLayout::Window)
        .unwrap_or_else(|| {
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

fn build_results_content(
    state: &State,
    layout: ResultsLayout,
) -> Option<Element<'_, Message>> {
    let search_state = &state.domains.search.state;

    let is_window = matches!(layout, ResultsLayout::Window);

    if !is_window && search_state.mode != SearchMode::Dropdown {
        return None;
    }

    if !is_window
        && search_state.query.is_empty()
        && search_state.results.is_empty()
        && !search_state.is_searching
    {
        return None;
    }

    let mut results_column = column![].spacing(if is_window { 6 } else { 2 });

    if search_state.is_searching {
        results_column = results_column.push(
            container(
                row![
                    text("Searching...").size(if is_window { 16 } else { 14 }),
                    Space::new().width(Length::Fill),
                    text("⏳").size(if is_window { 18 } else { 16 }),
                ]
                .align_y(Alignment::Center)
                .spacing(10),
            )
            .padding(Padding::from([12.0, if is_window { 20.0 } else { 16.0 }]))
            .width(Length::Fill),
        );
    } else if let Some(error) = &search_state.error {
        results_column = results_column.push(
            container(
                text(format!("Search error: {}", error))
                    .size(if is_window { 16 } else { 14 })
                    .color(MediaServerTheme::ERROR),
            )
            .padding(Padding::from([12.0, if is_window { 20.0 } else { 16.0 }]))
            .width(Length::Fill),
        );
    } else if search_state.results.is_empty() {
        results_column = results_column.push(
            container(
                text(format!("No results for \"{}\"", search_state.query))
                    .size(if is_window { 16 } else { 14 }),
            )
            .padding(Padding::from([12.0, if is_window { 20.0 } else { 16.0 }]))
            .width(Length::Fill),
        );
    } else {
        let displayed_count = if is_window {
            search_state.results.len()
        } else {
            search_state
                .displayed_results
                .min(search_state.results.len())
        };

        for (index, result) in search_state
            .results
            .iter()
            .take(displayed_count)
            .enumerate()
        {
            let is_selected = search_state.selected_index == Some(index);
            results_column = results_column.push(view_search_result(
                result,
                is_selected,
                layout,
            ));
        }

        if !is_window
            && (search_state.results.len() > displayed_count
                || search_state.total_results > search_state.results.len())
        {
            results_column = results_column.push(
                button(
                    container(text("Load More").size(if is_window {
                        15
                    } else {
                        14
                    }))
                    .padding(Padding::from([
                        12.0,
                        if is_window { 18.0 } else { 16.0 },
                    ]))
                    .width(Length::Fill)
                    .center_x(Length::Fill),
                )
                .on_press(DomainMessage::Search(
                    crate::domains::search::messages::SearchMessage::LoadMore,
                ))
                .style(if is_window {
                    ButtonStyle::Primary.style()
                } else {
                    ButtonStyle::Text.style()
                })
                .width(Length::Fill),
            );
        }

        if search_state.total_results > 0 {
            results_column = results_column.push(
                container(
                    text(format!(
                        "Showing {} of {} results",
                        displayed_count, search_state.total_results
                    ))
                    .size(if is_window { 13 } else { 12 }),
                )
                .padding(Padding::from([
                    8.0,
                    if is_window { 18.0 } else { 16.0 },
                ]))
                .width(Length::Fill)
                .center_x(Length::Fill),
            );
        }
    }

    Some(
        container(
            {
                let scrollable_view = scrollable(results_column);

                let scrollable_view = if is_window {
                    scrollable_view.id(
                        crate::domains::search::types::SEARCH_RESULTS_SCROLL_ID,
                    )
                } else {
                    scrollable_view
                };

                scrollable_view.direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::default(),
                ))
            }
            .height(if is_window {
                Length::Fill
            } else {
                Length::Shrink
            })
            .width(Length::Fill),
        )
        .width(if is_window {
            Length::Fill
        } else {
            Length::Fixed(600.0)
        })
        .height(if is_window {
            Length::Fill
        } else {
            Length::Shrink
        })
        .style(move |_theme: &Theme| {
            if is_window {
                container::Style {
                    background: Some(iced::Background::Color(
                        Color::from_rgba(0.08, 0.08, 0.1, 0.88),
                    )),
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
                }
            } else {
                container::Style {
                    background: Some(iced::Background::Color(
                        Color::from_rgba(0.1, 0.1, 0.1, 0.98),
                    )),
                    border: iced::Border {
                        color: Color::from_rgb(0.3, 0.3, 0.3),
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    shadow: iced::Shadow {
                        color: Color::from_rgba(0.0, 0.0, 0.0, 0.4),
                        offset: iced::Vector::new(0.0, 4.0),
                        blur_radius: 12.0,
                    },
                    ..Default::default()
                }
            }
        })
        .max_height(if is_window { f32::MAX } else { 360.0 })
        .into(),
    )
}

/// Render an individual search result item
fn view_search_result(
    result: &SearchResult,
    is_selected: bool,
    layout: ResultsLayout,
) -> Element<'_, Message> {
    match layout {
        ResultsLayout::Dropdown => {
            let background_color = if is_selected {
                Color::from_rgba(0.3, 0.3, 0.3, 0.8)
            } else {
                Color::from_rgba(0.15, 0.15, 0.15, 0.0)
            };

            let mut content_row = row![].spacing(12).align_y(Alignment::Center);

            content_row = content_row.push(
                container(text(get_media_icon(&result.media_ref)).size(20))
                    .width(Length::Fixed(40.0))
                    .height(Length::Fixed(40.0))
                    .center_x(Length::Fixed(40.0))
                    .center_y(Length::Fixed(40.0)),
            );

            let mut text_column = column![].spacing(2);
            text_column = text_column.push(text(&result.title).size(14));

            if let Some(subtitle) = &result.subtitle {
                text_column = text_column.push(text(subtitle).size(12));
            }

            content_row = content_row.push(text_column);

            if cfg!(debug_assertions) {
                content_row =
                    content_row.push(Space::new().width(Length::Fill)).push(
                        text(format!("{:.0}%", result.match_score * 100.0))
                            .size(11),
                    );
            }

            button(
                container(content_row)
                    .padding(Padding::from([8.0, 16.0]))
                    .width(Length::Fill)
                    .style(move |_theme: &Theme| container::Style {
                        background: Some(iced::Background::Color(
                            background_color,
                        )),
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
        ResultsLayout::Window => {
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

            let mut text_column =
                column![text(&result.title).size(17)].spacing(6);

            if let Some(subtitle) = &result.subtitle {
                text_column = text_column.push(
                    text(subtitle)
                        .size(14)
                        .color(MediaServerTheme::TEXT_SECONDARY),
                );
            }

            let mut metadata_row = row![].spacing(8);

            if let Some(year) = result.year {
                metadata_row =
                    metadata_row.push(metadata_badge(year.to_string()));
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
                        background: Some(iced::Background::Color(
                            Color::from_rgba(0.2, 0.2, 0.24, 0.65,)
                        )),
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
    }
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
    result: &SearchResult,
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
