use ferrex_core::player_prelude::{
    EpisodeSize, ImageRequest, ImageSize, Media, Priority, ProfileSize,
    TheaterPlateViewport, theater_plate_backdrop_size_for_viewport,
};
use iced::Task;
use rkyv::option::ArchivedOption;

use super::super::views::carousel::CarouselState;
use crate::domains::ui::shell_ui::Scope;
use crate::domains::ui::views::{
    detail::{
        DetailRailCardVariant, DetailRailMetrics,
        solve_detail_layout_from_runtime,
    },
    virtual_carousel::{
        CarouselConfig, CarouselKey, DetailCarouselOwnerKind, planner,
    },
};
use crate::{
    domains::ui::{
        ViewState, messages::UiMessage, shell_ui::UiShellMessage, types,
        views::grid::macros,
    },
    state::State,
};
use ferrex_core::{
    traits::{id::MediaIDLike, media_ops::MediaOps},
    types::{
        ids::{EpisodeID, MovieID, SeasonID, SeriesID},
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
fn detail_backdrop_request_size(state: &State) -> ImageSize {
    theater_plate_backdrop_size_for_viewport(
        TheaterPlateViewport::from_logical_size(
            state.window_size.width,
            state.window_size.height,
        ),
    )
}

fn detail_rail_metrics_for_state(
    state: &State,
    variant: DetailRailCardVariant,
) -> (DetailRailMetrics, f32) {
    let plan = solve_detail_layout_from_runtime(
        state.window_size.width,
        state.window_size.height,
        state.domains.ui.state.view.header_height().unwrap_or(0.0),
        state.interface_mode,
        &state.domains.ui.state.size_provider,
        &state.domains.ui.state.scaled_layout,
    );
    let metrics = plan.rail.metrics_for(variant);
    let stage_width = plan.foreground_stage(0.0).stage.stage_width;
    let viewport_width = stage_width
        .min(plan.viewport_width)
        .max(metrics.card_width)
        .max(1.0);
    (metrics, viewport_width)
}

fn ensure_detail_rail_carousel(
    state: &mut State,
    key: CarouselKey,
    total_items: usize,
    variant: DetailRailCardVariant,
) {
    let (metrics, viewport_width) =
        detail_rail_metrics_for_state(state, variant);
    let config = CarouselConfig::detail_rail(metrics.card_width, metrics.gap);
    let scale = state.domains.ui.state.scaled_layout.scale;
    state.domains.ui.state.carousel_registry.ensure_default(
        key,
        total_items,
        viewport_width,
        config,
        scale,
    );
}

fn emit_detail_rail_snapshot_for_visible<F>(
    state: &State,
    key: &CarouselKey,
    total_items: usize,
    ids_fn: F,
    image_kind: planner::CarouselDemandImageKind,
) where
    F: Fn(usize) -> Option<uuid::Uuid>,
{
    if let Some(handle) = state.domains.metadata.state.planner_handle.as_ref()
        && let Some(vc) = state.domains.ui.state.carousel_registry.get(key)
    {
        handle.send(planner::snapshot_for_visible_with_image_kind(
            vc,
            total_items,
            ids_fn,
            image_kind,
            state.domains.settings.display.detail_poster_quality,
            &state.runtime_config,
        ));
    }
}

fn prepare_depth_regions_for_transition(
    state: &mut State,
    new_view: &ViewState,
) {
    // Update depth regions for the new view BEFORE changing view state
    // This triggers the fade animation between different depth layouts

    // TODO: This is cumbersome, fix it
    let uuid = state
        .domains
        .ui
        .state
        .scope
        .lib_id()
        .map(|library_id| library_id.to_uuid());

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
pub fn handle_view_movie_details(
    state: &mut State,
    movie_id: MovieID,
) -> Task<UiMessage> {
    log::info!("Viewing movie details for id: {})", movie_id.as_str());

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
        if let ArchivedOption::Some(iid) = &movie.details.primary_backdrop_iid {
            let request =
                ImageRequest::new(*iid, detail_backdrop_request_size(state))
                    .with_priority(Priority::Visible);
            if state.image_service.get(&request).is_none() {
                state.image_service.request_image(request);
            }
        } else {
            log::warn!("Movie missing primary_backdrop_iid");
        }

        // Ensure the hero poster is ready when the detail view renders
        let detail_quality =
            state.domains.settings.display.detail_poster_quality;
        if let ArchivedOption::Some(iid) = &movie.details.primary_poster_iid {
            let poster_request =
                ImageRequest::new(*iid, ImageSize::Poster(detail_quality))
                    .with_priority(Priority::Visible);
            if state.image_service.get(&poster_request).is_none() {
                state.image_service.request_image(poster_request);
            }
        } else {
            log::warn!("Movie missing primary_poster_iid");
        }

        // Register movie cast as a detail rail so initial profile demand uses
        // the same viewport-aware windowing as scroll updates.
        let cast_total = movie.details.cast.len();
        let cast_profile_image_ids: Vec<_> = movie
            .details
            .cast
            .iter()
            .filter_map(|c| match &c.image_id {
                ArchivedOption::Some(iid) => Some(*iid),
                ArchivedOption::None => None,
            })
            .collect();
        let cast_key = CarouselKey::DetailCast {
            owner_kind: DetailCarouselOwnerKind::Movie,
            owner_id: movie_uuid,
        };
        ensure_detail_rail_carousel(
            state,
            cast_key.clone(),
            cast_total,
            DetailRailCardVariant::Profile,
        );
        emit_detail_rail_snapshot_for_visible(
            state,
            &cast_key,
            cast_total,
            |i| cast_profile_image_ids.get(i).copied(),
            planner::CarouselDemandImageKind::Profile {
                size: ProfileSize::W185,
            },
        );

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
) -> Task<UiMessage> {
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
        if let ArchivedOption::Some(iid) = &series.details.primary_backdrop_iid
        {
            let request =
                ImageRequest::new(*iid, detail_backdrop_request_size(state))
                    .with_priority(Priority::Visible);
            if state.image_service.get(&request).is_none() {
                state.image_service.request_image(request);
            }
        } else {
            log::warn!("Series missing primary_backdrop_iid");
        }

        // Preload the primary series poster
        let detail_quality =
            state.domains.settings.display.detail_poster_quality;
        if let ArchivedOption::Some(iid) = &series.details.primary_poster_iid {
            let poster_request =
                ImageRequest::new(*iid, ImageSize::Poster(detail_quality))
                    .with_priority(Priority::Visible);
            if state.image_service.get(&poster_request).is_none() {
                state.image_service.request_image(poster_request);
            }
        } else {
            log::warn!("Series missing primary_poster_iid");
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

        let seasons_key = CarouselKey::ShowSeasons(series_id.to_uuid());
        ensure_detail_rail_carousel(
            state,
            seasons_key.clone(),
            total_seasons,
            DetailRailCardVariant::Poster,
        );

        // After constructing the carousel, emit a viewport-aware demand snapshot.
        if let Ok(seasons) = state
            .domains
            .ui
            .state
            .repo_accessor
            .get_series_seasons(&series_id)
        {
            emit_detail_rail_snapshot_for_visible(
                state,
                &seasons_key,
                seasons.len(),
                |i| seasons.get(i).and_then(|s| s.details.primary_poster_iid),
                planner::CarouselDemandImageKind::Poster {
                    size: state.domains.settings.display.detail_poster_quality,
                },
            );
        }

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
) -> Task<UiMessage> {
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

        // Queue backdrop request for the containing series (season itself has no backdrop)
        if let Ok(media) = state
            .domains
            .ui
            .state
            .repo_accessor
            .get(&MediaID::Series(series_id))
            && let Media::Series(sr) = media
            && let Some(iid) = sr.details.primary_backdrop_iid
        {
            let request =
                ImageRequest::new(iid, detail_backdrop_request_size(state))
                    .with_priority(Priority::Visible);
            if state.image_service.get(&request).is_none() {
                state.image_service.request_image(request);
            }
        }

        // Ensure the season poster is ready for the detail header
        if let ArchivedOption::Some(iid) = &season.details.primary_poster_iid {
            let detail_quality =
                state.domains.settings.display.detail_poster_quality;
            let request =
                ImageRequest::new(*iid, ImageSize::Poster(detail_quality))
                    .with_priority(Priority::Visible);
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

        let episodes_key = CarouselKey::SeasonEpisodes(season.id().to_uuid());
        ensure_detail_rail_carousel(
            state,
            episodes_key.clone(),
            total_eps,
            DetailRailCardVariant::StillWide,
        );

        // After constructing the episodes carousel, emit a viewport-aware demand snapshot.
        let episodes = state
            .domains
            .ui
            .state
            .repo_accessor
            .get_season_episodes(&season.id())
            .unwrap_or_else(|_| Vec::new());
        emit_detail_rail_snapshot_for_visible(
            state,
            &episodes_key,
            episodes.len(),
            |i| episodes.get(i).and_then(|e| e.details.primary_still_iid),
            planner::CarouselDemandImageKind::EpisodeStill {
                size: EpisodeSize::W512,
            },
        );

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
) -> Task<UiMessage> {
    log::info!("Viewing episode: {}", episode_id.as_str());

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
        let episode = yoke.get();
        let new_view = ViewState::EpisodeDetail {
            episode_id: episode.id(),
            backdrop_handle: None,
        };

        prepare_depth_regions_for_transition(state, &new_view);

        if let Ok(media) = state
            .domains
            .ui
            .state
            .repo_accessor
            .get(&MediaID::Season(SeasonID(episode.season_id.0)))
            && let Media::Season(season) = media
            && let Some(hex) = season.theme_color()
            && let Ok(color) = macros::parse_hex_color(hex)
        {
            let primary_dark = iced::Color::from_rgb(
                color.r * 0.2,
                color.g * 0.2,
                color.b * 0.2,
            );
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

        if let ArchivedOption::Some(iid) = &episode.details.primary_still_iid {
            let request = ImageRequest::new(
                *iid,
                ImageSize::Thumbnail(EpisodeSize::W512),
            )
            .with_priority(Priority::Visible);
            if state.image_service.get(&request).is_none() {
                state.image_service.request_image(request);
            }
        }

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
pub fn handle_navigate_home(state: &mut State) -> Task<UiMessage> {
    state.domains.ui.state.view = ViewState::Library;

    Task::done(UiShellMessage::SelectScope(Scope::Home).into())
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_exit_fullscreen(state: &mut State) -> Task<UiMessage> {
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
pub fn handle_toggle_backdrop_aspect_mode(
    state: &mut State,
) -> Task<UiMessage> {
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
    let library_id = state.domains.ui.state.scope.lib_id();

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
