use iced::Task;
use std::collections::HashMap;

use crate::{
    api_types::{MediaReference, MovieReference},
    media_library::{self, MediaFile},
    messages::ui::Message,
    state::{State, ViewMode, ViewState},
    views::carousel::CarouselState,
};
use ferrex_core::{EpisodeID, SeasonID, SeriesID};

pub fn handle_view_details(state: &mut State, media: MediaFile) -> Task<Message> {
    log::info!("Viewing details for: {}", media.display_title());

    // Save current scroll position before navigating away
    state.save_scroll_position();

    // Determine if it's a movie or TV episode
    if media.is_tv_episode() {
        state.view = ViewState::EpisodeDetail {
            episode_id: EpisodeID::new(media.id.clone())
                .unwrap_or_else(|_| EpisodeID::new("unknown".to_string()).unwrap()),
            backdrop_handle: None,
        };
    } else {
        // NEW ARCHITECTURE: Find movie in MediaStore
        let movie_id = ferrex_core::MovieID::new(media.id.clone())
            .unwrap_or_else(|_| ferrex_core::MovieID::new("unknown".to_string()).unwrap());
        let media_id = ferrex_core::api_types::MediaId::Movie(movie_id);

        if let Ok(store) = state.media_store.read() {
            if let Some(MediaReference::Movie(movie)) = store.get(&media_id) {
                state.view = ViewState::MovieDetail {
                    movie: movie.clone(),
                    backdrop_handle: None,
                };
            } else {
                log::error!("Movie not found in MediaStore: {}", media.id);
                state.error_message = Some(format!("Movie not found: {}", media.display_title()));
            }
        }
    }

    // Update depth regions for the new detail view
    state.background_shader_state.update_depth_lines(
        &state.view,
        state.window_size.width,
        state.window_size.height,
    );

    Task::none()
}

pub fn handle_view_movie_details(state: &mut State, movie: MovieReference) -> Task<Message> {
    log::info!("Viewing movie details for: {}", movie.title.as_str());

    // Save current scroll position before navigating away
    state.save_scroll_position();

    // Check if we need to fetch details on-demand
    let fetch_task = if let Some(library_id) = state.current_library_id {
        let movie_media_id = ferrex_core::api_types::MediaId::Movie(movie.id.clone());
        state.fetch_media_details_on_demand(library_id, movie_media_id)
    } else {
        Task::none()
    };

    // Transition to new theme colors
    if let Some(hex) = &movie.theme_color {
        if let Ok(color) = crate::views::macros::parse_hex_color(hex) {
            // Apply stronger contrast for detail views
            let r = color.r * 0.2; // Very dark primary
            let g = color.g * 0.2;
            let b = color.b * 0.2;
            let primary_dark = iced::Color::from_rgb(r, g, b);

            // Secondary color is much lighter for strong gradient
            let secondary = iced::Color::from_rgb(
                (color.r * 0.8).min(1.0), // 4x brighter than primary
                (color.g * 0.8).min(1.0),
                (color.b * 0.8).min(1.0),
            );

            // Start color transition
            state
                .background_shader_state
                .color_transitions
                .transition_to(primary_dark, secondary);
        }
    }

    // Animate gradient center to new position
    let new_center = crate::transitions::generate_random_gradient_center();
    state
        .background_shader_state
        .gradient_transitions
        .transition_to(new_center);

    // Request backdrop if available but don't store it - view will pull reactively
    if let crate::api_types::MediaDetailsOption::Details(crate::api_types::TmdbDetails::Movie(
        movie_details,
    )) = &movie.details
    {
        if movie_details.backdrop_path.is_some() {
            let request = crate::image_types::ImageRequest::new(
                ferrex_core::api_types::MediaId::Movie(movie.id.clone()),
                crate::image_types::ImageSize::Backdrop,
            );
            // Just request the image if not in cache - view will pull it when ready
            if state.image_service.get(&request).is_none() {
                state.image_service.request_image(request);
            }
        }
    }

    // Start backdrop transition animation (view will detect backdrop presence)
    state
        .background_shader_state
        .backdrop_transitions
        .start_fade_slide(50.0); // 50px slide

    // Keep gradient effect active, just update the backdrop handle
    // The shader will overlay the backdrop on top of the gradient

    // Store the full movie reference directly (backdrop will be pulled reactively)
    state.view = ViewState::MovieDetail {
        movie,
        backdrop_handle: None, // Deprecated - kept for compatibility, will be removed
    };

    // Update depth regions for movie detail view
    state.background_shader_state.update_depth_lines(
        &state.view,
        state.window_size.width,
        state.window_size.height,
    );

    // Convert DomainMessage task to ui::Message task
    fetch_task.map(|_| Message::NoOp)
}

