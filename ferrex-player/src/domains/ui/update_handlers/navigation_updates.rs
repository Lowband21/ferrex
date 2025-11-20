use iced::Task;
use rkyv::option::ArchivedOption;
use uuid::Uuid;

use super::super::views::carousel::CarouselState;
use crate::{
    domains::{
        metadata::image_types::ImageRequest,
        ui::{ViewState, messages::Message, types, views::grid::macros},
    },
    infrastructure::api_types::{Media, MovieReference},
    state_refactored::State,
};
use ferrex_core::{
    EpisodeID, ImageSize, ImageType, MediaFile, MediaID, MediaIDLike, MovieID, MovieLike, SeasonID,
    SeriesID, SeriesLike,
};

/// Updates background shader depth regions when transitioning to a detail view
/// This ensures smooth animation from current regions to new regions
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
fn prepare_depth_regions_for_transition(state: &mut State, new_view: &ViewState) {
    // Update depth regions for the new view BEFORE changing view state
    // This triggers the fade animation between different depth layouts

    // TODO: This is cumbersome, fix it
    let uuid = if let Some(library_id) = state.domains.library.state.current_library_id {
        Some(library_id.as_uuid())
    } else {
        None
    };
    state
        .domains
        .ui
        .state
        .background_shader_state
        .update_depth_lines(
            new_view,
            state.window_size.width,
            state.window_size.height,
            uuid,
        );
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_view_details(state: &mut State, media: MediaID) -> Task<Message> {
    // Save current view to navigation history
    state
        .domains
        .ui
        .state
        .navigation_history
        .push(state.domains.ui.state.view.clone());

    // Save current scroll position before navigating away
    save_current_scroll_state(state);

    /* TODO: Get media for details views
    // Determine if it's a movie or TV episode
    if media.is_tv_episode() {
        state.domains.ui.state.view = ViewState::EpisodeDetail {
            episode_id: EpisodeID::new(media.id.clone())
                .unwrap_or_else(|_| EpisodeID::new("unknown".to_string()).unwrap()),
            backdrop_handle: None,
        };
    } else {
        // NEW ARCHITECTURE: Find movie in MediaStore
        let movie_id = ferrex_core::MovieID::new(media.id.clone())
            .unwrap_or_else(|_| ferrex_core::MovieID::new("unknown".to_string()).unwrap());
        let media_id = ferrex_core::MediaID::Movie(movie_id);

        if let Ok(store) = state.domains.media.state.media_store.read() {
            // TODO: Media state reference outside of media domain
            if let Some(Media::Movie(movie)) = store.get(&media_id) {
                state.domains.ui.state.view = ViewState::MovieDetail {
                    movie: movie.clone(),
                    backdrop_handle: None,
                };
            } else {
                log::error!("Movie not found in MediaStore: {}", media.id);
                state.domains.ui.state.error_message =
                    Some(format!("Movie not found: {}", media.display_title()));
            }
        }
    } */

    // Update depth regions for the new detail view
    // TODO: Please don't push this
    state
        .domains
        .ui
        .state
        .background_shader_state
        .update_depth_lines(
            &state.domains.ui.state.view,
            state.window_size.width,
            state.window_size.height,
            None,
        );

    Task::none()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_view_movie_details(state: &mut State, movie_id: MovieID) -> Task<Message> {
    let mut buff = Uuid::encode_buffer();
    log::info!(
        "Viewing movie details for id: {})",
        movie_id.as_str(&mut buff)
    );

    // Save current view to navigation history
    state
        .domains
        .ui
        .state
        .navigation_history
        .push(state.domains.ui.state.view.clone());

    // Ensure yoke is in the UI cache for detail view borrowing
    let movie_uuid = movie_id.to_uuid();
    if let Ok(yoke) = state
        .domains
        .ui
        .state
        .repo_accessor
        .get_movie_yoke(&MediaID::Movie(movie_id))
    {
        let movie = *yoke.get();

        // Save current scroll position before navigating away
        save_current_scroll_state(state);

        let new_view = ViewState::MovieDetail {
            movie_id: movie_id,
            backdrop_handle: None,
        };

        // FIRST: Set up depth regions for the transition (this enables the fade animation)
        prepare_depth_regions_for_transition(state, &new_view);

        // THEN: Transition to new theme colors
        if let ArchivedOption::Some(hex) = &movie.theme_color {
            if let Ok(color) = macros::parse_hex_color(hex) {
                let r = color.r * 0.2;
                let g = color.g * 0.2;
                let b = color.b * 0.2;
                let primary_dark = iced::Color::from_rgb(r, g, b);

                // Secondary color is much lighter for stronger gradient
                let secondary = iced::Color::from_rgb(
                    (color.r * 0.8).min(1.0), // 4x primary
                    (color.g * 0.8).min(1.0),
                    (color.b * 0.8).min(1.0),
                );

                // Start color transition
                state
                    .domains
                    .ui
                    .state
                    .background_shader_state
                    .color_transitions
                    .transition_to(primary_dark, secondary);
            }
        }

        // Non-functional
        //let new_center = crate::domains::ui::transitions::generate_random_gradient_center();
        //state
        //    .domains
        //    .ui
        //    .state
        //    .background_shader_state
        //    .gradient_transitions
        //    .transition_to(new_center);

        // Queue request if not in cache
        if let Some(movie_details) = movie.details() {
            if movie_details.backdrop_path.is_some() {
                let request =
                    ImageRequest::new(movie.id.to_uuid(), ImageSize::Backdrop, ImageType::Movie);
                if state.image_service.get(&request).is_none() {
                    state.image_service.request_image(request);
                }
            } else {
                log::warn!("Cannot find path for movie backdrop");
            }
        } else {
            log::warn!("Movie {} has no details", movie.title());
        }

        //// Start backdrop transition animation (Broken)
        //state
        //    .domains
        //    .ui
        //    .state
        //    .background_shader_state
        //    .backdrop_transitions
        //    .start_fade_slide(50.0); // 50px slide

        // Finally change the view state
        state.domains.ui.state.view = new_view;

        state
            .domains
            .ui
            .state
            .movie_yoke_cache
            .insert(movie_uuid, std::sync::Arc::new(yoke));
    }
    Task::none()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_view_series(state: &mut State, series_id: SeriesID) -> Task<Message> {
    log::info!("Viewing series: {:?}", series_id);

    // Save current view to navigation history
    state
        .domains
        .ui
        .state
        .navigation_history
        .push(state.domains.ui.state.view.clone());

    // Ensure yoke is in the UI cache for detail view borrowing
    let series_uuid = series_id.to_uuid();
    if let Ok(yoke) = state
        .domains
        .ui
        .state
        .repo_accessor
        .get_series_yoke(&MediaID::Series(series_id))
    {
        let series = *yoke.get();

        // Save current scroll position before navigating away
        save_current_scroll_state(state);

        let new_view = ViewState::SeriesDetail {
            series_id,
            backdrop_handle: None,
        };

        // FIRST: Set up depth regions for the transition (this enables the fade animation)
        prepare_depth_regions_for_transition(state, &new_view);

        // THEN: Transition to new theme colors
        if let ArchivedOption::Some(hex) = &series.theme_color {
            if let Ok(color) = macros::parse_hex_color(hex) {
                let r = color.r * 0.2;
                let g = color.g * 0.2;
                let b = color.b * 0.2;
                let primary_dark = iced::Color::from_rgb(r, g, b);

                // Secondary color is much lighter for stronger gradient
                let secondary = iced::Color::from_rgb(
                    (color.r * 0.8).min(1.0), // 4x primary
                    (color.g * 0.8).min(1.0),
                    (color.b * 0.8).min(1.0),
                );

                // Start color transition
                state
                    .domains
                    .ui
                    .state
                    .background_shader_state
                    .color_transitions
                    .transition_to(primary_dark, secondary);
            }
        }

        // Queue request if not in cache
        if let Some(details) = series.details() {
            if details.backdrop_path.is_some() {
                let request =
                    ImageRequest::new(series.id.to_uuid(), ImageSize::Backdrop, ImageType::Series);
                if state.image_service.get(&request).is_none() {
                    state.image_service.request_image(request);
                }
            } else {
                log::warn!("Cannot find path for series backdrop");
            }
        } else {
            log::warn!("Series {} has no details", series.title());
        }
        // Finally change the view state
        state.domains.ui.state.view = new_view;

        state
            .domains
            .ui
            .state
            .series_yoke_cache
            .insert(series_uuid, std::sync::Arc::new(yoke));
    }
    Task::none()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_view_season(
    state: &mut State,
    series_id: SeriesID,
    season_id: SeasonID,
) -> Task<Message> {
    log::info!("Viewing season {:?} of series {:?}", season_id, series_id);

    // Save current view to navigation history
    state
        .domains
        .ui
        .state
        .navigation_history
        .push(state.domains.ui.state.view.clone());

    // Save current scroll position before navigating away
    save_current_scroll_state(state);

    // Clear previous season details
    state.domains.media.state.current_season_details = None;

    // Save current scroll position if navigating from library view
    if matches!(state.domains.ui.state.view, ViewState::Library) {}

    // Create the new view state
    let new_view = ViewState::SeasonDetail {
        series_id: series_id.clone(),
        season_id: season_id.clone(),
        backdrop_handle: None,
    };

    // Update depth regions for season detail view (uses same regions as movie/series for now)
    // TODO: Add season-specific depth regions in the future
    prepare_depth_regions_for_transition(state, &new_view);

    // Change the view state
    state.domains.ui.state.view = new_view;

    // Return the fetch task converted to ui::Message
    //fetch_task.map(|_| Message::NoOp)
    Task::none()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_view_episode(state: &mut State, episode_id: EpisodeID) -> Task<Message> {
    let mut buff = Uuid::encode_buffer();
    log::info!("Viewing episode: {}", episode_id.as_str(&mut buff));

    // Save current view to navigation history
    state
        .domains
        .ui
        .state
        .navigation_history
        .push(state.domains.ui.state.view.clone());

    // Save current scroll position before navigating away
    save_current_scroll_state(state);

    // Create the new view state
    let new_view = ViewState::EpisodeDetail {
        episode_id: episode_id,
        backdrop_handle: None,
    };

    // Update depth regions for episode detail view (uses same regions as movie/series for now)
    // TODO: Add episode-specific depth regions in the future
    prepare_depth_regions_for_transition(state, &new_view);

    // Change the view state
    state.domains.ui.state.view = new_view;

    // Convert DomainMessage task to ui::Message task
    //fetch_task.map(|_| Message::NoOp)
    Task::none()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_navigate_home(state: &mut State) -> Task<Message> {
    state.domains.ui.state.view = ViewState::Library;

    state.domains.library.state.current_library_id = None;

    // Clear detail view data
    // REMOVED: No longer clearing duplicate state fields
    // MediaStore is the single source of truth

    // Refresh media to show all libraries
    Task::done(Message::AggregateAllLibraries)
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_exit_fullscreen(state: &mut State) -> Task<Message> {
    // Only exit fullscreen if we're actually in fullscreen
    if state.domains.player.state.is_fullscreen {
        state.domains.player.state.is_fullscreen = false;
        let mode = iced::window::Mode::Windowed;
        iced::window::get_latest().and_then(move |id| iced::window::set_mode(id, mode))
    } else {
        Task::none()
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
pub fn handle_toggle_backdrop_aspect_mode(state: &mut State) -> Task<Message> {
    // Toggle between Auto and Force21x9 modes
    state
        .domains
        .ui
        .state
        .background_shader_state
        .backdrop_aspect_mode = match state
        .domains
        .ui
        .state
        .background_shader_state
        .backdrop_aspect_mode
    {
        types::BackdropAspectMode::Auto => types::BackdropAspectMode::Force21x9,
        types::BackdropAspectMode::Force21x9 => types::BackdropAspectMode::Auto,
    };
    log::info!(
        "Toggled backdrop aspect mode to: {:?}",
        state
            .domains
            .ui
            .state
            .background_shader_state
            .backdrop_aspect_mode
    );

    Task::none()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
fn save_current_scroll_state(state: &mut State) {
    let current_view = state.domains.ui.state.view.clone();
    let library_id = state.domains.library.state.current_library_id;

    match current_view {
        ViewState::Library => {
            // Scroll state management for libraries is handled by tabs, it should be migrated to the unified tab mangager

            log::debug!(
                "Saved independent scroll states for movies and TV ViewModels (library_id: {:?})",
                library_id
            );
        }
        _ => {
            // We need to save scroll state for detail views, settings, etc.
            log::debug!("No scroll state to save for view: {:?}", current_view);
        }
    }
}
