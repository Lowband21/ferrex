use crate::{
    domains::ui::theme,
    domains::{
        self,
        metadata::image_types::{ImageSize, Priority},
        ui::{messages::Message, widgets::image_for::image_for},
    },
    infrastructure::api_types::{MediaReference, SeasonReference},
    state_refactored::State,
};
use ferrex_core::{api_types::MediaId, EpisodeID, SeasonID, SeriesID};
use iced::{
    widget::{button, column, container, row, scrollable, text, Space, Stack},
    Element, Length,
};

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_tv_show_detail<'a>(state: &'a State, _show_name: &'a str) -> Element<'a, Message> {
    let mut content = column![].spacing(20);

    // Add dynamic spacing at the top based on backdrop dimensions
    let window_width = state.window_size.width;
    let window_height = state.window_size.height;
    let content_offset = state
        .domains
        .ui
        .state
        .background_shader_state
        .calculate_content_offset_with_height(window_width, window_height);
    content = content.push(Space::with_height(Length::Fixed(content_offset)));

    // Get the series ID from the current view state
    let series_id = match &state.domains.ui.state.view {
        crate::domains::ui::types::ViewState::TvShowDetail { series_id, .. } => series_id.clone(),
        _ => {
            // Should not happen, but handle gracefully
            return scrollable(
                content.push(
                    text("Error: Invalid view state")
                        .size(16)
                        .color(theme::MediaServerTheme::ERROR),
                ),
            )
            .into();
        }
    };

    // Use MediaQueryService (clean architecture)
    let series_ref = state
        .domains
        .media
        .state
        .query_service
        .get_series(&series_id);

    if let Some(series_ref) = series_ref {
        // Use MediaQueryService to get seasons (clean architecture)
        let seasons = state
            .domains
            .media
            .state
            .query_service
            .get_seasons_for_series(&series_id);

        let poster_element: Element<Message> = image_for(MediaId::Series(series_id.clone()))
            .size(ImageSize::Full)
            .width(Length::Fixed(300.0))
            .height(Length::Fixed(450.0))
            .priority(Priority::Visible)
            .into();

        // Details column
        let mut details = column![].spacing(15).padding(20).width(Length::Fill);

        // Title
        details = details.push(
            text(series_ref.title.as_str().to_string())
                .size(32)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
        );

        // Extract details from the series reference
        let (description, genres, rating, total_episodes) = match &series_ref.details {
            crate::infrastructure::api_types::MediaDetailsOption::Details(
                crate::infrastructure::api_types::TmdbDetails::Series(series_details),
            ) => {
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
                    series_details.overview.clone(),
                    series_details.genres.clone(),
                    series_details.vote_average.map(|v| v as f32),
                    series_details.number_of_episodes,
                )
            }
            _ => {
                log::warn!("Series {} has no TMDB details", series_ref.title.as_str());
                (None, vec![], None, None)
            }
        };

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
        );

        // Rating
        if let Some(rating) = rating {
            details = details.push(
                text(format!("★ {:.1}", rating))
                    .size(16)
                    .color(theme::MediaServerTheme::ACCENT_BLUE),
            );
        }

        // Play button row - find first episode to play using the seasons we already have
        let first_episode = {
            let first_season = seasons
                .iter()
                .find(|season| season.season_number.value() == 1)
                .or_else(|| seasons.first());

            if let Some(season) = first_season {
                // Use MediaQueryService to get episodes (clean architecture)
                let episodes = state
                    .domains
                    .media
                    .state
                    .query_service
                    .get_episodes_for_season(&season.id);
                episodes.into_iter().next()
            } else {
                None
            }
        };

        if let Some(episode) = first_episode {
            let series_details = match &series_ref.details {
                crate::infrastructure::api_types::MediaDetailsOption::Details(
                    crate::infrastructure::api_types::TmdbDetails::Series(details),
                ) => Some(details),
                _ => None,
            };
            let legacy_file = crate::infrastructure::api_types::episode_reference_to_legacy(
                &episode,
                series_details,
            );
            let button_row = crate::domains::ui::components::create_action_button_row(
                Message::PlayMediaWithId(
                    legacy_file,
                    ferrex_core::api_types::MediaId::Episode(episode.id.clone()),
                ),
                vec![], // No additional buttons yet
            );
            details = details.push(Space::with_height(10));
            details = details.push(button_row);
        }

        // Description
        if let Some(desc) = description {
            details = details.push(Space::with_height(10));
            details = details.push(
                container(
                    text(desc)
                        .size(14)
                        .color(theme::MediaServerTheme::TEXT_PRIMARY),
                )
                .width(Length::Fill)
                .padding(10),
            );
        }

        // Genres
        if !genres.is_empty() {
            details = details.push(
                text(format!("Genres: {}", genres.join(", ")))
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            );
        }

        // Content row with poster and details
        let info_row = row![poster_element, details]
            .spacing(10)
            .align_y(iced::Alignment::Start);

        content = content.push(info_row);

        // Seasons carousel - use the seasons we fetched at the beginning
        if !seasons.is_empty() {
            content = content.push(Space::with_height(20));

            if let Some(carousel_state) = &state.domains.ui.state.show_seasons_carousel {
                // Pass owned seasons to components - idiomatic Iced pattern
                // Components take ownership to avoid lifetime issues
                let season_cards: Vec<_> = seasons
                    .iter()
                    .cloned()
                    .map(|season| {
                        crate::domains::ui::components::season_reference_card_with_state(
                            // We need to pass watch status here
                            season,
                            false,
                            Some(state),
                            None,
                        )
                    })
                    .collect();

                let seasons_carousel = crate::domains::ui::views::carousel::media_carousel(
                    "show_seasons".to_string(),
                    "Seasons",
                    season_cards,
                    carousel_state,
                );

                content = content.push(seasons_carousel);
            }
        }
    } else {
        // Loading state
        content = content.push(
            container(
                column![
                    text("⏳").size(48),
                    text("Loading show details...")
                        .size(16)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY)
                ]
                .spacing(10),
            )
            .padding(100),
        );
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
    let aspect_button = crate::domains::ui::components::create_backdrop_aspect_button(state);

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
    series_id: &'a SeriesID,
    season_id: &'a SeasonID,
) -> Element<'a, Message> {
    let mut content = column![].spacing(20);

    // Add dynamic spacing at the top based on backdrop dimensions
    let window_width = state.window_size.width;
    let window_height = state.window_size.height;
    let content_offset = state
        .domains
        .ui
        .state
        .background_shader_state
        .calculate_content_offset_with_height(window_width, window_height);
    content = content.push(Space::with_height(Length::Fixed(content_offset)));

    // NEW ARCHITECTURE: Get data from MediaStore
    let (series_ref, season_ref, episodes) =
        if let Ok(store) = state.domains.media.state.media_store.read() {
            let series_ref = if let Some(MediaReference::Series(series)) =
                store.get(&MediaId::Series(series_id.clone()))
            {
                Some(series.clone())
            } else {
                None
            };

            let season_ref = if let Some(MediaReference::Season(season)) =
                store.get(&MediaId::Season(season_id.clone()))
            {
                Some(season.clone())
            } else {
                None
            };

            // SINGLE SOURCE OF TRUTH: Always get episodes from MediaStore
            let episodes: Vec<_> = if let Some(ref season) = season_ref {
                store
                    .get_episodes(season.id.as_str())
                    .into_iter()
                    .cloned()
                    .collect()
            } else {
                vec![]
            };

            (series_ref, season_ref, episodes)
        } else {
            (None, None, vec![])
        };

    // Get series name for display
    let series_name = series_ref
        .as_ref()
        .map(|s| s.title.as_str())
        .unwrap_or("Unknown Series");

    // Check if we have season reference
    if let Some(season_ref) = season_ref {
        // Season poster using unified image system
        let poster_element: Element<Message> = image_for(MediaId::Season(season_id.to_owned()))
            .size(ImageSize::Full)
            .width(Length::Fixed(300.0))
            .height(Length::Fixed(450.0))
            .priority(Priority::Visible)
            .into();

        // Details column
        let mut details = column![].spacing(15).padding(20).width(Length::Fill);

        // Extract season details from the reference
        let (season_name, overview) = match &season_ref.details {
            crate::infrastructure::api_types::MediaDetailsOption::Details(
                crate::infrastructure::api_types::TmdbDetails::Season(details),
            ) => (Some(details.name.clone()), details.overview.clone()),
            _ => (None, None),
        };

        // Title
        let season_number = season_ref.season_number.value();
        let display_title = if let Some(name) = season_name.filter(|n| !n.is_empty()) {
            format!("{} - {}", series_name, name)
        } else if season_number == 0 {
            format!("{} - Specials", series_name)
        } else {
            format!("{} - Season {}", series_name, season_number)
        };

        details = details.push(
            text(display_title)
                .size(32)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
        );

        // Episode count
        details = details.push(
            text(format!("{} Episodes", episodes.len()))
                .size(16)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );

        // Air date range if available
        let air_dates: Vec<String> = episodes
            .iter()
            .filter_map(|e| match &e.details {
                crate::infrastructure::api_types::MediaDetailsOption::Details(
                    crate::infrastructure::api_types::TmdbDetails::Episode(details),
                ) => details.air_date.clone(),
                _ => None,
            })
            .collect();

        if !air_dates.is_empty() {
            let empty_string = String::new();
            let first_date = air_dates.iter().min().unwrap_or(&empty_string);
            let last_date = air_dates.iter().max().unwrap_or(&empty_string);

            let date_range = if first_date == last_date {
                first_date.to_string()
            } else {
                format!("{} - {}", first_date, last_date)
            };

            details = details.push(
                text(format!("Aired: {}", date_range))
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            );
        }

        // Total duration
        let total_duration: f64 = episodes
            .iter()
            .filter_map(|e| match &e.details {
                crate::infrastructure::api_types::MediaDetailsOption::Details(
                    crate::infrastructure::api_types::TmdbDetails::Episode(details),
                ) => details.runtime.map(|r| r as f64 * 60.0),
                _ => None,
            })
            .sum();

        if total_duration > 0.0 {
            let hours = (total_duration / 3600.0) as u32;
            let minutes = ((total_duration % 3600.0) / 60.0) as u32;
            details = details.push(
                text(format!("Total Runtime: {}h {}m", hours, minutes))
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            );
        }

        // Play button row - play first episode of the season
        if let Some(first_episode) = episodes.first() {
            let series_details = series_ref.as_ref().and_then(|s| match &s.details {
                crate::infrastructure::api_types::MediaDetailsOption::Details(
                    crate::infrastructure::api_types::TmdbDetails::Series(details),
                ) => Some(details.clone()),
                _ => None,
            });
            let legacy_file = crate::infrastructure::api_types::episode_reference_to_legacy(
                first_episode,
                series_details.as_ref(),
            );
            let button_row = crate::domains::ui::components::create_action_button_row(
                Message::PlayMediaWithId(
                    legacy_file,
                    ferrex_core::api_types::MediaId::Episode(first_episode.id.clone()),
                ),
                vec![], // No additional buttons yet
            );
            details = details.push(Space::with_height(10));
            details = details.push(button_row);
        }

        // Overview/Description
        if let Some(desc) = overview {
            details = details.push(Space::with_height(10));
            details = details.push(
                container(
                    text(desc)
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

        // Episodes carousel
        if !episodes.is_empty() {
            content = content.push(Space::with_height(20));

            if let Some(carousel_state) = &state.domains.ui.state.season_episodes_carousel {
                // Get visible range for virtualization
                let visible_range = carousel_state.get_visible_range();

                // Create episode cards directly from references
                let episode_cards: Vec<_> = episodes
                    .iter()
                    .enumerate()
                    .map(|(index, episode)| {
                        // Create episode card directly
                        let title = match &episode.details {
                            crate::infrastructure::api_types::MediaDetailsOption::Details(
                                crate::infrastructure::api_types::TmdbDetails::Episode(details),
                            ) => details.name.clone(),
                            _ => format!("Episode {}", episode.episode_number.value()),
                        };

                        let mut info_parts =
                            vec![format!("E{:02}", episode.episode_number.value())];

                        // Add duration if available
                        if let Some(metadata) = &episode.file.media_file_metadata {
                            if let Some(dur) = metadata.duration {
                                info_parts.push(format!("{} min", (dur / 60.0) as u32));
                            }
                        }

                        // Add air date if available
                        if let crate::infrastructure::api_types::MediaDetailsOption::Details(
                            crate::infrastructure::api_types::TmdbDetails::Episode(details),
                        ) = &episode.details
                        {
                            if let Some(air_date) = &details.air_date {
                                if let Ok(date) =
                                    chrono::NaiveDate::parse_from_str(air_date, "%Y-%m-%d")
                                {
                                    info_parts.push(date.format("%Y").to_string());
                                }
                            }
                        }

                        let info = info_parts.join(" • ");

                        // Determine image priority based on visibility
                        let image_priority = if visible_range.contains(&index) {
                            Priority::Visible
                        } else if index < visible_range.start
                            && index >= visible_range.start.saturating_sub(2)
                        {
                            Priority::Preload
                        } else if index > visible_range.end && index <= visible_range.end + 2 {
                            Priority::Preload
                        } else {
                            Priority::Background
                        };

                        // Get watch progress for this episode
                        let watch_progress = state
                            .domains
                            .media
                            .state
                            .get_media_progress(&MediaId::Episode(episode.id.clone()));

                        // Create episode thumbnail with watch progress
                        let mut episode_image = image_for(MediaId::Episode(episode.id.clone()))
                            .size(ImageSize::Thumbnail)
                            .width(Length::Fixed(250.0))
                            .height(Length::Fixed(140.0))
                            .priority(image_priority);

                        // Add watch progress - default to unwatched (0.0) if no watch state
                        let progress = watch_progress.unwrap_or(0.0);
                        episode_image = episode_image.progress(progress);

                        // Create a clickable episode card
                        button(
                            container(
                                column![
                                    // Episode thumbnail using unified image system
                                    episode_image,
                                    // Episode info
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
                                    .width(Length::Fixed(250.0))
                                    .height(Length::Fixed(80.0))
                                    .clip(true)
                                ]
                                .spacing(5),
                            )
                            .width(Length::Fixed(250.0))
                            .height(Length::Fixed(230.0)),
                        )
                        .on_press(Message::ViewEpisode(episode.id.clone()))
                        .padding(0)
                        .style(theme::Button::MediaCard.style())
                        .into()
                    })
                    .collect();

                let episodes_carousel = crate::domains::ui::views::carousel::media_carousel(
                    "season_episodes".to_string(),
                    "Episodes",
                    episode_cards,
                    carousel_state,
                );

                content = content.push(episodes_carousel);
            }
        }
    } else {
        // Loading state
        content = content.push(
            container(
                column![
                    text("⏳").size(48),
                    text("Loading season details...")
                        .size(16)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY)
                ]
                .spacing(10),
            )
            .padding(100),
        );
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
    let aspect_button = crate::domains::ui::components::create_backdrop_aspect_button(state);

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
    let mut content = column![].spacing(20).padding(20);

    // Add dynamic spacing at the top based on backdrop dimensions
    let window_width = state.window_size.width;
    let window_height = state.window_size.height;
    let content_offset = state
        .domains
        .ui
        .state
        .background_shader_state
        .calculate_content_offset_with_height(window_width, window_height);
    content = content.push(Space::with_height(Length::Fixed(content_offset)));

    // NEW ARCHITECTURE: Find the episode from MediaStore
    let (episode_ref, series_name) = if let Ok(store) = state.domains.media.state.media_store.read()
    {
        let episode = if let Some(MediaReference::Episode(ep)) =
            store.get(&MediaId::Episode(episode_id.clone()))
        {
            Some(ep.clone())
        } else {
            None
        };

        let series_name = if let Some(ref ep) = episode {
            if let Ok(series_id) = SeriesID::new(ep.series_id.as_str().to_string()) {
                if let Some(MediaReference::Series(series)) = store.get(&MediaId::Series(series_id))
                {
                    series.title.as_str().to_string()
                } else {
                    "Unknown Series".to_string()
                }
            } else {
                "Unknown Series".to_string()
            }
        } else {
            "Unknown Series".to_string()
        };

        (episode, series_name)
    } else {
        (None, "Unknown Series".to_string())
    };

    if let Some(episode) = episode_ref {
        // Get watch progress for this episode
        let watch_progress = state
            .domains
            .media
            .state
            .get_media_progress(&MediaId::Episode(episode_id.to_owned()));

        // Episode still/thumbnail using unified image system
        let mut episode_still = image_for(MediaId::Episode(episode_id.to_owned()))
            .size(ImageSize::Thumbnail)
            .width(Length::Fixed(640.0))
            .height(Length::Fixed(360.0))
            .priority(Priority::Visible);

        // Add watch progress - default to unwatched (0.0) if no watch state
        let progress = watch_progress.unwrap_or(0.0);
        episode_still = episode_still.progress(progress);

        let still_element: Element<Message> = episode_still.into();

        // Details column
        let mut details = column![].spacing(15).padding(20).width(Length::Fill);

        // Extract episode details
        let (episode_name, overview, air_date, runtime, vote_average) = match &episode.details {
            crate::infrastructure::api_types::MediaDetailsOption::Details(
                crate::infrastructure::api_types::TmdbDetails::Episode(details),
            ) => (
                Some(details.name.clone()),
                details.overview.clone(),
                details.air_date.clone(),
                details.runtime,
                details.vote_average,
            ),
            _ => (None, None, None, None, None),
        };

        // Title
        let title =
            episode_name.unwrap_or_else(|| format!("Episode {}", episode.episode_number.value()));
        details = details.push(
            text(format!(
                "{} - S{:02}E{:02}: {}",
                series_name,
                episode.season_number.value(),
                episode.episode_number.value(),
                title
            ))
            .size(28)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        );

        // Episode info
        let mut info_parts = Vec::new();
        if let Some(date) = air_date {
            info_parts.push(date);
        }
        if let Some(runtime) = runtime {
            info_parts.push(format!("{} min", runtime));
        }

        // Add watch status
        if let Some(progress) = watch_progress {
            if state
                .domains
                .media
                .state
                .is_watched(&MediaId::Episode(episode_id.to_owned()))
            {
                info_parts.push("✓ Watched".to_string());
            } else if progress > 0.0 {
                let percentage = (progress * 100.0) as u32;
                info_parts.push(format!("{}% watched", percentage));
            }
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

        // File info
        if let Some(metadata) = &episode.file.media_file_metadata {
            let mut file_info = vec![format!("File: {}", episode.file.filename)];

            if let Some(duration) = metadata.duration {
                let minutes = (duration / 60.0) as u32;
                file_info.push(format!("Duration: {} min", minutes));
            }

            if let Some(width) = metadata.width {
                if let Some(height) = metadata.height {
                    file_info.push(format!("Resolution: {}x{}", width, height));
                }
            }

            details = details.push(Space::with_height(10));
            details = details.push(
                text(file_info.join(" • "))
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            );
        }

        // Play button - convert to legacy MediaFile for playback
        let series_details = if let Ok(store) = state.domains.media.state.media_store.read() {
            if let Ok(series_id) = SeriesID::new(episode.series_id.as_str().to_string()) {
                if let Some(MediaReference::Series(series)) = store.get(&MediaId::Series(series_id))
                {
                    match &series.details {
                        crate::infrastructure::api_types::MediaDetailsOption::Details(
                            crate::infrastructure::api_types::TmdbDetails::Series(details),
                        ) => Some(details.clone()),
                        _ => None,
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let legacy_file = crate::infrastructure::api_types::episode_reference_to_legacy(
            &episode,
            series_details.as_ref(),
        );
        let button_row = crate::domains::ui::components::create_action_button_row(
            Message::PlayMediaWithId(
                legacy_file,
                ferrex_core::api_types::MediaId::Episode(episode.id.clone()),
            ),
            vec![], // No additional buttons yet
        );
        details = details.push(Space::with_height(10));
        details = details.push(button_row);

        // Overview/Description
        if let Some(desc) = overview {
            details = details.push(Space::with_height(20));
            details = details.push(
                container(
                    text(desc)
                        .size(14)
                        .color(theme::MediaServerTheme::TEXT_PRIMARY),
                )
                .width(Length::Fill)
                .padding(10),
            );
        }

        // Content layout
        content = content.push(column![still_element, Space::with_height(20), details].spacing(10));
    } else {
        // Episode not found
        content = content.push(
            container(
                column![
                    text("❌").size(48),
                    text("Episode not found")
                        .size(16)
                        .color(theme::MediaServerTheme::ERROR)
                ]
                .spacing(10),
            )
            .padding(100),
        );
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
    let aspect_button = crate::domains::ui::components::create_backdrop_aspect_button(state);

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
