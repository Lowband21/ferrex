//! Unified media card system using macros for consistent UI components
//!
//! This module provides a macro-based approach to creating media cards for
//! movies, TV shows, seasons, and episodes with consistent styling, animations,
//! and loading states.

use crate::infrastructure::repository::MaybeYoked;
use ferrex_core::{ImageSize, ImageType, MediaOps, MovieID, MovieLike, SeriesID};
use iced::Length;
use iced::widget::{button, container, mouse_area, text};
// Module organization
use crate::domains::ui::messages::Message;
use crate::domains::ui::views::grid::macros::parse_hex_color;
use crate::domains::ui::widgets::image_for;
use crate::infrastructure::api_types::WatchProgress;
use crate::infrastructure::constants::poster::CORNER_RADIUS;
use ferrex_core::SeriesLike;
use iced::{Element, widget::column};
use uuid::Uuid;

use crate::{domains::ui::theme, state_refactored::State};
use std::sync::Arc;

use ferrex_core::{MediaIDLike, Priority};

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn create_movie_card<'a>(
    movie_id: Uuid,
    hovered_media_id: &Option<Uuid>,
    is_visible: bool,
    watch_progress: Option<WatchProgress>,
    state: &'a State,
) -> Element<'a, Message> {
    let is_hovered = hovered_media_id
        .as_ref()
        .map(|id| id == &movie_id)
        .unwrap_or(false);

    movie_reference_card_with_state(
        state,
        MovieID(movie_id),
        is_hovered,
        is_visible,
        watch_progress,
    )
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn create_series_card<'a>(
    series_id: Uuid,
    hovered_media_id: &Option<Uuid>,
    is_visible: bool,
    watch_progress: Option<WatchProgress>,
    state: &'a State,
) -> Element<'a, Message> {
    let is_hovered = hovered_media_id
        .as_ref()
        .map(|id| id == &series_id)
        .unwrap_or(false);

    series_reference_card_with_state(
        state,
        SeriesID(series_id),
        is_hovered,
        is_visible,
        watch_progress,
    )
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn movie_reference_card_with_state<'a>(
    state: &'a State,
    movie_id: MovieID,
    is_hovered: bool,
    is_visible: bool,
    watch_progress: Option<WatchProgress>,
) -> Element<'a, Message> {
    // Try from UI yoke cache first
    let uuid = movie_id.to_uuid();
    let yoke_arc: Arc<crate::infrastructure::repository::MovieYoke> =
        match state.domains.ui.state.movie_yoke_cache.peek_ref(&uuid) {
            Some(arc) => arc.clone(),
            _ => {
                // Lazily fetch from repo and insert into cache
                match state
                    .domains
                    .ui
                    .state
                    .repo_accessor
                    .get_movie_yoke(&ferrex_core::MediaID::Movie(movie_id))
                {
                    Ok(yoke) => {
                        let arc = Arc::new(yoke);
                        state
                            .domains
                            .ui
                            .state
                            .movie_yoke_cache
                            .insert(uuid, arc.clone());
                        arc
                    }
                    Err(e) => {
                        log::warn!("Failed to fetch movie yoke for {}: {:?}", uuid, e);
                        let placeholder_img: Element<'_, Message> = image_for(movie_id.to_uuid())
                            .size(ImageSize::Poster)
                            .image_type(ImageType::Movie)
                            .radius(CORNER_RADIUS)
                            .width(Length::Fixed(200.0))
                            .height(Length::Fixed(300.0))
                            .animation(state.domains.ui.state.default_widget_animation)
                            .placeholder(lucide_icons::Icon::Film)
                            .priority(if is_hovered || is_visible {
                                Priority::Visible
                            } else {
                                Priority::Preload
                            })
                            .is_hovered(is_hovered)
                            .into();
                        let image_with_hover = mouse_area(placeholder_img)
                            .on_enter(Message::MediaHovered(uuid))
                            .on_exit(Message::MediaUnhovered(uuid));
                        let poster_element = button(image_with_hover)
                            .on_press(Message::ViewMovieDetails(movie_id))
                            .padding(0)
                            .style(theme::Button::MediaCard.style());
                        let text_content = column![
                            text("...").size(14),
                            text(" ")
                                .size(12)
                                .color(theme::MediaServerTheme::TEXT_SECONDARY),
                        ]
                        .spacing(2);
                        return column![
                            poster_element,
                            container(text_content)
                                .padding(5)
                                .width(Length::Fixed(200.0))
                                .height(Length::Fixed(60.0))
                        ]
                        .spacing(5)
                        .into();
                    }
                }
            }
        };
    let movie = yoke_arc.get();

    let media_id = movie.media_id();
    let movie_id = movie.id();
    //let release_data = movie.release_date();
    //let year = movie.release_year();
    let theme_color = movie.theme_color();

    // Determine priority based on visibility and hover state
    let priority = if is_hovered || is_visible {
        Priority::Visible
    } else {
        Priority::Preload
    };

    // Create image with watch progress and scroll tier
    let mut img = image_for(media_id.to_uuid())
        .size(ImageSize::Poster)
        .image_type(ImageType::Movie)
        .radius(CORNER_RADIUS)
        .width(Length::Fixed(200.0))
        .height(Length::Fixed(300.0))
        .animation(state.domains.ui.state.default_widget_animation)
        .placeholder(lucide_icons::Icon::Film)
        .priority(priority)
        .is_hovered(is_hovered)
        .on_play(Message::PlayMediaWithId(media_id))
        .on_click(Message::ViewDetails(media_id));

    // Add theme color if available
    if let Some(theme_color_str) = theme_color {
        // Try cached color first
        let uuid_key = movie_id.to_uuid();
        let cached = state
            .domains
            .ui
            .state
            .theme_color_cache
            .read()
            .get(&uuid_key)
            .cloned();
        let color_opt = if let Some(c) = cached {
            Some(c)
        } else if let Ok(color) = parse_hex_color(&theme_color_str) {
            // Insert parsed color into cache
            state
                .domains
                .ui
                .state
                .theme_color_cache
                .write()
                .insert(uuid_key, color);
            Some(color)
        } else {
            None
        };
        if let Some(color) = color_opt {
            img = img.theme_color(color);
            img = img.progress_color(color);
        }
    }

    if let Some(progress) = watch_progress {
        img = img.progress(progress.as_percentage());
    }

    // Create the full card manually to match media_card! structure
    let image_element: Element<'_, Message> = img.into();

    // Wrap with hover detection
    let image_with_hover = mouse_area(image_element)
        .on_enter(Message::MediaHovered(movie_id.to_uuid()))
        .on_exit(Message::MediaUnhovered(movie_id.to_uuid()));

    // Create poster button
    let poster_element = button(image_with_hover)
        .on_press(Message::ViewMovieDetails(
            movie_id, //deserialize::<MovieReference, Error>(movie).unwrap(),
        ))
        .padding(0)
        .style(theme::Button::MediaCard.style());

    // Create text content
    let text_content = column![
        // TODO: We need to get title and year without creating temporary values
        text(movie.title().to_string()).size(14),
        text(if let Some(year) = movie.release_year() {
            year.to_string()
        } else {
            "".to_string()
        })
        .size(12)
        .color(theme::MediaServerTheme::TEXT_SECONDARY),
    ]
    .spacing(2);

    column![
        poster_element,
        container(text_content)
            .padding(5)
            .width(Length::Fixed(200.0))
            .height(Length::Fixed(60.0))
    ]
    .spacing(5)
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
pub fn series_reference_card_with_state<'a>(
    state: &'a State,
    series_id: SeriesID,
    is_hovered: bool,
    is_visible: bool,
    _watch_status: Option<WatchProgress>, // Number of remaining episodes equal to integer from watch_status, individual episode watch progress can be passed with the decimal part
) -> Element<'a, Message> {
    // Try from UI yoke cache first
    let uuid = series_id.to_uuid();
    let yoke_arc: Arc<crate::infrastructure::repository::SeriesYoke> =
        match state.domains.ui.state.series_yoke_cache.peek_ref(&uuid) {
            Some(arc) => arc.clone(),
            _ => {
                // Lazily fetch from repo and insert into cache (do not remove handlers or legacy comments)
                match state
                    .domains
                    .ui
                    .state
                    .repo_accessor
                    .get_series_yoke(&ferrex_core::MediaID::Series(series_id))
                {
                    Ok(yoke) => {
                        let arc = Arc::new(yoke);
                        state
                            .domains
                            .ui
                            .state
                            .series_yoke_cache
                            .insert(uuid, arc.clone());
                        arc
                    }
                    Err(e) => {
                        log::warn!("Failed to fetch series yoke for {}: {:?}", uuid, e);
                        // Fallback placeholder card preserving mouse handlers
                        let placeholder_img: Element<'_, Message> = image_for(series_id.to_uuid())
                            .size(ImageSize::Poster)
                            .image_type(ImageType::Series)
                            .radius(CORNER_RADIUS)
                            .width(Length::Fixed(200.0))
                            .height(Length::Fixed(300.0))
                            .animation(state.domains.ui.state.default_widget_animation)
                            .placeholder(lucide_icons::Icon::Tv)
                            .priority(if is_hovered || is_visible {
                                Priority::Visible
                            } else {
                                Priority::Preload
                            })
                            .is_hovered(is_hovered)
                            .into();
                        let image_with_hover = mouse_area(placeholder_img)
                            .on_enter(Message::MediaHovered(uuid))
                            .on_exit(Message::MediaUnhovered(uuid));
                        let poster_element = button(image_with_hover)
                            .on_press(Message::ViewTvShow(series_id))
                            .padding(0)
                            .style(theme::Button::MediaCard.style());
                        let text_content = column![
                            text("...").size(14),
                            text(" ")
                                .size(12)
                                .color(theme::MediaServerTheme::TEXT_SECONDARY),
                        ]
                        .spacing(2);
                        return column![
                            poster_element,
                            container(text_content)
                                .padding(5)
                                .width(Length::Fixed(200.0))
                                .height(Length::Fixed(60.0))
                        ]
                        .spacing(5)
                        .into();
                    }
                }
            }
        };
    let series = yoke_arc.get();

    let media_id = series.media_id();
    let series_id = series.id();
    let num_seasons = series.num_seasons();
    let theme_color = series.theme_color();

    // Determine priority based on visibility and hover state
    let priority = if is_hovered || is_visible {
        Priority::Visible
    } else {
        Priority::Preload
    };

    // Create image with scroll tier
    let mut img = image_for(series_id.to_uuid())
        .size(ImageSize::Poster)
        .image_type(ImageType::Series)
        .radius(CORNER_RADIUS)
        .width(Length::Fixed(200.0))
        .height(Length::Fixed(300.0))
        .animation(state.domains.ui.state.default_widget_animation)
        .placeholder(lucide_icons::Icon::Tv)
        .priority(priority)
        .is_hovered(is_hovered)
        .on_play(Message::PlaySeriesNextEpisode(series_id))
        .on_click(Message::ViewTvShow(series_id));

    // Add theme color if available
    if let Some(theme_color_str) = &theme_color {
        let uuid_key = series_id.to_uuid();
        let cached = state
            .domains
            .ui
            .state
            .theme_color_cache
            .read()
            .get(&uuid_key)
            .cloned();
        let color_opt = if let Some(c) = cached {
            Some(c)
        } else if let Ok(color) = parse_hex_color(theme_color_str) {
            state
                .domains
                .ui
                .state
                .theme_color_cache
                .write()
                .insert(uuid_key, color);
            Some(color)
        } else {
            None
        };
        if let Some(color) = color_opt {
            img = img.theme_color(color);
        }
    }

    let image_element: Element<'_, Message> = img.into();

    let image_with_hover = mouse_area(image_element)
        .on_enter(Message::MediaHovered(series_id.to_uuid()))
        .on_exit(Message::MediaUnhovered(series_id.to_uuid()));

    let poster_element = button(image_with_hover)
        .on_press(Message::ViewTvShow(series_id))
        .padding(0)
        .style(theme::Button::MediaCard.style());

    let text_content = column![
        text(series.title().to_string()).size(14),
        text("year")
            .size(12)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    ]
    .spacing(2);

    column![
        poster_element,
        container(text_content)
            .padding(5)
            .width(Length::Fixed(200.0))
            .height(Length::Fixed(60.0))
    ]
    .spacing(5)
    .into()
}

