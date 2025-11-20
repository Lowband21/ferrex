use crate::domains::media::selectors;
use crate::domains::ui::components;
use crate::domains::ui::views::grid::macros::parse_hex_color;
use crate::domains::ui::views::virtual_carousel::{self, types::CarouselKey};
use crate::infra::api_types::{
    EpisodeID, ImageSize, ImageType, MediaID, Priority, SeasonID,
    SeasonReference, SeriesID,
};
use crate::{
    domains::ui::{
        messages::Message, theme, views::grid::macros::ThemeColorAccess,
        widgets::image_for::image_for,
    },
    media_card,
    state::State,
};
use ferrex_core::player_prelude::{
    EpisodeLike, MediaIDLike, SeasonLike, SeriesDetailsLike, SeriesLike,
};
use iced::{
    Element, Length,
    widget::{Space, Stack, column, container, row, text},
};

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_series_detail<'a>(
    state: &'a State,
    series_id: SeriesID,
) -> Element<'a, Message> {
    // Resolve series yoke via UI cache with lazy fetch (interior mutable cache)
    let series_uuid = series_id.to_uuid();
    let series_yoke_arc = match state
        .domains
        .ui
        .state
        .series_yoke_cache
        .peek_ref(&series_uuid)
    {
        Some(arc) => arc,
        _ => {
            match state
                .domains
                .ui
                .state
                .repo_accessor
                .get_series_yoke(&MediaID::Series(series_id))
            {
                Ok(yoke) => {
                    let arc = std::sync::Arc::new(yoke);
                    state
                        .domains
                        .ui
                        .state
                        .series_yoke_cache
                        .insert(series_uuid, arc.clone());
                    arc
                }
                Err(e) => {
                    log::warn!(
                        "[TV] Failed to fetch series yoke for {}: {:?}",
                        series_uuid,
                        e
                    );
                    // Render minimal error content
                    return container(
                        column![
                            text("Media Not Found")
                                .size(24)
                                .color(theme::MediaServerTheme::TEXT_SECONDARY),
                            Space::new().height(10),
                            text("Repository error:")
                                .size(16)
                                .color(theme::MediaServerTheme::TEXT_SUBDUED),
                        ]
                        .spacing(10)
                        .align_x(iced::Alignment::Center),
                    )
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .into();
                }
            }
        }
    };
    let series = series_yoke_arc.get();
    //let season = season.get();
    let media_id = series_id.to_media_id();

    //let media_id = MediaID::Series(SeriesID(series_id.to_uuid()));

    let mut content = column![].spacing(20);

    // Add dynamic spacing at the top based on backdrop dimensions
    let window_width = state.window_size.width;
    let window_height = state.window_size.height;
    let content_offset = state
        .domains
        .ui
        .state
        .background_shader_state
        .calculate_content_offset_height(window_width, window_height);
    content = content.push(Space::new().height(Length::Fixed(content_offset)));

    // Details column
    let mut details = column![].spacing(15).padding(20).width(Length::Fill);

    // Title
    details = details.push(
        text(series.title().to_string())
            .size(32)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
    );

    // Apply theme color to poster if present
    let mut poster = image_for(media_id.to_uuid())
        .size(ImageSize::Full)
        .image_type(ImageType::Series)
        .width(Length::Fixed(300.0))
        .height(Length::Fixed(450.0))
        .priority(Priority::Visible);

    if let Some(hex) = series.theme_color()
        && let Ok(color) = parse_hex_color(hex)
    {
        poster = poster.theme_color(color);
    }
    let poster_element: Element<Message> = poster.into();

    let series_details_opt = series.details();

    // Fetch seasons for this series
    let seasons: Vec<SeasonReference> = match state
        .domains
        .ui
        .state
        .repo_accessor
        .get_series_seasons(&series_id)
    {
        Ok(s) => s,
        Err(e) => {
            log::warn!(
                "[TV] Failed to fetch seasons for series {}: {:?}",
                series_id,
                e
            );
            Vec::new()
        }
    };

    // Extract details from the series reference
    let (series_details, description, rating, total_episodes) =
        if let Some(series_details) = series_details_opt {
            /*
            log::info!(
                "Series {} has overview: {:?}",
                series_ref.title.as_str(),
                series_details
                    .overview
                    .as_ref()
                    .map(|o| crate::domains::ui::views::macros::truncate_text(o, 50))
            ); */
            (
                Some(series_details),
                series_details.overview.as_ref(),
                series_details.vote_average.as_ref(),
                series_details.number_of_episodes.as_ref(),
            )
        } else {
            log::warn!("Series {} has no TMDB details", series.title);
            (None, None, None, None)
        };

    /*
    // Stats - use the seasons we already fetched
    let season_count = seasons.len();
    if season_count > 0 {
        log::info!(
            "DETAIL VIEW: Found {} seasons for series '{}' (ID: {})",
            season_count,
            series_ref.title.as_str(),
            series_id.as_str()
        );
    }

    let stats = format!(
        "{} Seasons • {} Episodes",
        season_count,
        total_episodes.unwrap_or(0)
    );
    details = details.push(
        text(stats)
            .size(16)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    ); */

    // Stats: seasons and total episodes (if season list available)
    if !seasons.is_empty() {
        let season_count = seasons.len();
        let total_eps: u32 = seasons
            .iter()
            .map(|s| s.details().map(|d| d.episode_count).unwrap_or(0))
            .sum();
        details = details.push(
            text(format!("{} Seasons • {} Episodes", season_count, total_eps))
                .size(16)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );
    }

    // Rating
    if let Some(rating) = rating {
        details = details.push(
            text(format!("★ {:.1}", rating))
                .size(16)
                .color(theme::MediaServerTheme::ACCENT_BLUE),
        );
    }

    // Play button: play first in-progress or first unwatched episode in the series
    if let Some(next_ep_id) =
        selectors::select_next_episode_for_series(state, series_id)
    {
        let button_row = components::create_action_button_row(
            Message::PlayMediaWithId(MediaID::Episode(next_ep_id)),
            vec![],
        );
        details = details.push(Space::new().height(10));
        details = details.push(button_row);
    }

    // Description
    if let Some(desc) = description {
        details = details.push(Space::new().height(10));
        details = details.push(
            container(
                text(desc.to_string())
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
            )
            .width(Length::Fill)
            .padding(10),
        );
    }

    // Genres
    if let Some(series_details) = series_details
        && !series_details.genres().is_empty()
    {
        details = details.push(
            text(format!("Genres: {}", series_details.genres().join(", ")))
                .size(14)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );
    }

    // Content row with poster and details
    let info_row = row![poster_element, details]
        .spacing(10)
        .align_y(iced::Alignment::Start);

    content = content.push(info_row);

    // Seasons carousel - virtual carousel module
    if !seasons.is_empty() {
        content = content.push(Space::new().height(20));
        let key = CarouselKey::ShowSeasons(series_id.to_uuid());
        if let Some(vc_state) =
            state.domains.ui.state.carousel_registry.get(&key)
        {
            let seasons_vec = seasons.clone();
            let section = virtual_carousel::virtual_carousel(
                key.clone(),
                "Seasons",
                seasons_vec.len(),
                vc_state,
                move |idx| {
                    seasons_vec.get(idx).map(|s| {
                        let title_str = if s.season_number.value() == 0 {
                            "Specials".to_string()
                        } else {
                            format!("Season {}", s.season_number.value())
                        };
                        let subtitle_str = s
                            .details()
                            .map(|d| format!("{} episodes", d.episode_count))
                            .unwrap_or_else(|| String::from("\u{00A0}"));

                        media_card! {
                            type: Season,
                            data: s.clone(),
                            {
                                id: s.id.to_uuid(),
                                title: title_str.as_str(),
                                subtitle: subtitle_str.as_str(),
                                image: {
                                    key: s.id.to_uuid(),
                                    type: Poster,
                                    fallback: "📺",
                                },
                                size: Medium,
                                on_click: Message::ViewSeason(s.series_id, s.id),
                                on_play: Message::NoOp,
                                hover_icon: lucide_icons::Icon::List,
                                is_hovered: false,
                            }
                        }
                    })
                },
                false,
            );
            content = content.push(section);
        }
    }
    // Create the main content container
    let content_container = container(content).width(Length::Fill);

    // Calculate backdrop dimensions
    let window_width = state.window_size.width;
    let window_height = state.window_size.height;
    let display_aspect = state
        .domains
        .ui
        .state
        .background_shader_state
        .calculate_display_aspect(window_width, window_height);
    let backdrop_height = window_width / display_aspect;

    // Create aspect ratio toggle button
    let aspect_button =
        crate::domains::ui::components::create_backdrop_aspect_button(state);

    // Position the button at bottom-right of backdrop with small margin
    let button_container = container(aspect_button)
        .padding([0, 20])
        .width(Length::Fill)
        .height(Length::Fixed(backdrop_height - 22.5))
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Bottom);

    // Layer the button over the content using Stack
    Stack::new()
        .push(content_container)
        .push(button_container)
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
pub fn view_season_detail<'a>(
    state: &'a State,
    _series_id: &'a SeriesID,
    season_id: &'a SeasonID,
) -> Element<'a, Message> {
    // Resolve season yoke via UI cache with lazy fetch
    let season_uuid = season_id.to_uuid();
    let season_yoke_arc = match state
        .domains
        .ui
        .state
        .season_yoke_cache
        .peek_ref(&season_uuid)
    {
        Some(arc) => arc,
        _ => match state
            .domains
            .ui
            .state
            .repo_accessor
            .get_season_yoke(&MediaID::Season(*season_id))
        {
            Ok(yoke) => {
                let arc = std::sync::Arc::new(yoke);
                state
                    .domains
                    .ui
                    .state
                    .season_yoke_cache
                    .insert(season_uuid, arc.clone());
                arc
            }
            Err(e) => {
                log::warn!(
                    "[TV] Failed to fetch season yoke for {}: {:?}",
                    season_uuid,
                    e
                );
                return container(
                    column![
                        text("Media Not Found")
                            .size(24)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                        Space::new().height(10),
                        text("Repository error:")
                            .size(16)
                            .color(theme::MediaServerTheme::TEXT_SUBDUED),
                    ]
                    .spacing(10)
                    .align_x(iced::Alignment::Center),
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into();
            }
        },
    };
    let season = season_yoke_arc.get();

    let mut content = column![].spacing(20);

    // Add dynamic spacing at the top based on backdrop dimensions
    let window_width = state.window_size.width;
    let window_height = state.window_size.height;
    let content_offset = state
        .domains
        .ui
        .state
        .background_shader_state
        .calculate_content_offset_height(window_width, window_height);
    content = content.push(Space::new().height(Length::Fixed(content_offset)));

    // Poster element
    let mut poster = image_for(season.id.to_uuid())
        .size(ImageSize::Poster)
        .image_type(ImageType::Season)
        .width(Length::Fixed(300.0))
        .height(Length::Fixed(450.0))
        .priority(Priority::Visible);
    if let Some(hex) = season.theme_color()
        && let Ok(color) = parse_hex_color(hex)
    {
        poster = poster.theme_color(color);
    }
    let poster_element: Element<Message> = poster.into();

    // Details column
    let mut details = column![].spacing(15).padding(20).width(Length::Fill);

    // Title and episode count
    let season_number = season.details().map(|d| d.season_number).unwrap_or(0);
    let title = if season_number == 0 {
        "Specials".to_string()
    } else {
        format!("Season {}", season_number)
    };
    details = details.push(
        text(title)
            .size(32)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
    );

    let episode_count = season.num_episodes();
    details = details.push(
        text(format!("{} Episodes", episode_count))
            .size(16)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    );

    // Play button: play first in-progress or first unwatched episode in this season
    if let Some(next_ep_id) =
        selectors::select_next_episode_for_season(state, *season_id)
    {
        let button_row = components::create_action_button_row(
            Message::PlayMediaWithId(MediaID::Episode(next_ep_id)),
            vec![],
        );
        details = details.push(Space::new().height(10));
        details = details.push(button_row);
    }

    // Overview
    if let Some(season_details) = season.details()
        && let Some(desc) = season_details.overview.as_ref()
    {
        details = details.push(Space::new().height(10));
        details = details.push(
            container(
                text(desc.to_string())
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
            )
            .width(Length::Fill)
            .padding(10),
        );
    }

    // Content row with poster and details
    let info_row = row![poster_element, details]
        .spacing(10)
        .align_y(iced::Alignment::Start);

    content = content.push(info_row);

    // Episodes carousel for this season
    let episodes = state
        .domains
        .ui
        .state
        .repo_accessor
        .get_season_episodes(season_id)
        .unwrap_or_else(|_| Vec::new());

    if !episodes.is_empty() {
        content = content.push(Space::new().height(20));
        let key = CarouselKey::SeasonEpisodes(season_id.to_uuid());
        if let Some(vc_state) =
            state.domains.ui.state.carousel_registry.get(&key)
        {
            let eps_vec = episodes.clone();
            let ep_section = virtual_carousel::virtual_carousel(
                key.clone(),
                "Episodes",
                eps_vec.len(),
                vc_state,
                move |idx| {
                    eps_vec.get(idx).map(|e| {
                        let title_str = format!(
                            "S{:02}E{:02}",
                            e.season_number.value(),
                            e.episode_number.value()
                        );
                        let subtitle_str = e
                            .details()
                            .map(|d| d.name.as_str())
                            .unwrap_or("Episode title unavailable");

                        media_card! {
                            type: Episode,
                            data: e.clone(),
                            {
                                id: e.id.to_uuid(),
                                title: title_str.as_str(),
                                subtitle: subtitle_str,
                                image: {
                                    key: e.id.to_uuid(),
                                    type: Thumbnail,
                                    fallback: "🎞",
                                },
                                size: Wide,
                                on_click: Message::PlayMediaWithId(MediaID::Episode(e.id)),
                                on_play: Message::PlayMediaWithId(MediaID::Episode(e.id)),
                                hover_icon: lucide_icons::Icon::Play,
                                is_hovered: false,
                            }
                        }
                    })
                },
                false,
            );
            content = content.push(ep_section);
        }
    }

    // Create the main content container
    let content_container = container(content).width(Length::Fill);

    // Calculate backdrop dimensions
    let window_width = state.window_size.width;
    let window_height = state.window_size.height;
    let display_aspect = state
        .domains
        .ui
        .state
        .background_shader_state
        .calculate_display_aspect(window_width, window_height);
    let backdrop_height = window_width / display_aspect;

    // Create aspect ratio toggle button
    let aspect_button =
        crate::domains::ui::components::create_backdrop_aspect_button(state);

    // Position the button at bottom-right of backdrop with small margin
    let button_container = container(aspect_button)
        .padding([0, 20])
        .width(Length::Fill)
        .height(Length::Fixed(backdrop_height - 22.5))
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Bottom);

    // Layer the button over the content using Stack
    Stack::new()
        .push(content_container)
        .push(button_container)
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
pub fn view_episode_detail<'a>(
    state: &'a State,
    episode_id: &'a EpisodeID,
) -> Element<'a, Message> {
    // Try to get episode yoke from cache or fetch on-demand
    let ep_uuid = episode_id.to_uuid();
    let episode_yoke_arc =
        match state.domains.ui.state.episode_yoke_cache.peek_ref(&ep_uuid) {
            Some(arc) => arc,
            _ => match state
                .domains
                .ui
                .state
                .repo_accessor
                .get_episode_yoke(&MediaID::Episode(*episode_id))
            {
                Ok(yoke) => {
                    let arc = std::sync::Arc::new(yoke);
                    state
                        .domains
                        .ui
                        .state
                        .episode_yoke_cache
                        .insert(ep_uuid, arc.clone());
                    arc
                }
                Err(e) => {
                    log::warn!(
                        "[TV] Failed to fetch episode yoke for {}: {:?}",
                        ep_uuid,
                        e
                    );
                    return container(
                        column![
                            text("Media Not Found")
                                .size(24)
                                .color(theme::MediaServerTheme::TEXT_SECONDARY),
                            Space::new().height(10),
                            text("Repository error:")
                                .size(16)
                                .color(theme::MediaServerTheme::TEXT_SUBDUED),
                        ]
                        .spacing(10)
                        .align_x(iced::Alignment::Center),
                    )
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .into();
                }
            },
        };
    let episode = episode_yoke_arc.get();

    // Add dynamic spacing at the top based on backdrop dimensions
    let window_width = state.window_size.width;
    let window_height = state.window_size.height;
    let content_offset = state
        .domains
        .ui
        .state
        .background_shader_state
        .calculate_content_offset_height(window_width, window_height);

    let mut content = column![].spacing(20).padding(20);
    content = content.push(Space::new().height(Length::Fixed(content_offset)));

    // Episode still image
    let still_element: Element<Message> = image_for(episode.id.to_uuid())
        .size(ImageSize::Thumbnail)
        .image_type(ImageType::Episode)
        .width(Length::Fixed(640.0))
        .height(Length::Fixed(360.0))
        .priority(Priority::Visible)
        .into();

    // Details column
    let mut details = column![].spacing(15).padding(20).width(Length::Fill);

    // Title and info
    let (ep_name, overview, air_date, runtime, vote_average) =
        if let Some(d) = episode.details() {
            (
                Some(d.name.to_string()),
                d.overview.as_ref(),
                d.air_date.as_ref(),
                d.runtime.as_ref(),
                d.vote_average.as_ref(),
            )
        } else {
            (None, None, None, None, None)
        };

    let (season_number, ep_number) = if let Some(d) = episode.details() {
        (d.season_number, d.episode_number)
    } else {
        (0, 0)
    };
    let title = ep_name.unwrap_or_else(|| format!("Episode {}", ep_number));
    details = details.push(
        text(format!("S{:02}E{:02}: {}", season_number, ep_number, title))
            .size(28)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
    );

    let mut info_parts = Vec::new();
    if let Some(date) = air_date {
        info_parts.push(date.to_string());
    }
    if let Some(rt) = runtime {
        info_parts.push(format!("{} min", rt));
    }
    if let Some(rating) = vote_average {
        info_parts.push(format!("★ {:.1}", rating));
    }
    if !info_parts.is_empty() {
        details = details.push(
            text(info_parts.join(" • "))
                .size(16)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );
    }

    // Play button
    let button_row = components::create_action_button_row(
        Message::PlayMediaWithId(MediaID::Episode(EpisodeID(
            episode.id.to_uuid(),
        ))),
        vec![],
    );
    details = details.push(Space::new().height(10));
    details = details.push(button_row);

    // Overview
    if let Some(desc) = overview {
        details = details.push(Space::new().height(20));
        details = details.push(
            container(
                text(desc.to_string())
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
            )
            .width(Length::Fill)
            .padding(10),
        );
    }

    // Layout
    content = content.push(
        column![still_element, Space::new().height(20), details].spacing(10),
    );

    // Create the main content container
    let content_container = container(content).width(Length::Fill);

    // Calculate backdrop dimensions
    let display_aspect = state
        .domains
        .ui
        .state
        .background_shader_state
        .calculate_display_aspect(window_width, window_height);
    let backdrop_height = window_width / display_aspect;

    // Create aspect ratio toggle button
    let aspect_button =
        crate::domains::ui::components::create_backdrop_aspect_button(state);

    // Position the button at bottom-right of backdrop with small margin
    let button_container = container(aspect_button)
        .padding([0, 20])
        .width(Length::Fill)
        .height(Length::Fixed(backdrop_height - 22.5))
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Bottom);

    Stack::new()
        .push(content_container)
        .push(button_container)
        .into()
}
