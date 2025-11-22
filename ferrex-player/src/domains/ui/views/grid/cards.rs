//! Unified media card system using macros for consistent UI components
//!
//! This module provides a macro-based approach to creating media cards for
//! movies, TV shows, seasons, and episodes with consistent styling, animations,
//! and loading states.

use crate::common::text;
use crate::{
    domains::ui::{
        interaction_ui::InteractionMessage, messages::UiMessage,
        playback_ui::PlaybackMessage, shell_ui::UiShellMessage, theme,
        views::grid::macros::parse_hex_color, widgets::image_for,
    },
    infra::{
        constants::poster::CORNER_RADIUS,
        repository::MaybeYoked,
        shader_widgets::{
            poster::PosterFace,
            poster::poster_animation_types::AnimationBehavior,
        },
    },
    state::State,
};

use ferrex_core::player_prelude::{
    ImageSize, ImageType, MediaDetailsOptionLike, MediaID, MediaIDLike,
    MediaOps, MovieID, MovieLike, Priority, SeriesID, SeriesLike,
    WatchProgress,
};

use iced::widget::text::Wrapping;
use iced::{
    Alignment, Element, Length,
    widget::{column, container, mouse_area},
};

use std::sync::Arc;
use uuid::Uuid;

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
) -> Element<'a, UiMessage> {
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
) -> Element<'a, UiMessage> {
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
) -> Element<'a, UiMessage> {
    // Try from UI yoke cache first
    let uuid = movie_id.to_uuid();
    let yoke_arc: Arc<crate::infra::repository::MovieYoke> =
        match state.domains.ui.state.movie_yoke_cache.peek_ref(&uuid) {
            Some(arc) => arc.clone(),
            _ => {
                // Lazily fetch from repo and insert into cache
                match state
                    .domains
                    .ui
                    .state
                    .repo_accessor
                    .get_movie_yoke(&MediaID::Movie(movie_id))
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
                        // If this UUID actually belongs to a Series, gracefully fall back
                        // to the series card builder to avoid a dangling placeholder and
                        // to ensure images/types flow correctly without panics.
                        match state
                            .domains
                            .ui
                            .state
                            .repo_accessor
                            .get_series_yoke(&MediaID::Series(SeriesID(uuid)))
                        {
                            Ok(_) => {
                                return series_reference_card_with_state(
                                    state,
                                    SeriesID(uuid),
                                    is_hovered,
                                    is_visible,
                                    watch_progress,
                                );
                            }
                            _ => (),
                        }

                        log::warn!(
                            "Failed to fetch movie yoke for {}: {:?}",
                            uuid,
                            e
                        );
                        let placeholder_img: Element<'_, UiMessage> =
                        image_for(movie_id.to_uuid())
                            .size(ImageSize::Poster)
                            .image_type(ImageType::Movie)
                            .radius(CORNER_RADIUS)
                            .width(Length::Fixed(200.0))
                            .height(Length::Fixed(300.0))
                            .animation_behavior(AnimationBehavior::default())
                            .placeholder(lucide_icons::Icon::Film)
                            .priority(if is_hovered || is_visible {
                                Priority::Visible
                            } else {
                                Priority::Preload
                            })
                            .is_hovered(is_hovered)
                            .on_click(
                                UiShellMessage::ViewMovieDetails(movie_id)
                                    .into(),
                            )
                            .into();
                        let image_with_hover = mouse_area(placeholder_img)
                            .on_enter(
                                InteractionMessage::MediaHovered(uuid).into(),
                            )
                            .on_exit(
                                InteractionMessage::MediaUnhovered(uuid).into(),
                            );
                        let poster_element = image_with_hover;
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
        .animation_behavior(AnimationBehavior::default())
        .placeholder(lucide_icons::Icon::Film)
        .priority(priority)
        .skip_request(true)
        .is_hovered(is_hovered)
        .on_play(PlaybackMessage::PlayMediaWithId(media_id).into())
        .on_click(UiShellMessage::ViewMovieDetails(movie_id).into());

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
        } else if let Ok(color) = parse_hex_color(theme_color_str) {
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

    let poster_id = movie_id.to_uuid();
    let (face, rotation_override) = if let Some(menu_state) =
        state.domains.ui.state.poster_menu_states.get(&poster_id)
    {
        (menu_state.face_for_render(), Some(menu_state.angle))
    } else if state.domains.ui.state.poster_menu_open == Some(poster_id) {
        (PosterFace::Back, Some(std::f32::consts::PI))
    } else {
        (PosterFace::Front, None)
    };
    img = img.face(face);
    if let Some(rot) = rotation_override {
        img = img.rotation_y(rot);
    }

    // Create the full card manually to match media_card! structure
    let image_element: Element<'_, UiMessage> = img.into();

    // Wrap with hover detection
    let image_with_hover = mouse_area(image_element)
        .on_enter(InteractionMessage::MediaHovered(movie_id.to_uuid()).into())
        .on_exit(InteractionMessage::MediaUnhovered(movie_id.to_uuid()).into());
    let poster_element = image_with_hover;

    let title = truncate(&mut movie.title().to_string());

    // Create text content
    let text_content = column![
        text(title)
            .align_x(Alignment::Center)
            .width(Length::Fixed(200.0))
            .size(13)
            .wrapping(Wrapping::None),
        text(movie.release_year().unwrap_or("932").to_string())
            .align_x(Alignment::Center)
            .width(Length::Fixed(200.0))
            .size(12)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    ]
    .spacing(2);

    column![
        poster_element,
        container(text_content)
            .padding([5, 0])
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
) -> Element<'a, UiMessage> {
    // Try from UI yoke cache first
    let uuid = series_id.to_uuid();
    let yoke_arc: Arc<crate::infra::repository::SeriesYoke> =
        match state.domains.ui.state.series_yoke_cache.peek_ref(&uuid) {
            Some(arc) => arc.clone(),
            _ => {
                // Lazily fetch from repo and insert into cache (do not remove handlers or legacy comments)
                match state
                    .domains
                    .ui
                    .state
                    .repo_accessor
                    .get_series_yoke(&MediaID::Series(series_id))
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
                        // If this UUID actually belongs to a Movie, gracefully fall back
                        // to the movie card builder.
                        if let Ok(_) = state
                            .domains
                            .ui
                            .state
                            .repo_accessor
                            .get_movie_yoke(&MediaID::Movie(MovieID(uuid)))
                        {
                            return movie_reference_card_with_state(
                                state,
                                MovieID(uuid),
                                is_hovered,
                                is_visible,
                                _watch_status,
                            );
                        }

                        log::warn!(
                            "Failed to fetch series yoke for {}: {:?}",
                            uuid,
                            e
                        );
                        // Fallback placeholder card preserving mouse handlers
                        let placeholder_img: Element<'_, UiMessage> =
                        image_for(series_id.to_uuid())
                            .size(ImageSize::Poster)
                            .image_type(ImageType::Series)
                            .radius(CORNER_RADIUS)
                            .width(Length::Fixed(200.0))
                            .height(Length::Fixed(300.0))
                            .animation_behavior(AnimationBehavior::default())
                            .placeholder(lucide_icons::Icon::Tv)
                            .priority(if is_hovered || is_visible {
                                Priority::Visible
                            } else {
                                Priority::Preload
                            })
                            .is_hovered(is_hovered)
                            .on_click(
                                UiShellMessage::ViewTvShow(series_id).into(),
                            )
                            .into();
                        let image_with_hover = mouse_area(placeholder_img)
                            .on_enter(
                                InteractionMessage::MediaHovered(uuid).into(),
                            )
                            .on_exit(
                                InteractionMessage::MediaUnhovered(uuid).into(),
                            );
                        let poster_element = image_with_hover;
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

    let _media_id = series.media_id();
    let series_id = series.id();
    let _num_seasons = series.num_seasons();
    let theme_color = series.theme_color();
    let details_opt = &series.details;

    // Determine priority based on visibility and hover state
    let priority = if is_hovered || is_visible {
        Priority::Visible
    } else {
        Priority::Preload
    };

    // Determine if we have a poster_path; if absent, skip fetching and render placeholder
    // let has_poster_path = details_opt
    //     .as_series()
    //     .and_then(|d| d.poster_path.as_ref())
    //     .is_some();

    // Create image with scroll tier
    let mut img = image_for(series_id.to_uuid())
        .size(ImageSize::Poster)
        .image_type(ImageType::Series)
        .radius(CORNER_RADIUS)
        .width(Length::Fixed(200.0))
        .height(Length::Fixed(300.0))
        .animation_behavior(AnimationBehavior::default())
        .placeholder(lucide_icons::Icon::Tv)
        .priority(priority)
        .is_hovered(is_hovered)
        .skip_request(true)
        .on_play(PlaybackMessage::PlaySeriesNextEpisode(series_id).into())
        .on_click(UiShellMessage::ViewTvShow(series_id).into());

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

    let poster_id = series_id.to_uuid();
    let (face, rotation_override) = if let Some(menu_state) =
        state.domains.ui.state.poster_menu_states.get(&poster_id)
    {
        (menu_state.face_for_render(), Some(menu_state.angle))
    } else if state.domains.ui.state.poster_menu_open == Some(poster_id) {
        (PosterFace::Back, Some(std::f32::consts::PI))
    } else {
        (PosterFace::Front, None)
    };
    img = img.face(face);
    if let Some(rot) = rotation_override {
        img = img.rotation_y(rot);
    }

    let image_element: Element<'_, UiMessage> = img.into();

    let image_with_hover = mouse_area(image_element)
        .on_enter(InteractionMessage::MediaHovered(series_id.to_uuid()).into())
        .on_exit(
            InteractionMessage::MediaUnhovered(series_id.to_uuid()).into(),
        );
    let poster_element = image_with_hover;

    let text_content = column![
        text(series.title().to_string())
            .align_x(Alignment::Center)
            .wrapping(Wrapping::None)
            .size(14),
        text(details_opt.get_release_year().unwrap_or(932))
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
    state: Option<&'a crate::state::State>,
    _watch_status: Option<WatchProgress>, // Number of remaining episodes equal to integer from watch_status, individual episode watch progress can be passed with the decimal part
)where
        Season::InnerRef: SeasonLike,
        <Season as MaybeYoked>::InnerRef: MediaOps<Id = SeasonID>{
    let season_id = season.id.as_uuid();

    // Extract season name from details if available
    let season_name = if let Some(details) = season.details.as_season() {
        if details.name.is_empty() {
            if season.season_number.value() == 0 {
                "Specials"
            } else {
                "Season"
            }
        } else {
            details.name.as_str()
        }
    } else if season.season_number.value() == 0 {
        "Specials"
    } else {
        "Season"
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
            id: MediaID::Season(season_id),
            title: season_name,
            //subtitle: &subtitle,
            subtitle: "Blank",
            image: {
                key: season_id,
                type: Poster,
                fallback: "📺",
            },
            size: Medium,
            on_click: UiShellMessage::ViewSeason(season_id, season_id).into(),
            on_play: UiShellMessage::ViewSeason(season_id, season_id).into(),
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
// NOTE: Removed stale episode card builder that used an outdated image_for API
*/

// Take str instead?
fn truncate(text: &mut String) -> String {
    const LIMIT: usize = 28;

    let text_len = text.len();

    if text_len >= LIMIT {
        // Byte index at the LIMIT-th char boundary.
        let limit_idx = text
            .char_indices()
            .nth(LIMIT)
            .map(|(i, _)| i)
            .unwrap_or_else(|| text.len());

        // Last whitespace before or at the limit.
        let cut_at = text[..limit_idx]
            .char_indices()
            .rev()
            .find(|&(_, c)| c.is_whitespace())
            .map(|(i, _)| i)
            .unwrap_or(limit_idx);

        // Trim any trailing spaces before adding ellipsis.
        let cut_at = text[..cut_at].trim_end().len();

        text.truncate(cut_at);
        text.push_str("...");
    }
    text.to_owned()
}