pub fn handle_view_tv_show(state: &mut State, series_id: SeriesID) -> Task<Message> {
    log::info!("Viewing TV show: {:?}", series_id);

    // No need to clear show details - using MediaStore as single source of truth

    // NEW ARCHITECTURE: Get seasons from MediaStore
    if let Ok(store) = state.media_store.read() {
        let seasons = store.get_seasons(series_id.as_str());
        if !seasons.is_empty() {
            state.show_seasons_carousel = Some(CarouselState::new_with_dimensions(
                seasons.len(),
                200.0, // Season card width (Medium size)
                15.0,  // Spacing
            ));
            if let Some(carousel) = &mut state.show_seasons_carousel {
                let available_width = state.window_size.width - 80.0;
                carousel.update_items_per_page(available_width);
            }
        }
    }

    // Save current scroll position before navigating away
    state.save_scroll_position();

    // NEW ARCHITECTURE: Get series from MediaStore
    let series_media_id = ferrex_core::api_types::MediaId::Series(series_id.clone());

    // Check if we need to fetch details on-demand
    let fetch_task = if let Some(library_id) = state.current_library_id {
        state
            .fetch_media_details_on_demand(library_id, series_media_id.clone())
            .map(|_| Message::NoOp)
    } else {
        Task::none()
    };

    if let Ok(store) = state.media_store.read() {
        if let Some(MediaReference::Series(series)) = store.get(&series_media_id) {
            // Transition to new theme colors
            if let Some(hex) = &series.theme_color {
                if let Ok(color) = crate::views::macros::parse_hex_color(hex) {
                    // Apply stronger contrast for detail views
                    let r = color.r * 0.2; // Very dark primary
                    let g = color.g * 0.2;
                    let b = color.b * 0.2;
                    let primary_dark = iced::Color::from_rgb(r, g, b);

                    // Secondary color is much lighter for strong gradient
                    let secondary = iced::Color::from_rgb(
                        (color.r * 0.8).min(1.0), // 4x brighter than primary
                        (color.g * 0.8).min(1.0),
                        (color.b * 0.8).min(1.0),
                    );

                    // Start color transition
                    state
                        .background_shader_state
                        .color_transitions
                        .transition_to(primary_dark, secondary);
                }
            }

            // Animate gradient center to new position
            let new_center = crate::transitions::generate_random_gradient_center();
            state
                .background_shader_state
                .gradient_transitions
                .transition_to(new_center);

            // Request backdrop if available but don't store it - view will pull reactively
            if let crate::api_types::MediaDetailsOption::Details(
                crate::api_types::TmdbDetails::Series(series_details),
            ) = &series.details
            {
                if series_details.backdrop_path.is_some() {
                    let request = crate::image_types::ImageRequest::new(
                        ferrex_core::api_types::MediaId::Series(series.id.clone()),
                        crate::image_types::ImageSize::Backdrop,
                    );
                    // Just request the image if not in cache - view will pull it when ready
                    if state.image_service.get(&request).is_none() {
                        state.image_service.request_image(request);
                    }
                }
            }

            // Start backdrop transition animation (view will detect backdrop presence)
            state
                .background_shader_state
                .backdrop_transitions
                .start_fade_slide(50.0); // 50px slide

            state.view = ViewState::TvShowDetail {
                series_id: series_id.clone(),
                backdrop_handle: None, // Deprecated - kept for compatibility, will be removed
            };
        } else {
            state.view = ViewState::TvShowDetail {
                series_id: series_id.clone(),
                backdrop_handle: None, // Deprecated - kept for compatibility, will be removed
            };
        }
    } else {
        state.view = ViewState::TvShowDetail {
            series_id: series_id.clone(),
            backdrop_handle: None, // Deprecated - kept for compatibility, will be removed
        };
    }

    // Update depth regions for TV show detail view
    state.background_shader_state.update_depth_lines(
        &state.view,
        state.window_size.width,
        state.window_size.height,
    );

    // Load show details using the unified API
    let server_url = state.server_url.clone();
    let series_id_str = series_id.as_str().to_string();

    // NEW ARCHITECTURE: Extract season and episode data from MediaStore
    let (series_seasons, episode_refs) = if let Ok(store) = state.media_store.read() {
        let seasons = store.get_seasons_owned(series_id.as_str());

        // Build episode map for all seasons
        let mut episodes_map = std::collections::HashMap::new();
        for season in &seasons {
            let episodes = store.get_episodes(season.id.as_str());
            if !episodes.is_empty() {
                episodes_map.insert(
                    season.id.as_str().to_string(),
                    episodes.into_iter().cloned().collect::<Vec<_>>(),
                );
            }
        }

        (Some(seasons), episodes_map)
    } else {
        (None, std::collections::HashMap::new())
    };

    // Store seasons in state for UI rendering
    if let Some(seasons) = series_seasons.clone() {
        state.current_show_seasons = seasons;
    }

    // The existing fetch_media_details call is still needed for loading the TvShowDetails
    // But we'll also return the fetch_task to ensure details are fetched if needed
    let existing_task = if let Some(library_id) = state.current_library_id {
        let media_id = ferrex_core::MediaId::Series(series_id.clone());
        Task::perform(
            media_library::fetch_media_details(server_url, library_id, media_id),
            move |result| match result {
                Ok(MediaReference::Series(series_ref)) => {
                    // Debug: Log the series we're loading
                    log::info!(
                        "Loading details for series: {} (ID: {})",
                        series_ref.title.as_str(),
                        series_ref.id.as_str()
                    );

                    // Extract full details from SeriesReference
                    let (
                        description,
                        poster_url,
                        backdrop_url,
                        tmdb_id,
                        genres,
                        rating,
                        total_episodes,
                    ) = match &series_ref.details {
                        crate::api_types::MediaDetailsOption::Details(
                            crate::api_types::TmdbDetails::Series(series_details),
                        ) => {
                            log::info!(
                                "Series {} has overview: {:?}",
                                series_ref.title.as_str(),
                                series_details
                                    .overview
                                    .as_ref()
                                    .map(|o| crate::views::macros::truncate_text(o, 50))
                            );
                            (
                                series_details.overview.clone(),
                                series_details.poster_path.clone(),
                                series_details.backdrop_path.clone(),
                                Some(series_details.id),
                                series_details.genres.clone(),
                                series_details.vote_average.map(|v| v as f32),
                                series_details.number_of_episodes,
                            )
                        }
                        _ => {
                            log::warn!("Series {} has no TMDB details", series_ref.title.as_str());
                            (
                                None,
                                None,
                                None,
                                Some(series_ref.tmdb_id),
                                vec![],
                                None,
                                None,
                            )
                        }
                    };

                    // Convert season references to SeasonSummary format
                    let seasons = if let Some(season_refs) = series_seasons {
                        season_refs
                            .iter()
                            .map(|season| {
                                // Extract season details
                                let (name, poster_url) = match &season.details {
                                    crate::api_types::MediaDetailsOption::Details(
                                        crate::api_types::TmdbDetails::Season(details),
                                    ) => (
                                        if details.name.is_empty() {
                                            None
                                        } else {
                                            Some(details.name.clone())
                                        },
                                        details.poster_path.clone(),
                                    ),
                                    _ => (None, None),
                                };

                                // Get episode count from episode_references
                                let episode_count = episode_refs
                                    .get(season.id.as_str())
                                    .map(|episodes| episodes.len())
                                    .unwrap_or(0);

                                crate::models::SeasonSummary {
                                    number: season.season_number.value() as u32,
                                    episode_count,
                                    poster_url,
                                    name,
                                }
                            })
                            .collect()
                    } else {
                        vec![]
                    };

                    // Convert SeriesReference to TvShowDetails for backward compatibility
                    let details = crate::models::TvShowDetails {
                        name: series_ref.title.as_str().to_string(),
                        tmdb_id,
                        poster_url,
                        backdrop_url,
                        description,
                        seasons,
                        genres,
                        rating,
                        total_episodes,
                    };
                    Message::TvShowLoaded(series_id_str.clone(), Ok(details))
                }
                Ok(_) => Message::TvShowLoaded(
                    series_id_str.clone(),
                    Err("Unexpected media type returned".to_string()),
                ),
                Err(e) => Message::TvShowLoaded(series_id_str.clone(), Err(e.to_string())),
            },
        )
    } else {
        // No library selected
        Task::none()
    };

    // Batch both tasks together
    Task::batch([fetch_task, existing_task])
}

