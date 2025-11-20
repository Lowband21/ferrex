use iced::Task;
use rkyv::option::ArchivedOption;
use uuid::Uuid;

use super::super::views::carousel::CarouselState;
use crate::{
    domains::ui::{ViewState, messages::Message, types, views::grid::macros},
    state_refactored::State,
};
use ferrex_core::{
    traits::{
        id::MediaIDLike,
        media_ops::MediaOps,
        sub_like::{MovieLike, SeasonLike, SeriesLike},
    },
    types::{
        ids::{EpisodeID, MovieID, SeasonID, SeriesID},
        image_request::{
            BackdropKind, BackdropSize, ImageRequest, PosterKind, PosterSize,
            Priority, ProfileSize,
        },
        media_id::MediaID,
    },
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
fn prepare_depth_regions_for_transition(
    state: &mut State,
    new_view: &ViewState,
) {
    // Update depth regions for the new view BEFORE changing view state
    // This triggers the fade animation between different depth layouts

    // TODO: This is cumbersome, fix it
    let uuid = state
        .domains
        .library
        .state
        .current_library_id
        .map(|library_id| library_id.as_uuid());
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
        let movie_id = MovieID::new(media.id.clone())
            .unwrap_or_else(|_| MovieID::new("unknown".to_string()).unwrap());
        let media_id = MediaID::Movie(movie_id);

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
pub fn handle_view_movie_details(
    state: &mut State,
    movie_id: MovieID,
) -> Task<Message> {
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
            movie_id,
            backdrop_handle: None,
        };

        // FIRST: Set up depth regions for the transition (this enables the fade animation)
        prepare_depth_regions_for_transition(state, &new_view);

        // THEN: Transition to new theme colors
        if let ArchivedOption::Some(hex) = &movie.theme_color
            && let Ok(color) = macros::parse_hex_color(hex)
        {
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

        // Non-functional
        //let new_center = crate::domains::ui::transitions::generate_random_gradient_center();
        //state
        //    .domains
        //    .ui
        //    .state
        //    .background_shader_state
        //    .gradient_transitions
        //    .transition_to(new_center);

        // Queue image requests if not in cache
        if let Some(movie_details) = movie.details() {
            if movie_details.backdrop_path.is_some() {
                let request = ImageRequest::backdrop(
                    movie.id.to_uuid(),
                    BackdropKind::Movie,
                    BackdropSize::Quality,
                );
                if state.image_service.get(&request).is_none() {
                    state.image_service.request_image(request);
                }
            } else {
                log::warn!("Cannot find path for movie backdrop");
            }

            // Ensure the hero poster is ready when the detail view renders
            let poster_request = ImageRequest::poster(
                movie.id.to_uuid(),
                PosterKind::Movie,
                PosterSize::Original,
            )
            .with_priority(Priority::Visible);
            if state.image_service.get(&poster_request).is_none() {
                state.image_service.request_image(poster_request);
            }

            // Preload primary cast portraits for the carousel
            for cast_member in movie_details.cast.iter().take(12) {
                let person_uuid = match &cast_member.profile_media_id {
                    ArchivedOption::Some(uuid) => *uuid,
                    ArchivedOption::None => continue,
                };

                let image_index = match &cast_member.profile_image_index {
                    ArchivedOption::Some(index) => index.to_native(),
                    ArchivedOption::None => 0,
                };
                let cast_request = ImageRequest::person_profile(
                    person_uuid,
                    ProfileSize::Standard,
                )
                .with_priority(Priority::Preload)
                .with_index(image_index);

                if state.image_service.get(&cast_request).is_none() {
                    state.image_service.request_image(cast_request);
                }
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
pub fn handle_view_series(
    state: &mut State,
    series_id: SeriesID,
) -> Task<Message> {
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
        if let ArchivedOption::Some(hex) = &series.theme_color
            && let Ok(color) = macros::parse_hex_color(hex)
        {
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

        // Queue request if not in cache
        if let Some(details) = series.details() {
            if details.backdrop_path.is_some() {
                let request = ImageRequest::backdrop(
                    series.id.to_uuid(),
                    BackdropKind::Series,
                    BackdropSize::Quality,
                );
                if state.image_service.get(&request).is_none() {
                    state.image_service.request_image(request);
                }
            } else {
                log::warn!("Cannot find path for series backdrop");
            }

            // Preload the primary series poster
            let poster_request = ImageRequest::poster(
                series.id.to_uuid(),
                PosterKind::Series,
                PosterSize::Original,
            )
            .with_priority(Priority::Visible);
            if state.image_service.get(&poster_request).is_none() {
                state.image_service.request_image(poster_request);
            }

            // Prefetch lead cast portraits for the detail view carousel
            for cast_member in details.cast.iter().take(12) {
                let person_uuid = match &cast_member.profile_media_id {
                    ArchivedOption::Some(uuid) => *uuid,
                    ArchivedOption::None => continue,
                };

                let image_index = match &cast_member.profile_image_index {
                    ArchivedOption::Some(index) => index.to_native(),
                    ArchivedOption::None => 0,
                };
                let cast_request = ImageRequest::person_profile(
                    person_uuid,
                    ProfileSize::Standard,
                )
                .with_priority(Priority::Preload)
                .with_index(image_index);

                if state.image_service.get(&cast_request).is_none() {
                    state.image_service.request_image(cast_request);
                }
            }
        } else {
            log::warn!("Series {} has no details", series.title());
        }
        // Finally change the view state
        state.domains.ui.state.view = new_view;

        let total_seasons = state
            .domains
            .ui
            .state
            .repo_accessor
            .get_series_seasons(&series_id)
            .map(|v| v.len())
            .unwrap_or(0);
        let mut cs = CarouselState::new(total_seasons);
        cs.update_items_per_page(state.window_size.width);
        state.domains.ui.state.show_seasons_carousel = Some(cs);

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

    let season_uuid = season_id.to_uuid();
    if let Ok(yoke) = state
        .domains
        .ui
        .state
        .repo_accessor
        .get_season_yoke(&MediaID::Season(season_id))
    {
        let season = *yoke.get();

        let new_view = ViewState::SeasonDetail {
            series_id,
            season_id: season.id(),
            backdrop_handle: None,
        };

        prepare_depth_regions_for_transition(state, &new_view);

        if let Some(hex) = season.theme_color()
            && let Ok(color) = macros::parse_hex_color(hex)
        {
            let r = color.r * 0.2;
            let g = color.g * 0.2;
            let b = color.b * 0.2;
            let primary_dark = iced::Color::from_rgb(r, g, b);
            let secondary = iced::Color::from_rgb(
                (color.r * 0.8).min(1.0),
                (color.g * 0.8).min(1.0),
                (color.b * 0.8).min(1.0),
            );
            state
                .domains
                .ui
                .state
                .background_shader_state
                .color_transitions
                .transition_to(primary_dark, secondary);
        }

        // Queue season backdrop request if details include one

        if let Some(details) = season.details()
            && (details.poster_path.is_some() || !details.name.is_empty())
        {
            let request = ImageRequest::backdrop(
                season.id().to_uuid(),
                BackdropKind::Season,
                BackdropSize::Quality,
            );
            if state.image_service.get(&request).is_none() {
                state.image_service.request_image(request);
            }
        }

        state.domains.ui.state.view = new_view;

        // Initialize episodes carousel for this season
        let total_eps = state
            .domains
            .ui
            .state
            .repo_accessor
            .get_season_episodes(&season.id())
            .map(|v| v.len())
            .unwrap_or(0);
        // Episodes are typically wide (16:9); use a wider item width
        let mut ep_cs = crate::domains::ui::views::carousel::CarouselState::new_with_dimensions(
            total_eps, 400.0, 15.0,
        );
        ep_cs.update_items_per_page(state.window_size.width);
        state.domains.ui.state.season_episodes_carousel = Some(ep_cs);

        state
            .domains
            .ui
            .state
            .season_yoke_cache
            .insert(season_uuid, std::sync::Arc::new(yoke));
    } else {
        let new_view = ViewState::SeasonDetail {
            series_id,
            season_id,
            backdrop_handle: None,
        };
        prepare_depth_regions_for_transition(state, &new_view);
        state.domains.ui.state.view = new_view;
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
pub fn handle_view_episode(
    state: &mut State,
    episode_id: EpisodeID,
) -> Task<Message> {
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

    let episode_uuid = episode_id.to_uuid();
    if let Ok(yoke) = state
        .domains
        .ui
        .state
        .repo_accessor
        .get_episode_yoke(&MediaID::Episode(episode_id))
    {
        let new_view = ViewState::EpisodeDetail {
            episode_id: yoke.get().id(),
            backdrop_handle: None,
        };

        prepare_depth_regions_for_transition(state, &new_view);

        state.domains.ui.state.view = new_view;
        state
            .domains
            .ui
            .state
            .episode_yoke_cache
            .insert(episode_uuid, std::sync::Arc::new(yoke));
    } else {
        let new_view = ViewState::EpisodeDetail {
            episode_id,
            backdrop_handle: None,
        };
        prepare_depth_regions_for_transition(state, &new_view);
        state.domains.ui.state.view = new_view;
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
pub fn handle_navigate_home(state: &mut State) -> Task<Message> {
    state.domains.ui.state.view = ViewState::Library;

    state.domains.library.state.current_library_id = None;

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
        iced::window::latest()
            .and_then(move |id| iced::window::set_mode(id, mode))
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
