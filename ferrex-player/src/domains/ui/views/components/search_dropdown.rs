//! Search dropdown overlay component

use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Color, Element, Length, Padding, Theme};

use crate::common::messages::DomainMessage;
use crate::domains::search::types::{SearchMode, SearchResult};
use crate::domains::ui::theme::Button as ButtonStyle;
use crate::infrastructure::api_types::Media;
use crate::state::State;

type Message = DomainMessage;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_search_dropdown(state: &State) -> Option<Element<'_, Message>> {
    let search_state = &state.domains.search.state;

    // Only show dropdown if we're in dropdown mode and have a query or results
    if search_state.mode != SearchMode::Dropdown {
        return None;
    }

    if search_state.query.is_empty() && search_state.results.is_empty() {
        return None;
    }

    // Build results list
    let mut results_column = column![].spacing(2);

    if search_state.is_searching {
        // Show loading state
        results_column = results_column.push(
            container(
                row![
                    text("Searching...").size(14),
                    Space::new().width(Length::Fill),
                    text("⏳").size(16)
                ]
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([12.0, 16.0]))
            .width(Length::Fill),
        );
    } else if let Some(error) = &search_state.error {
        // Show error state
        results_column = results_column.push(
            container(
                text(format!("Search error: {}", error))
                    .size(14)
                    .color(Color::from_rgb(0.9, 0.3, 0.3)),
            )
            .padding(Padding::from([12.0, 16.0]))
            .width(Length::Fill),
        );
    } else if search_state.results.is_empty() {
        // Show no results state
        results_column = results_column.push(
            container(
                text(format!("No results for \"{}\"", search_state.query))
                    .size(14),
            )
            .padding(Padding::from([12.0, 16.0]))
            .width(Length::Fill),
        );
    } else {
        // Show results
        let displayed_count = search_state
            .displayed_results
            .min(search_state.results.len());

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

        // Show "Load More" button if there are more results
        if search_state.results.len() > displayed_count
            || search_state.total_results > search_state.results.len()
        {
            results_column = results_column.push(
                button(
                    container(text("Load More").size(14))
                        .padding(Padding::from([12.0, 16.0]))
                        .width(Length::Fill)
                        .center_x(Length::Fill),
                )
                .on_press(DomainMessage::Search(
                    crate::domains::search::messages::Message::LoadMore,
                ))
                .style(ButtonStyle::Text.style())
                .width(Length::Fill),
            );
        }

        // Show result count
        if search_state.total_results > 0 {
            results_column = results_column.push(
                container(
                    text(format!(
                        "Showing {} of {} results",
                        displayed_count, search_state.total_results
                    ))
                    .size(12),
                )
                .padding(Padding::from([8.0, 16.0]))
                .width(Length::Fill)
                .center_x(Length::Fill),
            );
        }
    }

    // Create dropdown container
    let dropdown_content = container(scrollable(results_column).direction(
        scrollable::Direction::Vertical(scrollable::Scrollbar::default()),
    ))
    .width(Length::Fixed(600.0))
    .max_height(400.0)
    .style(|_theme: &Theme| {
        container::Style {
            background: Some(iced::Background::Color(Color::from_rgba(
                0.1, 0.1, 0.1, 0.98,
            ))),
            border: iced::Border {
                color: Color::from_rgb(0.3, 0.3, 0.3),
                width: 1.0,
                radius: 0.0.into(), // Sharp corners as requested
            },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 12.0,
            },
            ..Default::default()
        }
    });

    // Use Stack to layer backdrop and dropdown
    use iced::widget::Stack;

    // Create transparent backdrop button that clears search when clicked
    let backdrop = button(
        container(Space::new().width(Length::Fill).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .on_press(DomainMessage::Search(
        crate::domains::search::messages::Message::ClearSearch,
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

/// Render an individual search result item
fn view_search_result(
    result: &SearchResult,
    is_selected: bool,
) -> Element<'_, Message> {
    let background_color = if is_selected {
        Color::from_rgba(0.3, 0.3, 0.3, 0.8)
    } else {
        Color::from_rgba(0.15, 0.15, 0.15, 0.0)
    };

    let mut content_row = row![].spacing(12).align_y(Alignment::Center);

    // Add poster/icon placeholder (future enhancement)
    content_row = content_row.push(
        container(text(get_media_icon(&result.media_ref)).size(20))
            .width(Length::Fixed(40.0))
            .height(Length::Fixed(40.0))
            .center_x(Length::Fixed(40.0))
            .center_y(Length::Fixed(40.0)),
    );

    // Add title and subtitle
    let mut text_column = column![].spacing(2);
    text_column = text_column.push(text(&result.title).size(14));

    if let Some(subtitle) = &result.subtitle {
        text_column = text_column.push(text(subtitle).size(12));
    }

    content_row = content_row.push(text_column);

    // Add match score indicator (for debugging)
    if cfg!(debug_assertions) {
        content_row = content_row
            .push(Space::new().width(Length::Fill))
            .push(text(format!("{:.0}%", result.match_score * 100.0)).size(11));
    }

    button(
        container(content_row)
            .padding(Padding::from([8.0, 16.0]))
            .width(Length::Fill)
            .style(move |_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(background_color)),
                ..Default::default()
            }),
    )
    .on_press(DomainMessage::Search(
        crate::domains::search::messages::Message::SelectResult(
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
                crate::domains::search::messages::Message::SetMode(
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
                            crate::domains::search::messages::Message::LoadMore,
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
        crate::domains::search::messages::Message::SelectResult(
            result.media_ref.clone(),
        ),
    ))
    .style(ButtonStyle::Text.style())
    .width(Length::Fill)
    .into()
}
