use crate::{
    media_library::MediaFile,
    models::{MediaOrganizer, Season, TvShow},
    poster_cache::{PosterCache, PosterState},
    theme,
    widgets::{rounded_image, rounded_image_shader, AnimationType},
    Message,
};
use iced::{
    widget::{button, column, container, row, scrollable, text, Column, Row, Space, Stack},
    Element, Length,
};
use iced::{Color, Font};
use lucide_icons::Icon;
use std::collections::HashMap;

/// Get the lucide font
fn lucide_font() -> Font {
    Font::with_name("lucide")
}

/// Helper function to create icon text
fn icon_text(icon: Icon) -> text::Text<'static> {
    text(icon.unicode()).font(lucide_font()).size(20)
}

/// Get icon character string
fn icon_char(icon: Icon) -> String {
    icon.unicode().to_string()
}

/// Create a movie card for grid display
pub fn movie_card<'a>(
    file: &'a MediaFile,
    poster_cache: &'a PosterCache,
    is_hovered: bool,
) -> Element<'a, Message> {
    let title = file.display_title();
    let info = file.display_info();

    // Create poster element with fixed dimensions
    let poster_element_base: Element<Message> = match poster_cache.get(&file.id) {
        Some(PosterState::Loaded {
            thumbnail, opacity: _, ..
        }) => {
            // TEST: Use shader-based rounded image widget
            rounded_image_shader(thumbnail.clone())
                .radius(8.0)
                .width(iced::Length::Fixed(200.0))
                .height(iced::Length::Fixed(300.0))
                .into()
        }
        Some(PosterState::Loading) => container(
            column![text("⏳").size(32), text("Loading...").size(12)]
                .align_x(iced::Alignment::Center)
                .spacing(5),
        )
        .width(Length::Fixed(200.0))
        .height(Length::Fixed(300.0))
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(if is_hovered {
            theme::Container::CardHovered.style()
        } else {
            theme::Container::Card.style()
        })
        .into(),
        _ => container(text("🎬").size(48))
            .width(Length::Fixed(200.0))
            .height(Length::Fixed(300.0))
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .style(if is_hovered {
                theme::Container::CardHovered.style()
            } else {
                theme::Container::Card.style()
            })
            .into(),
    };

    // Wrap poster in a button for click handling
    let poster_button = button(poster_element_base)
        .on_press(Message::ViewDetails(file.clone()))
        .padding(0)
        .style(theme::Button::MediaCard.style());

    // Create the poster element with overlay if hovered
    let poster_element: Element<Message> = if is_hovered {
        // Create the overlay background
        let overlay_background: Element<Message> = container("")
            .width(Length::Fixed(200.0))
            .height(Length::Fixed(300.0))
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(Color::from_rgba(
                    0.0, 0.0, 0.0, 0.4,
                ))),
                border: iced::Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            })
            .into();

        // Play button in center
        let play_button = button(icon_text(Icon::Play).size(32).style(theme::icon_white))
            .on_press(Message::PlayMedia(file.clone()))
            .padding(16)
            .style(theme::Button::PlayOverlay.style());

        // Edit button in bottom left
        let edit_button = button(icon_text(Icon::Pencil).size(20).style(theme::icon_white))
            .on_press(Message::NoOp) // TODO: Implement edit functionality
            .padding(8)
            .style(theme::Button::Icon.style());

        // Menu button in bottom right
        let menu_button = button(
            icon_text(Icon::EllipsisVertical)
                .size(20)
                .style(theme::icon_white),
        )
        .on_press(Message::NoOp) // TODO: Implement menu functionality
        .padding(8)
        .style(theme::Button::Icon.style());

        // Checkmark in top left (empty circle for now)
        let check_button = button(icon_text(Icon::Circle).size(20).style(theme::icon_white))
            .on_press(Message::NoOp) // TODO: Implement selection functionality
            .padding(8)
            .style(theme::Button::Icon.style());

        // Create overlay layout
        let overlay_content = container(
            column![
                // Top row with checkmark
                row![check_button, Space::with_width(Length::Fill)].width(Length::Fill),
                // Center space with play button
                container(play_button)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center),
                // Bottom row with edit and menu
                row![edit_button, Space::with_width(Length::Fill), menu_button].width(Length::Fill)
            ]
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .width(Length::Fixed(200.0))
        .height(Length::Fixed(300.0))
        .padding(8);

        Stack::new()
            .push(poster_button)
            .push(overlay_background)
            .push(overlay_content)
            .into()
    } else {
        // Just the poster button without overlay
        poster_button.into()
    };

    // Create the complete media card with fixed total height
    let content = column![
        poster_element,
        // Text container with fixed dimensions
        container(
            column![
                text(title)
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                text(info)
                    .size(12)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY)
            ]
            .spacing(2)
        )
        .padding(5)
        .width(Length::Fixed(200.0))
        .height(Length::Fixed(60.0)) // Fixed height for text area
        .clip(true) // Clip any overflow
    ]
    .spacing(5);

    // Wrap in a mouse area for hover detection without additional styling
    let mouse_area = iced::widget::mouse_area(
        container(content)
            .width(Length::Fixed(200.0))
            .height(Length::Fixed(370.0)), // 300 (poster) + 5 (spacing) + 60 (text) + 5 (padding)
    )
    .on_enter(Message::MediaHovered(file.id.clone()))
    .on_exit(Message::MediaUnhovered);

    mouse_area.into()
}