pub fn handle_view_season(
    state: &mut State,
    series_id: SeriesID,
    season_id: SeasonID,
) -> Task<Message> {
    log::info!("Viewing season {:?} of series {:?}", season_id, series_id);

    // Clear previous season details
    state.current_season_details = None;

    // Check if we need to fetch season details on-demand
    let fetch_task = if let Some(library_id) = state.current_library_id {
        let season_media_id = ferrex_core::api_types::MediaId::Season(season_id.clone());
        state.fetch_media_details_on_demand(library_id, season_media_id)
    } else {
        Task::none()
    };

    // NEW ARCHITECTURE: Get episodes from MediaStore
    if let Ok(store) = state.media_store.read() {
        let episodes = store.get_episodes(season_id.as_str());
        if !episodes.is_empty() {
            // Store episodes in state for UI rendering
            state.current_season_episodes = episodes.into_iter().cloned().collect();

            state.season_episodes_carousel = Some(CarouselState::new_with_dimensions(
                state.current_season_episodes.len(),
                250.0, // Episode thumbnail width
                15.0,  // Spacing
            ));
            if let Some(carousel) = &mut state.season_episodes_carousel {
                let available_width = state.window_size.width - 80.0;
                carousel.update_items_per_page(available_width);
            }
        }
    }

    // Save current scroll position if navigating from library view
    if matches!(state.view, ViewState::Library) {
        state.save_scroll_position();
    }

    state.view = ViewState::SeasonDetail {
        series_id: series_id.clone(),
        season_id: season_id.clone(),
        backdrop_handle: None,
    };

    // Update depth regions for season detail view
    state.background_shader_state.update_depth_lines(
        &state.view,
        state.window_size.width,
        state.window_size.height,
    );

    // Return the fetch task converted to ui::Message
    fetch_task.map(|_| Message::NoOp)
}

