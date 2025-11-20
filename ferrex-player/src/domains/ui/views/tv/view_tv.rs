use crate::domains::ui::components;
use crate::domains::ui::views::grid::macros::parse_hex_color;
use crate::{
    domains::ui::{
        messages::Message, theme, views::grid::macros::ThemeColorAccess,
        widgets::image_for::image_for,
    },
    media_card,
    state_refactored::State,
};
use ferrex_core::{
    traits::{
        details_like::SeriesDetailsLike,
        id::MediaIDLike,
        sub_like::{EpisodeLike, SeasonLike, SeriesLike},
    },
    types::{
        ids::{EpisodeID, SeasonID, SeriesID},
        image_request::Priority,
        media::SeasonReference,
        media_id::MediaID,
        util_types::{ImageSize, ImageType},
    },
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

    //// Play button row - use series progress service to find appropriate episode
    //use crate::domains::media::services::SeriesProgressService;

    //let media_store = state.domains.media.state.media_store.clone();
    //let service = SeriesProgressService::new(media_store);
    //let watch_state = state.domains.media.state.user_watch_state.as_ref();

    //// Get next episode to continue and first episode for play from beginning
    //let next_episode_info = service.get_next_episode_for_series(&series_id, watch_state);

    //// Also get the very first episode for "Play from Beginning" option
    //let first_episode = {
    //    let first_season = seasons
    //        .iter()
    //        .find(|season| season.season_number.value() == 1)
    //        .or_else(|| seasons.first());

    //    if let Some(season) = first_season {
    //        let episodes = state
    //            .domains
    //            .media
    //            .state
    //            .query_service
    //            .get_episodes_for_season(&season.id);
    //        episodes.into_iter().next()
    //    } else {
    //        None
    //    }
    //};

    //// Calculate series progress percentage
    //let series_progress = service.get_series_progress(&series_id, watch_state);
    //let unwatched_count = service.get_unwatched_count(&series_id, watch_state);

    /*
    // Build button row based on watch state
    let mut buttons = vec![];

    if let Some((next_episode, resume_position)) = next_episode_info {
        // Determine if this is a continuation or fresh start
        let is_in_progress = resume_position.is_some() || series_progress > 0.0;

        let primary_label = if is_in_progress {
            if let Some(pos) = resume_position {
                format!(
                    "Continue S{:02}E{:02}",
                    next_episode.season_number.value(),
                    next_episode.episode_number.value()
                )
            } else {
                format!(
                    "Continue S{:02}E{:02}",
                    next_episode.season_number.value(),
                    next_episode.episode_number.value()
                )
            }
        } else {
            "Play".to_string()
        };

        // Primary action button
        buttons.push(
            button(
                row![
                    icon_text(Icon::Play),
                    Space::new().width(8),
                    text(primary_label).size(16)
                ]
                .align_y(iced::Alignment::Center),
            )
            .padding([10, 20])
            .on_press(Message::PlaySeriesNextEpisode(series_id.clone()))
            .style(theme::Button::Primary.style())
            .into(),
        );

        // Add "Play from Beginning" if we're not already at the beginning
        if is_in_progress
            && first_episode.is_some()
            && first_episode
                .as_ref()
                .map(|e| e.id != next_episode.id)
                .unwrap_or(false)
        {
            if let Some(first_ep) = first_episode {
                let series_details = match &series_ref.details {
                    crate::infrastructure::api_types::MediaDetailsOption::Details(
                        crate::infrastructure::api_types::TmdbDetails::Series(details),
                    ) => Some(details),
                    _ => None,
                };
                let legacy_file = crate::infrastructure::api_types::episode_reference_to_legacy(
                    &first_ep,
                    series_details,
                );
                buttons.push(
                    button(
                        row![
                            icon_text(Icon::SkipBack),
                            Space::new().width(8),
                            text("Play from Beginning").size(16)
                        ]
                        .align_y(iced::Alignment::Center),
                    )
                    .padding([10, 20])
                    .on_press(Message::PlayMediaWithId(
                        legacy_file,
                        MediaID::Episode(first_ep.id.clone()),
                    ))
                    .style(theme::Button::Secondary.style())
                    .into(),
                );
            }
        }
    } else if let Some(first_ep) = first_episode {
        // No watch state, just show Play button for first episode
        let series_details = match &series_ref.details {
            crate::infrastructure::api_types::MediaDetailsOption::Details(
                crate::infrastructure::api_types::TmdbDetails::Series(details),
            ) => Some(details),
            _ => None,
        };
        let legacy_file = crate::infrastructure::api_types::episode_reference_to_legacy(
            &first_ep,
            series_details,
        );
        buttons.push(
            button(
                row![
                    icon_text(Icon::Play),
                    Space::new().width(8),
                    text("Play").size(16)
                ]
                .align_y(iced::Alignment::Center),
            )
            .padding([10, 20])
            .on_press(Message::PlayMediaWithId(
                legacy_file,
                MediaID::Episode(first_ep.id.clone()),
            ))
            .style(theme::Button::Primary.style())
            .into(),
        );
    }

    // Add progress info if available
    if series_progress > 0.0 {
        details = details.push(
            text(format!(
                "{} unwatched episodes • {:.0}% complete",
                unwatched_count,
                series_progress * 100.0
            ))
            .size(14)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );
    }

    // Add button row if we have buttons
    if !buttons.is_empty() {
        details = details.push(Space::new().height(10));
        details = details.push(row(buttons).spacing(10).align_y(iced::Alignment::Center));
    }*/

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

    // Seasons carousel - use the seasons we fetched above
    if !seasons.is_empty() {
        content = content.push(Space::new().height(20));

        if let Some(carousel_state) =
            &state.domains.ui.state.show_seasons_carousel
        {
            // Build season cards lazily using windowed carousel with media_card!
            let seasons_vec = seasons.clone();
            let section =
                crate::domains::ui::views::carousel::windowed_media_carousel(
                    "show_seasons".to_string(),
                    "Seasons",
                    seasons_vec.len(),
                    carousel_state,
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
                            .unwrap_or_else(|| String::from("\u{00A0}")); // non-breaking space to keep height

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
                                on_play: Message::ViewSeason(s.series_id, s.id),
                                hover_icon: lucide_icons::Icon::List,
                                is_hovered: false,
                            }
                        }
                    })
                    },
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
        .size(ImageSize::Full)
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
        if let Some(ep_cs) = &state.domains.ui.state.season_episodes_carousel {
            let eps_vec = episodes.clone();
            let ep_section =
                crate::domains::ui::views::carousel::windowed_media_carousel(
                    "season_episodes".to_string(),
                    "Episodes",
                    eps_vec.len(),
                    ep_cs,
                    move |idx| {
                        eps_vec.get(idx).map(|e| {
                        // Build episode title/subtitle
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