/// Create a TV show card for grid display
pub fn tv_show_card<'a>(
    show: &'a TvShow,
    poster_cache: &'a PosterCache,
    is_hovered: bool,
) -> Element<'a, Message> {
    // Get the best poster URL for the show
    let poster_url = MediaOrganizer::get_show_poster_url(show);

    // Create poster element
    let poster_element: Element<Message> = if let Some(_url) = poster_url {
        // Try to find a cached poster from any episode of this show
        let mut poster_found = false;
        let mut poster_elem: Element<Message> = default_tv_poster();

        // Check all episodes for a cached poster
        'outer: for season in show.sorted_seasons() {
            for episode in season.episodes.values() {
                if let Some(PosterState::Loaded {
                    thumbnail, opacity, ..
                }) = poster_cache.get(&episode.id)
                {
                    poster_elem = rounded_image(thumbnail.clone())
                        .size(200.0, 300.0)
                        .radius(8.0)
                        .opacity(opacity)
                        .build();
                    poster_found = true;
                    break 'outer;
                }
            }
        }

        if poster_found {
            poster_elem
        } else {
            default_tv_poster_hovered(is_hovered)
        }
    } else {
        default_tv_poster_hovered(is_hovered)
    };

    // Show info
    let info = format!(
        "{} Season{} • {} Episodes",
        show.seasons.len(),
        if show.seasons.len() == 1 { "" } else { "s" },
        show.total_episodes
    );

    // Rating
    let rating_text = if let Some(rating) = show.rating {
        format!("★ {:.1}", rating)
    } else {
        String::new()
    };

    // Wrap poster in a button for click handling
    let poster_button = button(poster_element)
        .on_press(Message::ViewTvShow(show.name.clone()))
        .padding(0)
        .style(theme::Button::MediaCard.style());

    // Create the poster element with overlay if hovered
    let poster_with_overlay: Element<Message> = if is_hovered {
        // Create the overlay background
        let overlay_background: Element<Message> = container("")
            .width(Length::Fixed(200.0))
            .height(Length::Fixed(300.0))
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(Color::from_rgba(
                    0.0, 0.0, 0.0, 0.4,
                ))),
                border: iced::Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            })
            .into();

        // Play button in center
        let play_btn = if let Some(first_episode) = show
            .sorted_seasons()
            .first()
            .and_then(|s| s.first_episode())
        {
            button(icon_text(Icon::Play).size(32).style(theme::icon_white))
                .on_press(Message::PlayMedia(first_episode.clone()))
                .padding(16)
                .style(theme::Button::PlayOverlay.style())
        } else {
            button(icon_text(Icon::Play).size(32).style(theme::icon_white))
                .padding(16)
                .style(theme::Button::PlayOverlay.style())
        };

        // Edit button in bottom left
        let edit_button = button(icon_text(Icon::Pencil).size(20).style(theme::icon_white))
            .on_press(Message::NoOp) // TODO: Implement edit functionality
            .padding(8)
            .style(theme::Button::Icon.style());

        // Menu button in bottom right
        let menu_button = button(
            icon_text(Icon::EllipsisVertical)
                .size(20)
                .style(theme::icon_white),
        )
        .on_press(Message::NoOp) // TODO: Implement menu functionality
        .padding(8)
        .style(theme::Button::Icon.style());

        // Checkmark in top left (empty circle for now)
        let check_button = button(icon_text(Icon::Circle).size(20).style(theme::icon_white))
            .on_press(Message::NoOp) // TODO: Implement selection functionality
            .padding(8)
            .style(theme::Button::Icon.style());

        // Create overlay layout
        let overlay_content = container(
            column![
                // Top row with checkmark
                row![check_button, Space::with_width(Length::Fill)].width(Length::Fill),
                // Center space with play button
                container(play_btn)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center),
                // Bottom row with edit and menu
                row![edit_button, Space::with_width(Length::Fill), menu_button].width(Length::Fill)
            ]
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .width(Length::Fixed(200.0))
        .height(Length::Fixed(300.0))
        .padding(8);

        Stack::new()
            .push(poster_button)
            .push(overlay_background)
            .push(overlay_content)
            .into()
    } else {
        // Just the poster button without overlay
        poster_button.into()
    };

    let content = column![
        poster_with_overlay,
        // Text container with fixed dimensions
        container(
            column![
                text(&show.name)
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                text(info)
                    .size(12)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                if !rating_text.is_empty() {
                    text(rating_text)
                        .size(12)
                        .color(theme::MediaServerTheme::ACCENT_BLUE)
                } else {
                    text("")
                }
            ]
            .spacing(2)
        )
        .padding(5)
        .width(Length::Fixed(200.0))
        .height(Length::Fixed(60.0)) // Fixed height for text area
        .clip(true) // Clip any overflow
    ]
    .spacing(5);

    // Get poster ID for hover tracking
    let poster_id = show.get_poster_id().unwrap_or_else(|| show.name.clone());

    // Wrap in a mouse area for hover detection without additional styling
    let mouse_area = iced::widget::mouse_area(
        container(content)
            .width(Length::Fixed(200.0))
            .height(Length::Fixed(370.0)), // Same as movie cards
    )
    .on_enter(Message::MediaHovered(poster_id.clone()))
    .on_exit(Message::MediaUnhovered);

    mouse_area.into()
}

