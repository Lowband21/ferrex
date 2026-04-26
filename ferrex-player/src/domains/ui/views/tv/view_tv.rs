use std::f32::consts::PI;

use crate::{
    common::ui_utils::{Icon, icon_text},
    domains::{
        media::selectors,
        ui::{
            components,
            interaction_ui::InteractionMessage,
            menu::{
                MenuButton, PosterMenuMessage,
                watched_button_mode_for_media_uuid,
            },
            messages::UiMessage,
            playback_ui::PlaybackMessage,
            shell_ui::UiShellMessage,
            theme,
            views::{
                grid::{
                    macros::{
                        ThemeColorAccess, parse_hex_color, truncate_text,
                    },
                    types::CardSize,
                },
                virtual_carousel::{self, types::CarouselKey},
            },
            widgets::image_for::image_for,
        },
    },
    infra::shader_widgets::poster::{
        PosterFace, PosterInstanceKey, WatchButtonMode,
        animation::AnimationBehavior,
    },
    infra::theme::accent,
    media_card,
    state::State,
};

use ferrex_core::player_prelude::{
    MediaIDLike, SeasonLike, SeriesDetailsLike, SeriesLike,
};

use ferrex_model::{
    EpisodeID, ImageSize, MediaID, Priority, SeasonID, SeasonReference,
    SeriesID,
};
use rkyv::option::ArchivedOption;

use iced::{
    Element, Length,
    widget::{Space, Stack, button, column, container, mouse_area, row, text},
};

fn first_episode_for_season(
    state: &State,
    season_id: SeasonID,
) -> Option<EpisodeID> {
    let mut episodes = state
        .domains
        .ui
        .state
        .repo_accessor
        .get_season_episodes(&season_id)
        .unwrap_or_default();
    episodes.sort_by_key(|episode| episode.episode_number.value());
    episodes.first().map(|episode| episode.id)
}

fn first_episode_for_series(
    state: &State,
    series_id: SeriesID,
) -> Option<EpisodeID> {
    let mut seasons = state
        .domains
        .ui
        .state
        .repo_accessor
        .get_series_seasons(&series_id)
        .unwrap_or_default();
    seasons.sort_by_key(|season| season.season_number.value());

    seasons
        .into_iter()
        .find_map(|season| first_episode_for_season(state, season.id))
}

fn season_has_watch_state(state: &State, season_id: SeasonID) -> bool {
    state
        .domains
        .ui
        .state
        .repo_accessor
        .get_season_episodes(&season_id)
        .unwrap_or_default()
        .into_iter()
        .any(|episode| {
            state
                .domains
                .media
                .state
                .has_watch_state(&MediaID::Episode(episode.id))
        })
}

fn series_has_watch_state(state: &State, series_id: SeriesID) -> bool {
    state
        .domains
        .ui
        .state
        .repo_accessor
        .get_series_seasons(&series_id)
        .unwrap_or_default()
        .into_iter()
        .any(|season| season_has_watch_state(state, season.id))
}

fn episode_primary_action_label(
    state: &State,
    episode_id: EpisodeID,
) -> &'static str {
    if state
        .domains
        .media
        .state
        .resume_position(&MediaID::Episode(episode_id))
        .is_some()
    {
        "Resume episode"
    } else {
        "Play"
    }
}

fn start_over_button_for_episode<'a>(
    episode_id: EpisodeID,
) -> Element<'a, UiMessage> {
    button(
        row![
            icon_text(Icon::Rewind),
            text("Start from beginning").size(16)
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    )
    .on_press(
        PlaybackMessage::PlayMediaWithIdFromStart(MediaID::Episode(episode_id))
            .into(),
    )
    .padding([10, 20])
    .style(theme::Button::DetailAction.style())
    .into()
}