/*
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn season_reference_card_with_state<'a, Season: MaybeYoked>(
    season: Season,
    is_hovered: bool,
    state: Option<&'a crate::state_refactored::State>,
    _watch_status: Option<WatchProgress>, // Number of remaining episodes equal to integer from watch_status, individual episode watch progress can be passed with the decimal part
)where
        Season::InnerRef: SeasonLike,
        <Season as MaybeYoked>::InnerRef: MediaOps<Id = SeasonID>{
    use crate::infrastructure::api_types::{MediaDetailsOption, TmdbDetails};

    let season_id = season.id.as_uuid();

    // Extract season name from details if available
    let season_name = match &season.details {
        MediaDetailsOption::Details(TmdbDetails::Season(details)) => {
            if details.name.is_empty() {
                if season.season_number.value() == 0 {
                    "Specials"
                } else {
                    "Season"
                }
            } else {
                details.name.as_str()
            }
        }
        _ => {
            if season.season_number.value() == 0 {
                "Specials"
            } else {
                "Season"
            }
        }
    };

    /*
       // Get episode count from state if available, otherwise show loading
       let episode_count = state
           .map(|s| {
               s.domains
                   .ui
                   .state
                   .repo_accessor
                   .get_season_episode_count(&season_id)
           })
           .unwrap_or(0);

       let subtitle = if episode_count > 0 {
           if season.season_number.value() == 0 {
               format!("{} episodes", episode_count)
           } else {
               format!(
                   "Season {} • {} episodes",
                   season.season_number.value(),
                   episode_count
               )
           }
       } else {
           // No episode count available yet
           if season.season_number.value() == 0 {
               "Specials".to_string()
           } else {
               format!("Season {}", season.season_number.value())
           }
       };
    */
    // Use the media_card! macro for consistent card creation
    let season_id = SeasonID(season.id().to_uuid());
    media_card! {
        type: Season,
        data: season,
        {
            id: ferrex_core::MediaID::Season(season_id),
            title: season_name,
            //subtitle: &subtitle,
            subtitle: "Blank",
            image: {
                key: season_id,
                type: Poster,
                fallback: "📺",
            },
            size: Medium,
            on_click: Message::ViewSeason(season_id,season_id),
            on_play: Message::ViewSeason(season_id, season_id),
            hover_icon: lucide_icons::Icon::List,
            is_hovered: is_hovered,
        }
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
pub fn episode_reference_card_with_state<'a, Episode: MediaOps + EpisodeLike>(
    episode: &'a Episode,
    is_hovered: bool,
    state: Option<&'a State>,
    _watch_status: Option<WatchProgress>, // Number of remaining episodes equal to integer from watch_status, individual episode watch progress can be passed with the decimal part
) -> Element<'a, Message> {
    let episode_id = episode.id.as_uuid();

    // Extract episode name from details if available
    let (episode_name, has_details) = match &episode.details {
        MediaDetailsOption::Details(TmdbDetails::Episode(details)) => (details.name.as_str(), true),
        _ => ("", false),
    };

    // Format season and episode numbers
    let season_episode = format!(
        "S{:02}E{:02}",
        episode.season_number.value(),
        episode.episode_number.value()
    );

    // Episode cards are typically wider (thumbnail format)
    let width = 240.0;
    let height = 135.0; // 16:9 aspect ratio
    let radius = 4.0;

    // Get watch progress for this episode
    let watch_progress = state.and_then(|s| {
        s.domains
            .media
            .state
            .get_media_progress(&ferrex_core::MediaID::Episode(episode.id.clone()))
    });

    // Create image element
    let mut image_element = image_for(ferrex_core::MediaID::Episode(episode.id.clone()))
        .size(ImageSize::Thumbnail)
        .rounded(radius)
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .placeholder(lucide_icons::Icon::Play)
        .priority(if is_hovered {
            Priority::Visible
        } else {
            Priority::Preload
        });

    // Add watch progress - default to unwatched (0.0) if no watch state
    let progress = watch_progress.unwrap_or(0.0);
    image_element = image_element.progress(progress);

    // Create the poster button
    let poster_button = button(image_element)
        .on_press(Message::PlayMediaWithId(
            episode.file.clone(),
            ferrex_core::MediaID::Episode(episode.id.clone()),
        ))
        .padding(0)
        .style(theme::Button::MediaCard.style());

    /*
    // Add hover overlay if needed
    let poster_with_overlay: Element<'_, Message> = if is_hovered {
        use crate::hover_overlay;
        let overlay = hover_overlay!(
            width,
            height,
            radius,
            {
                center: (lucide_icons::Icon::Play, Message::PlayMediaWithId(
                    episode.file.clone(),
                    ferrex_core::MediaID::Episode(episode.id.clone())
                )),
                top_left: (lucide_icons::Icon::Circle, Message::NoOp),
                bottom_left: (lucide_icons::Icon::Pencil, Message::NoOp),
                bottom_right: (lucide_icons::Icon::EllipsisVertical, Message::NoOp),
            }
        );

        Stack::new().push(poster_button).push(overlay).into()
    } else {
        poster_button.into()
    }; */

    // Truncate episode name if too long
    let truncated_name = if episode_name.is_empty() {
        String::new()
    } else {
        truncate_text(episode_name, 30)
    };

    // Create text content with three lines
    let text_content = column![
        // Line 1: Episode title (always visible)
        text(format!("Episode {}", episode.episode_number.value()))
            .size(14)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        // Line 2: Episode name (with fade animation)
        container(text(truncated_name).size(12).color(if has_details {
            theme::MediaServerTheme::TEXT_SECONDARY
        } else {
            // Start with transparent color for fade-in effect
            iced::Color::from_rgba(
                theme::MediaServerTheme::TEXT_SECONDARY.r,
                theme::MediaServerTheme::TEXT_SECONDARY.g,
                theme::MediaServerTheme::TEXT_SECONDARY.b,
                0.0,
            )
        }))
        .height(Length::Fixed(16.0)), // Reserve space even when empty
        // Line 3: Season/Episode format
        text(season_episode)
            .size(11)
            .color(theme::MediaServerTheme::TEXT_DIMMED)
    ]
    .spacing(2);

    // Complete card
    let card_content = column![
        image_element,
        container(text_content)
            .padding(5)
            .width(Length::Fixed(width))
            .height(Length::Fixed(65.0)) // Slightly taller for 3 lines
            .clip(true)
    ]
    .spacing(5);

    // Wrap in mouse area
    mouse_area(
        container(card_content)
            .width(Length::Fixed(width))
            .height(Length::Fixed(height + 70.0)),
    )
    .on_enter(Message::MediaHovered(episode_id))
    .on_exit(Message::MediaUnhovered(episode_id))
    .into()
}
*/