/// Create a season row with episode thumbnails
fn season_row<'a>(
    season: &'a Season,
    _show_name: &'a str,
    _poster_cache: &'a PosterCache,
) -> Element<'a, Message> {
    let mut episode_row = row![].spacing(10);

    // Season header
    let season_header = column![
        text(season.display_name())
            .size(16)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        text(format!("{} Episodes", season.episode_count))
            .size(12)
            .color(theme::MediaServerTheme::TEXT_SECONDARY)
    ]
    .spacing(2);

    // Add episode thumbnails
    for episode in season.sorted_episodes().into_iter().take(5) {
        let episode_num = episode
            .metadata
            .as_ref()
            .and_then(|m| m.parsed_info.as_ref())
            .and_then(|p| p.episode)
            .unwrap_or(0);

        let episode_card = container(
            column![
                // Episode thumbnail or number
                container(
                    text(format!("E{:02}", episode_num))
                        .size(24)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY)
                )
                .width(Length::Fixed(120.0))
                .height(Length::Fixed(67.0))
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .style(theme::Container::Card.style()),
                // Episode title
                container(
                    text(
                        episode
                            .metadata
                            .as_ref()
                            .and_then(|m| m.parsed_info.as_ref())
                            .and_then(|p| p.episode_title.as_ref())
                            .cloned()
                            .unwrap_or_else(|| format!("Episode {}", episode_num))
                    )
                    .size(11)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY)
                )
                .width(Length::Fixed(120.0))
                .height(Length::Shrink)
                .clip(true)
            ]
            .spacing(4),
        );

        episode_row = episode_row.push(
            button(episode_card)
                .on_press(Message::PlayMedia((*episode).clone()))
                .padding(5)
                .style(theme::Button::Secondary.style()),
        );
    }

    // Show "more" indicator if there are more episodes
    if season.episode_count > 5 {
        episode_row = episode_row.push(
            container(
                text(format!("+{} more", season.episode_count - 5))
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            )
            .padding(10),
        );
    }

    column![
        season_header,
        scrollable(episode_row).direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::default()
        ))
    ]
    .spacing(10)
    .into()
}