fn watched_action_button_for_media<'a>(
    state: &State,
    media_uuid: uuid::Uuid,
    instance_key: PosterInstanceKey,
) -> Element<'a, UiMessage> {
    let watch_button_mode =
        watched_button_mode_for_media_uuid(state, media_uuid);
    let (watch_label, watch_icon) = match watch_button_mode {
        WatchButtonMode::MarkUnwatched => ("Unwatch", Icon::X),
        WatchButtonMode::MarkWatched | WatchButtonMode::StaticWatched => {
            ("Watched", Icon::Check)
        }
    };

    button(
        row![icon_text(watch_icon), text(watch_label).size(16)]
            .spacing(8)
            .align_y(iced::Alignment::Center),
    )
    .on_press(UiMessage::PosterMenu(PosterMenuMessage::ButtonClicked(
        instance_key,
        MenuButton::Watched,
    )))
    .padding([10, 20])
    .style(theme::Button::DetailAction.style())
    .into()
}

fn season_carousel_card<'a>(
    state: &'a State,
    season: SeasonReference,
    carousel_key: &CarouselKey,
    poster_quality: ferrex_model::ImageSize,
) -> Element<'a, UiMessage> {
    let card_size = CardSize::Medium;
    let (width, height) = card_size.dimensions();
    let radius = card_size.radius();
    let (title_size, subtitle_size) = card_size.text_sizes();

    let title = if season.season_number.value() == 0 {
        "Specials".to_string()
    } else {
        format!("Season {}", season.season_number.value())
    };
    let subtitle = format!("{} episodes", season.details.episode_count);

    let instance_key =
        PosterInstanceKey::new(season.id.to_uuid(), Some(carousel_key.clone()));
    let is_hovered =
        state.domains.ui.state.hovered_media_id.as_ref() == Some(&instance_key);

    let mut img = image_for(season.id.to_uuid())
        .iid(season.details.primary_poster_iid)
        .skip_request(season.details.primary_poster_iid.is_none())
        .size(poster_quality)
        .radius(radius)
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .animation_behavior(AnimationBehavior::default())
        .placeholder(lucide_icons::Icon::Tv)
        .priority(Priority::Preload)
        .is_hovered(is_hovered)
        .watch_button_mode(watched_button_mode_for_media_uuid(
            state,
            season.id.to_uuid(),
        ))
        .carousel_key(carousel_key.clone())
        .on_play(UiMessage::NoOp)
        .on_click(
            UiShellMessage::ViewSeason(season.series_id, season.id).into(),
        );

    if let Some(theme_color_str) = season.theme_color() {
        if let Ok(color) = parse_hex_color(theme_color_str) {
            img = img.theme_color(color);
        } else {
            log::warn!(
                "Could not parse theme_color_str {} for season {:?}",
                theme_color_str,
                season.id.to_uuid()
            );
        }
    }

    let (face, rotation_override) = if let Some(menu_state) =
        state.domains.ui.state.poster_menu_states.get(&instance_key)
    {
        (menu_state.face_from_angle(), Some(menu_state.angle))
    } else if state.domains.ui.state.poster_menu_open.as_ref()
        == Some(&instance_key)
    {
        (PosterFace::Back, Some(PI))
    } else {
        (PosterFace::Front, None)
    };
    img = img.face(face);
    if let Some(rot) = rotation_override {
        img = img.rotation_y(rot);
    }

    let image_element: Element<'_, UiMessage> = img.into();
    let image_with_hover = mouse_area(image_element)
        .on_enter(InteractionMessage::MediaHovered(instance_key.clone()).into())
        .on_exit(InteractionMessage::MediaUnhovered(instance_key).into());

    let poster_element: Element<'_, UiMessage> = button(image_with_hover)
        .padding(0)
        .style(theme::Button::MediaCard.style())
        .into();

    use crate::infra::constants::animation;
    let h_padding = animation::calculate_horizontal_padding(width);
    let v_padding = animation::calculate_vertical_padding(height);
    let container_width = width + h_padding * 2.0;
    let container_height = height + v_padding * 2.0;

    let poster_with_overlay_element: Element<'_, UiMessage> =
        container(poster_element)
            .width(Length::Fixed(container_width))
            .height(Length::Fixed(container_height))
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .into();

    let title_max_chars = ((width - 10.0) / (title_size as f32 * 0.6)) as usize;
    let subtitle_max_chars =
        ((width - 10.0) / (subtitle_size as f32 * 0.6)) as usize;
    let truncated_title = truncate_text(&title, title_max_chars);
    let truncated_subtitle = truncate_text(&subtitle, subtitle_max_chars);

    let card_content = column![
        poster_with_overlay_element,
        container(
            column![
                text(truncated_title)
                    .size(title_size)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                text(truncated_subtitle)
                    .size(subtitle_size)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY)
            ]
            .spacing(2)
        )
        .padding(5)
        .width(Length::Fixed(width))
        .height(Length::Fixed(60.0))
        .clip(true)
    ]
    .spacing(5);

    container(card_content)
        .width(Length::Fixed(container_width))
        .height(Length::Fixed(container_height + 65.0))
        .clip(false)
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
pub fn view_series_detail<'a>(
    state: &'a State,
    series_id: SeriesID,
) -> Element<'a, UiMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;
    let scaled_layout = &state.domains.ui.state.scaled_layout;
    let detail_poster_quality =
        state.domains.settings.display.detail_poster_quality;
    let library_poster_quality =
        state.domains.settings.display.library_poster_quality;

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
                                .size(fonts.title)
                                .color(theme::MediaServerTheme::TEXT_SECONDARY),
                            Space::new().height(10),
                            text("Repository error:")
                                .size(fonts.body)
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

    let mut content = column![];

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
            .size(fonts.display)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
    );

    let series_details = series.details();

    let series_poster_iid = match &series_details.primary_poster_iid {
        ArchivedOption::Some(iid) => Some(*iid),
        ArchivedOption::None => None,
    };

    // Apply theme color to poster if present
    let mut poster = image_for(media_id.to_uuid())
        .iid(series_poster_iid)
        .skip_request(series_poster_iid.is_none())
        .size(ImageSize::Poster(detail_poster_quality))
        .width(Length::Fixed(300.0))
        .height(Length::Fixed(450.0))
        .priority(Priority::Visible)
        .watch_button_mode(watched_button_mode_for_media_uuid(
            state,
            media_id.to_uuid(),
        ))
        .animation_behavior(AnimationBehavior::flip_then_fade());

    if let Some(hex) = series.theme_color()
        && let Ok(color) = parse_hex_color(hex)
    {
        poster = poster.theme_color(color);
    }
    let poster_id = media_id.to_uuid();
    let instance_key = PosterInstanceKey::standalone(poster_id);
    let (face, rotation_override) = if let Some(menu_state) =
        state.domains.ui.state.poster_menu_states.get(&instance_key)
    {
        (menu_state.face_from_angle(), Some(menu_state.angle))
    } else if state.domains.ui.state.poster_menu_open.as_ref()
        == Some(&instance_key)
    {
        (PosterFace::Back, Some(PI))
    } else {
        (PosterFace::Front, None)
    };
    poster = poster.face(face);
    if let Some(rot) = rotation_override {
        poster = poster.rotation_y(rot);
    }
    let poster_element: Element<UiMessage> = poster.into();

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
    let (series_details, description, rating, _total_episodes) = (
        Some(series_details),
        series_details.overview.as_ref(),
        series_details.vote_average.as_ref(),
        series_details.number_of_episodes.as_ref(),
    );

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
        let total_eps: u16 =
            seasons.iter().map(|s| s.details.episode_count).sum();
        details = details.push(
            text(format!("{} Seasons • {} Episodes", season_count, total_eps))
                .size(fonts.body)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );
    }

    // Rating
    if let Some(rating) = rating {
        details = details.push(
            text(format!("★ {:.1}", rating))
                .size(fonts.body)
                .color(accent()),
        );
    }

    // Play/Resume button – uses identity endpoint when available, falls back to local selection.
    if let Some(next_ep_id) =
        selectors::select_next_episode_for_series(state, series_id)
    {
        let primary_label = episode_primary_action_label(state, next_ep_id);
        let mut additional_buttons = Vec::new();
        if series_has_watch_state(state, series_id)
            && let Some(first_ep_id) =
                first_episode_for_series(state, series_id)
        {
            additional_buttons.push(start_over_button_for_episode(first_ep_id));
        }
        additional_buttons.push(watched_action_button_for_media(
            state,
            media_id.to_uuid(),
            instance_key.clone(),
        ));

        let button_row = components::create_action_button_row_with_label(
            primary_label,
            PlaybackMessage::PlaySeriesNextEpisode(series_id).into(),
            Some(PlaybackMessage::PlaySeriesNextEpisode(series_id).into()),
            additional_buttons,
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
                    .size(fonts.caption)
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
                .size(fonts.caption)
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
                        season_carousel_card(
                            state,
                            s.clone(),
                            &key,
                            ImageSize::Poster(library_poster_quality),
                        )
                    })
                },
                false,
                fonts,
                scaled_layout,
                0.0,
            );
            content = content.push(section);
        }
    }
    // Create the main content container
    let content_container = container(content).width(Length::Fill);

    // Calculate backdrop dimensions using centralized method
    let window_width = state.window_size.width;
    let window_height = state.window_size.height;
    let backdrop_dims = state
        .domains
        .ui
        .state
        .background_shader_state
        .calculate_backdrop_dimensions(window_width, window_height);

    // Create aspect ratio toggle button
    let aspect_button =
        crate::domains::ui::components::create_backdrop_aspect_button(state);

    // Position the button at bottom-right of backdrop
    let button_container = container(aspect_button)
        .padding([0, 20])
        .width(Length::Fill)
        .height(Length::Fixed(backdrop_dims.button_height))
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
) -> Element<'a, UiMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;
    let scaled_layout = &state.domains.ui.state.scaled_layout;
    let detail_poster_quality =
        state.domains.settings.display.detail_poster_quality;

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
                            .size(fonts.title)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                        Space::new().height(10),
                        text("Repository error:")
                            .size(fonts.body)
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

    let mut content = column![];

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

    let season_poster_iid = match &season.details.primary_poster_iid {
        ArchivedOption::Some(iid) => Some(*iid),
        ArchivedOption::None => None,
    };

    // Poster element
    let mut poster = image_for(season.id.to_uuid())
        .iid(season_poster_iid)
        .skip_request(season_poster_iid.is_none())
        .size(ImageSize::Poster(detail_poster_quality))
        .width(Length::Fixed(300.0))
        .height(Length::Fixed(450.0))
        .priority(Priority::Visible)
        .watch_button_mode(watched_button_mode_for_media_uuid(
            state,
            season.id.to_uuid(),
        ))
        .animation_behavior(AnimationBehavior::flip_then_fade());
    if let Some(hex) = season.theme_color()
        && let Ok(color) = parse_hex_color(hex)
    {
        poster = poster.theme_color(color);
    }
    let poster_id = season.id.to_uuid();
    let season_instance_key = PosterInstanceKey::standalone(poster_id);
    let (face, rotation_override) = if let Some(menu_state) = state
        .domains
        .ui
        .state
        .poster_menu_states
        .get(&season_instance_key)
    {
        (menu_state.face_from_angle(), Some(menu_state.angle))
    } else if state.domains.ui.state.poster_menu_open.as_ref()
        == Some(&season_instance_key)
    {
        (PosterFace::Back, Some(PI))
    } else {
        (PosterFace::Front, None)
    };
    poster = poster.face(face);
    if let Some(rot) = rotation_override {
        poster = poster.rotation_y(rot);
    }
    let poster_element: Element<UiMessage> = poster.into();

    // Details column
    let mut details = column![].spacing(15).padding(20).width(Length::Fill);

    // Title and episode count
    let season_number = season.details.season_number;
    let title = if season_number == 0 {
        "Specials".to_string()
    } else {
        format!("Season {}", season_number)
    };
    details = details.push(
        text(title)
            .size(fonts.display)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
    );

    let episode_count = season.num_episodes();
    details = details.push(
        text(format!("{} Episodes", episode_count))
            .size(fonts.body)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    );

    // Play button: play first in-progress or first unwatched episode in this season.
    if let Some(next_ep_id) =
        selectors::select_next_episode_for_season(state, *season_id)
    {
        let primary_label = episode_primary_action_label(state, next_ep_id);
        let mut additional_buttons = Vec::new();
        if season_has_watch_state(state, *season_id)
            && let Some(first_ep_id) =
                first_episode_for_season(state, *season_id)
        {
            additional_buttons.push(start_over_button_for_episode(first_ep_id));
        }
        additional_buttons.push(watched_action_button_for_media(
            state,
            season.id.to_uuid(),
            season_instance_key.clone(),
        ));

        let button_row = components::create_action_button_row_with_label(
            primary_label,
            PlaybackMessage::PlayMediaWithId(MediaID::Episode(next_ep_id))
                .into(),
            Some(
                PlaybackMessage::PlayMediaWithIdInMpv(MediaID::Episode(
                    next_ep_id,
                ))
                .into(),
            ),
            additional_buttons,
        );
        details = details.push(Space::new().height(10));
        details = details.push(button_row);
    }

    // Overview
    if let Some(desc) = season.details.overview.as_ref() {
        details = details.push(Space::new().height(10));
        details = details.push(
            container(
                text(desc.to_string())
                    .size(fonts.caption)
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
                        let subtitle_str = e.details.name.as_str();

                        media_card! {
                            type: Episode,
                            data: (e.clone()),
                            {
                                id: e.id.to_uuid(),
                                title: title_str.as_str(),
                                subtitle: subtitle_str,
                                image: {
                                    key: e.id.to_uuid(),
                                    type: thumbnail,
                                    fallback: "🎞",
                                },
                                size: Wide,
                                on_click: PlaybackMessage::PlayMediaWithId(
                                    MediaID::Episode(e.id),
                                )
                                .into(),
                                on_play: PlaybackMessage::PlayMediaWithId(
                                    MediaID::Episode(e.id),
                                )
                                .into(),
                                hover_icon: lucide_icons::Icon::Play,
                                is_hovered: false,
                            }
                        }
                    })
                },
                false,
                fonts,
                scaled_layout,
                0.0,
            );
            content = content.push(ep_section);
        }
    }

    // Create the main content container
    let content_container = container(content).width(Length::Fill);

    // Calculate backdrop dimensions using centralized method
    let window_width = state.window_size.width;
    let window_height = state.window_size.height;
    let backdrop_dims = state
        .domains
        .ui
        .state
        .background_shader_state
        .calculate_backdrop_dimensions(window_width, window_height);

    // Create aspect ratio toggle button
    let aspect_button =
        crate::domains::ui::components::create_backdrop_aspect_button(state);

    // Position the button at bottom-right of backdrop
    let button_container = container(aspect_button)
        .padding([0, 20])
        .width(Length::Fill)
        .height(Length::Fixed(backdrop_dims.button_height))
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
) -> Element<'a, UiMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;

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
                                .size(fonts.title)
                                .color(theme::MediaServerTheme::TEXT_SECONDARY),
                            Space::new().height(10),
                            text("Repository error:")
                                .size(fonts.body)
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

    let mut content = column![].padding(20);
    content = content.push(Space::new().height(Length::Fixed(content_offset)));

    let still_iid = match &episode.details.primary_still_iid {
        ArchivedOption::Some(iid) => Some(*iid),
        ArchivedOption::None => None,
    };

    // Episode still image: keep this as a plain landscape hero image rather than
    // forcing the portrait backface menu onto a 16:9 surface.
    let still = image_for(episode.id.to_uuid())
        .iid(still_iid)
        .skip_request(still_iid.is_none())
        .size(ImageSize::thumbnail())
        .width(Length::Fixed(640.0))
        .height(Length::Fixed(360.0))
        .priority(Priority::Visible)
        .menu_enabled(false);
    let episode_instance_key =
        PosterInstanceKey::standalone(episode.id.to_uuid());
    let still_element: Element<UiMessage> = still.into();

    // Details column
    let mut details = column![].spacing(15).padding(20).width(Length::Fill);

    // Title and info
    let (ep_name, overview, air_date, runtime, vote_average) = (
        Some(episode.details.name.to_string()),
        episode.details.overview.as_ref(),
        episode.details.air_date.as_ref(),
        episode.details.runtime.as_ref(),
        episode.details.vote_average.as_ref(),
    );

    let (season_number, ep_number) = (
        episode.details.season_number,
        episode.details.episode_number,
    );

    let title = ep_name.unwrap_or_else(|| format!("Episode {}", ep_number));
    details = details.push(
        text(format!("S{:02}E{:02}: {}", season_number, ep_number, title))
            .size(fonts.title_lg)
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
                .size(fonts.body)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );
    }

    let episode_media_id = MediaID::Episode(EpisodeID(episode.id.to_uuid()));
    let primary_label = if state
        .domains
        .media
        .state
        .resume_position(&episode_media_id)
        .is_some()
    {
        "Resume"
    } else {
        "Play"
    };

    let watch_button_mode =
        watched_button_mode_for_media_uuid(state, episode.id.to_uuid());
    let (watch_label, watch_icon) = match watch_button_mode {
        WatchButtonMode::MarkUnwatched => ("Unwatch", Icon::X),
        WatchButtonMode::MarkWatched | WatchButtonMode::StaticWatched => {
            ("Watched", Icon::Check)
        }
    };

    let watched_button = button(
        row![icon_text(watch_icon), text(watch_label).size(16)]
            .spacing(8)
            .align_y(iced::Alignment::Center),
    )
    .on_press(UiMessage::PosterMenu(PosterMenuMessage::ButtonClicked(
        episode_instance_key,
        MenuButton::Watched,
    )))
    .padding([10, 20])
    .style(theme::Button::DetailAction.style());

    let mut additional_buttons = Vec::new();
    if state.domains.media.state.has_watch_state(&episode_media_id) {
        let start_over_button = button(
            row![
                icon_text(Icon::Rewind),
                text("Start from beginning").size(16)
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        )
        .on_press(
            PlaybackMessage::PlayMediaWithIdFromStart(episode_media_id).into(),
        )
        .padding([10, 20])
        .style(theme::Button::DetailAction.style());
        additional_buttons.push(start_over_button.into());
    }
    additional_buttons.push(watched_button.into());

    // Play button
    let button_row = components::create_action_button_row_with_label(
        primary_label,
        PlaybackMessage::PlayMediaWithId(episode_media_id).into(),
        Some(PlaybackMessage::PlayMediaWithIdInMpv(episode_media_id).into()),
        additional_buttons,
    );
    details = details.push(Space::new().height(10));
    details = details.push(button_row);

    // Overview
    if let Some(desc) = overview {
        details = details.push(Space::new().height(20));
        details = details.push(
            container(
                text(desc.to_string())
                    .size(fonts.caption)
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

    // Calculate backdrop dimensions using centralized method
    let backdrop_dims = state
        .domains
        .ui
        .state
        .background_shader_state
        .calculate_backdrop_dimensions(window_width, window_height);

    // Create aspect ratio toggle button
    let aspect_button =
        crate::domains::ui::components::create_backdrop_aspect_button(state);

    // Position the button at bottom-right of backdrop
    let button_container = container(aspect_button)
        .padding([0, 20])
        .width(Length::Fill)
        .height(Length::Fixed(backdrop_dims.button_height))
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Bottom);

    Stack::new()
        .push(content_container)
        .push(button_container)
        .into()
}