pub fn handle_view_episode(state: &mut State, episode_id: EpisodeID) -> Task<Message> {
    log::info!("Viewing episode: {}", episode_id.as_str());

    // Save current scroll position if navigating from library view
    if matches!(state.view, ViewState::Library) {
        state.save_scroll_position();
    }

    // Check if we need to fetch episode details on-demand
    let fetch_task = if let Some(library_id) = state.current_library_id {
        let episode_media_id = ferrex_core::api_types::MediaId::Episode(episode_id.clone());
        state.fetch_media_details_on_demand(library_id, episode_media_id)
    } else {
        Task::none()
    };

    state.view = ViewState::EpisodeDetail {
        episode_id: episode_id,
        backdrop_handle: None,
    };

    // Update depth regions for episode detail view
    state.background_shader_state.update_depth_lines(
        &state.view,
        state.window_size.width,
        state.window_size.height,
    );

    // Convert DomainMessage task to ui::Message task
    fetch_task.map(|_| Message::NoOp)
}

pub fn handle_navigate_home(state: &mut State) -> Task<Message> {
    state.view = ViewState::Library;
    state.view_mode = ViewMode::All;
    state.current_library_id = None;

    // Clear detail view data
    state.current_show_seasons.clear();
    state.current_season_episodes.clear();

    // Refresh media to show all libraries - emit cross-domain event instead
    Task::perform(async {}, |_| {
        Message::_EmitCrossDomainEvent(crate::messages::CrossDomainEvent::RequestLibraryRefresh)
    })
}

pub fn handle_exit_fullscreen(state: &mut State) -> Task<Message> {
    // Only exit fullscreen if we're actually in fullscreen
    if state.player.is_fullscreen {
        state.player.is_fullscreen = false;
        let mode = iced::window::Mode::Windowed;
        iced::window::get_latest().and_then(move |id| iced::window::set_mode(id, mode))
    } else {
        Task::none()
    }
}

pub fn handle_toggle_backdrop_aspect_mode(state: &mut State) -> Task<Message> {
    // Toggle between Auto and Force21x9 modes
    state.background_shader_state.backdrop_aspect_mode =
        match state.background_shader_state.backdrop_aspect_mode {
            crate::state::BackdropAspectMode::Auto => crate::state::BackdropAspectMode::Force21x9,
            crate::state::BackdropAspectMode::Force21x9 => crate::state::BackdropAspectMode::Auto,
        };
    log::info!(
        "Toggled backdrop aspect mode to: {:?}",
        state.background_shader_state.backdrop_aspect_mode
    );

    Task::none()
}