/// Default TV show poster
fn default_tv_poster() -> Element<'static, Message> {
    container(text("📺").size(48))
        .width(Length::Fixed(200.0))
        .height(Length::Fixed(300.0))
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(theme::Container::Card.style())
        .into()
}

/// Default TV show poster with hover support
fn default_tv_poster_hovered(is_hovered: bool) -> Element<'static, Message> {
    container(text("📺").size(48))
        .width(Length::Fixed(200.0))
        .height(Length::Fixed(300.0))
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(if is_hovered {
            theme::Container::CardHovered.style()
        } else {
            theme::Container::Card.style()
        })
        .into()
}

/// Default movie poster
pub fn default_movie_poster() -> Element<'static, Message> {
    container(text("🎬").size(48))
        .width(Length::Fixed(200.0))
        .height(Length::Fixed(300.0))
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(theme::Container::Card.style())
        .into()
}

/// Create a media section with title and horizontal scrolling content
pub fn media_section<'a>(
    title: &'a str,
    content: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    if content.is_empty() {
        return container(Space::with_height(0)).into();
    }

    let content_len = content.len();
    log::info!(
        "Creating media section '{}' with {} items",
        title,
        content_len
    );

    // Create rows of 5 items each instead of horizontal scrolling
    let items_per_row = 5;
    let mut rows = Vec::new();
    let mut current_row = Vec::new();

    for (i, item) in content.into_iter().enumerate() {
        current_row.push(item);

        if current_row.len() >= items_per_row || i == content_len - 1 {
            let row = Row::with_children(current_row).spacing(15).padding(5);
            rows.push(row.into());
            current_row = Vec::new();
        }
    }

    let rows_column = Column::with_children(rows).spacing(15);

    container(
        column![
            text(title)
                .size(24)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            Space::with_height(10),
            rows_column,
        ]
        .spacing(5),
    )
    .width(Length::Fill)
    .height(Length::Shrink)
    .into()
}

/// Create the main library view with organized media
pub fn library_view<'a>(
    movies: &'a [MediaFile],
    tv_shows: &'a std::collections::HashMap<String, TvShow>,
    _expanded_shows: &'a std::collections::HashSet<String>, // No longer used
    poster_cache: &'a PosterCache,
    movies_carousel: &'a crate::carousel::CarouselState,
    tv_shows_carousel: &'a crate::carousel::CarouselState,
    show_stats: bool,
) -> Element<'a, Message> {
    let mut content = column![].spacing(30).padding(20);

    // Library stats if enabled
    if show_stats {
        let stats_text = format!(
            "{} Movies • {} TV Shows • {} Episodes",
            movies.len(),
            tv_shows.len(),
            tv_shows.values().map(|s| s.total_episodes).sum::<usize>()
        );

        content = content.push(
            container(
                text(stats_text)
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            )
            .padding(10)
            .style(theme::Container::Card.style()),
        );
    }

    // TV Shows section
    if !tv_shows.is_empty() {
        log::info!("Creating TV Shows section with {} shows", tv_shows.len());

        let mut sorted_shows: Vec<_> = tv_shows.values().collect();
        sorted_shows.sort_by_key(|s| &s.name);

        let show_cards: Vec<_> = sorted_shows
            .into_iter()
            .map(|show| tv_show_card(show, poster_cache, false)) // Never expanded
            .collect();

        log::info!("Created {} TV show cards", show_cards.len());
        let tv_section = crate::carousel::media_carousel(
            "tv_shows".to_string(),
            "TV Shows",
            show_cards,
            tv_shows_carousel,
        );
        content = content.push(tv_section);
        log::info!("Added TV Shows section to content");
    } else {
        log::info!("No TV shows to display");
    }

    // Movies section
    if !movies.is_empty() {
        log::info!("Creating Movies section with {} movies", movies.len());

        let movie_cards: Vec<_> = movies
            .iter()
            .map(|movie| movie_card(movie, poster_cache, false))
            .collect();

        log::info!("Created {} movie cards", movie_cards.len());
        let movies_section = crate::carousel::media_carousel(
            "movies".to_string(),
            "Movies",
            movie_cards,
            movies_carousel,
        );
        content = content.push(movies_section);
        log::info!("Added Movies section to content");
    }

    // Add some padding at the bottom
    content = content.push(Space::with_height(50));

    // Debug: Log the final content structure
    log::info!(
        "Library view created with {} sections",
        if !tv_shows.is_empty() && !movies.is_empty() {
            2
        } else if !tv_shows.is_empty() || !movies.is_empty() {
            1
        } else {
            0
        }
    );

    // Wrap in scrollable
    scrollable(
        container(content)
            .width(Length::Fill)
            .height(Length::Shrink)
            .padding(0),
    )
    .direction(scrollable::Direction::Vertical(
        scrollable::Scrollbar::default(),
    ))
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Create a backdrop/banner image element
pub fn backdrop_image(url: Option<&str>, height: f32) -> Element<Message> {
    if let Some(_url) = url {
        // TODO: Implement backdrop image loading
        // For now, show a placeholder
        container(
            text("🎬")
                .size(48)
                .color(theme::MediaServerTheme::TEXT_DIMMED),
        )
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(theme::Container::Card.style())
        .into()
    } else {
        container(Space::with_height(0)).into()
    }
}

/// Create a season card for the show detail view
pub fn season_card<'a>(
    season: &'a crate::models::SeasonSummary,
    show_name: &'a str,
) -> Element<'a, Message> {
    // Create poster element
    let poster_element: Element<Message> = if let Some(_poster_url) = &season.poster_url {
        // TODO: Implement poster loading for seasons
        default_season_poster()
    } else {
        default_season_poster()
    };

    let title = season.name.clone().unwrap_or_else(|| {
        if season.number == 0 {
            "Specials".to_string()
        } else {
            format!("Season {}", season.number)
        }
    });

    let info = format!("{} Episodes", season.episode_count);

    // For seasons, clicking play button or card navigates to season details
    // (No direct play action since a season contains multiple episodes)
    let content = column![
        button(poster_element)
            .on_press(Message::ViewSeason(show_name.to_string(), season.number))
            .padding(0)
            .style(theme::Button::MediaCard.style()),
        // Text container with fixed dimensions
        container(
            column![
                text(title)
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                text(info)
                    .size(12)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY)
            ]
            .spacing(2)
        )
        .padding(5)
        .width(Length::Fixed(200.0))
        .height(Length::Fixed(60.0))
        .clip(true)
    ]
    .spacing(5);

    // Fixed size container
    container(
        container(content)
            .width(Length::Fixed(200.0))
            .height(Length::Fixed(370.0)),
    )
    .width(Length::Fixed(200.0))
    .height(Length::Fixed(370.0))
    .into()
}

/// Create an episode card for the season detail view
pub fn episode_card<'a>(
    episode: &'a crate::models::EpisodeSummary,
    show_name: &'a str,
    season_num: u32,
) -> Element<'a, Message> {
    // Create thumbnail element - episodes usually have wider thumbnails
    let thumbnail_element: Element<Message> = if let Some(_still_url) = &episode.still_url {
        // TODO: Implement still image loading
        default_episode_thumbnail(episode.number)
    } else {
        default_episode_thumbnail(episode.number)
    };

    let title = episode
        .title
        .clone()
        .unwrap_or_else(|| format!("Episode {}", episode.number));

    let mut info_parts = vec![format!("E{:02}", episode.number)];

    // Add duration if available
    if let Some(dur) = episode.duration {
        info_parts.push(format!("{} min", (dur / 60.0) as u32));
    }

    // Add air date if available
    if let Some(air_date) = &episode.air_date {
        // Parse and format the date nicely
        if let Some(year) = air_date.split('-').next() {
            info_parts.push(year.to_string());
        }
    }

    let info = info_parts.join(" • ");

    // Create a MediaFile for navigation
    let media_file = MediaFile {
        id: episode.id.clone(),
        filename: format!("{} S{:02}E{:02}", show_name, season_num, episode.number),
        path: String::new(), // Will be populated when playing
        size: 0,
        created_at: String::new(),
        metadata: None,
    };

    // Add play button overlay
    let play_button = container(
        button(icon_text(Icon::Play).size(24).style(theme::icon_white))
            .on_press(Message::PlayMedia(media_file.clone()))
            .padding(12)
            .style(theme::Button::PlayOverlay.style()),
    )
    .width(Length::Fill)
    .height(Length::Fixed(140.0)) // Match thumbnail height
    .align_x(iced::alignment::Horizontal::Center)
    .align_y(iced::alignment::Vertical::Center);

    // Stack thumbnail with play button
    let thumbnail_with_play = Stack::new()
        .push(
            button(thumbnail_element)
                .on_press(Message::ViewEpisode(media_file.clone()))
                .padding(0)
                .style(theme::Button::MediaCard.style()),
        )
        .push(play_button);

    let content = column![
        thumbnail_with_play,
        // Text container with more info
        container(
            column![
                text(title)
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                text(info)
                    .size(12)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                // Add truncated description if available
                if let Some(desc) = &episode.description {
                    let truncated = if desc.len() > 80 {
                        format!("{}...", &desc[..80])
                    } else {
                        desc.clone()
                    };
                    text(truncated)
                        .size(11)
                        .color(theme::MediaServerTheme::TEXT_DIMMED)
                } else {
                    text("")
                }
            ]
            .spacing(2)
        )
        .padding(5)
        .width(Length::Fixed(250.0))
        .height(Length::Fixed(80.0)) // Increased height for description
        .clip(true)
    ]
    .spacing(5);

    // Episode cards are wider than poster cards
    container(
        container(content)
            .width(Length::Fixed(250.0))
            .height(Length::Fixed(230.0)), // 140 (thumbnail) + 5 (spacing) + 80 (text) + 5 (padding)
    )
    .width(Length::Fixed(250.0))
    .height(Length::Fixed(230.0))
    .into()
}

/// Default season poster
pub fn default_season_poster() -> Element<'static, Message> {
    container(text("📺").size(48))
        .width(Length::Fixed(200.0))
        .height(Length::Fixed(300.0))
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(theme::Container::Card.style())
        .into()
}

/// Default episode thumbnail
pub fn default_episode_thumbnail(episode_num: u32) -> Element<'static, Message> {
    container(
        text(format!("E{:02}", episode_num))
            .size(32)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    )
    .width(Length::Fixed(250.0))
    .height(Length::Fixed(140.0))
    .align_x(iced::alignment::Horizontal::Center)
    .align_y(iced::alignment::Vertical::Center)
    .style(theme::Container::Card.style())
    .into()
}

/// Create a movie card with lazy loading support
pub fn movie_card_lazy<'a>(
    file: &'a MediaFile,
    poster_cache: &'a PosterCache,
    is_hovered: bool,
    _is_loading: bool, // Unused - we only check poster_cache state
    animation_types: &'a HashMap<String, (AnimationType, std::time::Instant)>,
) -> Element<'a, Message> {
    use crate::profiling::PROFILER;
    PROFILER.start("movie_card_lazy");

    let title = file.display_title();
    let info = file.display_info();

    // Create poster element with fixed dimensions
    let poster_state = poster_cache.get(&file.id);
    log::trace!(
        "Creating movie card for {}: poster_state = {:?}",
        file.filename,
        poster_state
            .as_ref()
            .map(|s| format!("{:?}", std::mem::discriminant(s)))
    );

    let poster_element_base: Element<Message> = match poster_state {
        Some(PosterState::Loaded {
            thumbnail, opacity: _, ..
        }) => {
            // Use shader-based rounded image with opacity from poster cache
            let mut rounded_img = rounded_image_shader(thumbnail.clone())
                .radius(8.0)
                .width(Length::Fixed(200.0))
                .height(Length::Fixed(300.0))
                .opacity(opacity);
            
            // Check if this poster has an active animation
            if let Some((animation_type, start_time)) = animation_types.get(&file.id) {
                rounded_img = rounded_img
                    .with_animation(*animation_type)
                    .with_load_time(*start_time);
            }
            
            rounded_img.into()
        }
        Some(PosterState::Loading) => container(
            column![text("⏳").size(32), text("Loading...").size(12)]
                .align_x(iced::Alignment::Center)
                .spacing(5),
        )
        .width(Length::Fixed(200.0))
        .height(Length::Fixed(300.0))
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(theme::Container::Card.style())
        .into(),
        Some(PosterState::Failed) | None => {
            // Show default poster for failed or not-yet-checked items
            default_movie_poster()
        }
    };
    // Create hover overlay with full controls if hovered
    let poster_element: Element<Message> = if is_hovered {
        // Create the overlay background
        let overlay_background: Element<Message> = container("")
            .width(Length::Fixed(200.0))
            .height(Length::Fixed(300.0))
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(Color::from_rgba(
                    0.0, 0.0, 0.0, 0.4,
                ))),
                border: iced::Border {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.4),
                    width: 0.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            })
            .into();
        let poster_border: Element<Message> = container("")
            .width(Length::Fixed(200.0))
            .height(Length::Fixed(300.0))
            .style(move |_| container::Style {
                //background: Some(iced::Background::Color(Color::from_rgba(
                //    0.0, 0.0, 0.0, 1.0,
                //))),
                border: iced::Border {
                    color: Color::from_rgba(0.1, 0.1, 0.1, 1.0),
                    width: 0.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            })
            .into();

        // Play button in center
        let play_button = button(icon_text(Icon::Play).size(32).style(theme::icon_white))
            .on_press(Message::PlayMedia(file.clone()))
            .padding(16)
            .style(theme::Button::PlayOverlay.style());

        // Edit button in bottom left
        let edit_button = button(icon_text(Icon::Pencil).size(20).style(theme::icon_white))
            .on_press(Message::NoOp) // TODO: Implement edit functionality
            .padding(8)
            .style(theme::Button::Icon.style());

        // Menu button in bottom right
        let menu_button = button(
            icon_text(Icon::EllipsisVertical)
                .size(20)
                .style(theme::icon_white),
        )
        .on_press(Message::NoOp) // TODO: Implement menu functionality
        .padding(8)
        .style(theme::Button::Icon.style());

        // Checkmark in top left (empty circle for now)
        let check_button = button(icon_text(Icon::Circle).size(20).style(theme::icon_white))
            .on_press(Message::NoOp) // TODO: Implement selection functionality
            .padding(8)
            .style(theme::Button::Icon.style());

        // Create overlay layout
        let overlay_content = container(
            column![
                // Top row with checkmark
                row![check_button, Space::with_width(Length::Fill)].width(Length::Fill),
                // Center space with play button
                container(play_button)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center),
                // Bottom row with edit and menu
                row![edit_button, Space::with_width(Length::Fill), menu_button].width(Length::Fill)
            ]
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .width(Length::Fixed(200.0))
        .height(Length::Fixed(300.0))
        .padding(8);

        Stack::new()
            //.push(poster_border)
            .push(poster_element_base)
            //.push(overlay_background)
            //.push(overlay_content)
            .width(Length::Fixed(200.0))
            .height(Length::Fixed(300.0))
            .into()
    } else {
        // Just show poster with rating badge when not hovered
        Stack::new()
            .push(poster_element_base)
            .push(
                container(if let Some(metadata) = &file.metadata {
                    if let Some(external) = &metadata.external_info {
                        text(format!("{:.1}★", external.rating.unwrap_or(0.0)))
                            .size(14)
                            .color(Color::from_rgb(1.0, 0.84, 0.0))
                    } else {
                        text("")
                    }
                } else {
                    text("")
                })
                .padding(5)
                .style(theme::Container::Badge.style()),
            )
            .width(Length::Fixed(200.0))
            .height(Length::Fixed(300.0))
            .into()
    };

    let content = column![
        button(poster_element)
            .padding(0)
            .on_press(Message::ViewDetails(file.clone()))
            .style(theme::Button::Card.style()),
        text(title)
            .size(14)
            .width(Length::Fixed(200.0))
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        text(info)
            .size(12)
            .width(Length::Fixed(200.0))
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    ]
    .spacing(5)
    .align_x(iced::Alignment::Center);

    let card = container(content)
        .width(Length::Fixed(200.0))
        .height(Length::Shrink)
        .clip(true);

    let result = iced::widget::mouse_area(card)
        .on_enter(Message::MediaHovered(file.id.clone()))
        .on_exit(Message::MediaUnhovered)
        .into();

    PROFILER.end("movie_card_lazy");
    result
}

/// Create a TV show card with lazy loading support
pub fn tv_show_card_lazy<'a>(
    show: &'a TvShow,
    poster_cache: &'a PosterCache,
    _is_hovered: bool,
    _is_loading: bool, // Unused - we only check poster_cache state
    compact: bool,
    animation_types: &'a HashMap<String, (AnimationType, std::time::Instant)>,
) -> Element<'a, Message> {
    let seasons_text = if show.seasons.len() == 1 {
        format!("1 Season")
    } else {
        format!("{} Seasons", show.seasons.len())
    };

    let episodes_text = format!("{} Episodes", show.total_episodes());

    // Get poster for the first episode that has one
    let poster_id = show.get_poster_id();

    let poster_element_base: Element<Message> =
        match poster_id.as_ref().and_then(|id| poster_cache.get(id)) {
            Some(PosterState::Loaded {
                thumbnail, opacity, ..
            }) => {
                // Use shader-based rounded image with opacity from poster cache
                let mut rounded_img = rounded_image_shader(thumbnail.clone())
                    .radius(8.0)
                    .width(Length::Fixed(200.0))
                    .height(Length::Fixed(300.0))
                    .opacity(opacity);
                
                // Check if this poster has an active animation
                if let Some(poster_id) = &poster_id {
                    if let Some((animation_type, start_time)) = animation_types.get(poster_id) {
                        rounded_img = rounded_img
                            .with_animation(*animation_type)
                            .with_load_time(*start_time);
                    }
                }
                
                rounded_img.into()
            }
            Some(PosterState::Loading) => container(
                column![text("⏳").size(32), text("Loading...").size(12)]
                    .align_x(iced::Alignment::Center)
                    .spacing(5),
            )
            .width(Length::Fixed(200.0))
            .height(Length::Fixed(300.0))
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .style(theme::Container::Card.style())
            .into(),
            Some(PosterState::Failed) | None => default_tv_poster(),
        };

    // Add play button overlay - TV shows navigate to show details, not play directly
    let play_button = container(
        button(icon_text(Icon::Tv).size(24).style(theme::icon_white))
            .on_press(Message::PlayMedia(show.next_episode().unwrap().clone()))
            .padding(12)
            .style(theme::Button::PlayOverlay.style()),
    )
    .width(Length::Fill)
    .height(Length::Fixed(300.0))
    .align_x(iced::alignment::Horizontal::Center)
    .align_y(iced::alignment::Vertical::Center);

    let poster_element: Element<Message> = Stack::new()
        .push(poster_element_base)
        .push(
            container(
                text(format!("{} {}", icon_char(Icon::Tv), seasons_text))
                    .size(12)
                    .color(Color::WHITE),
            )
            .padding(5)
            .style(theme::Container::Badge.style()),
        )
        .push(play_button)
        .width(Length::Fixed(200.0))
        .height(Length::Fixed(300.0))
        .into();

    let content = if compact {
        column![
            button(poster_element)
                .padding(0)
                .on_press(Message::ViewTvShow(show.name.clone()))
                .style(theme::Button::Card.style()),
            text(&show.name)
                .size(14)
                .width(Length::Fixed(200.0))
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
        ]
        .spacing(5)
        .align_x(iced::Alignment::Center)
    } else {
        column![
            button(poster_element)
                .padding(0)
                .on_press(Message::ViewTvShow(show.name.clone()))
                .style(theme::Button::Card.style()),
            text(show.name.clone())
                .size(14)
                .width(Length::Fixed(200.0))
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            text(episodes_text)
                .size(12)
                .width(Length::Fixed(200.0))
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        ]
        .spacing(5)
        .align_x(iced::Alignment::Center)
    };

    container(content)
        .width(Length::Fixed(200.0))
        .height(Length::Shrink)
        .into()
}
