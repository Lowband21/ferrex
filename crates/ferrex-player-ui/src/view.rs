//! Root-level view composition

use crate::common::messages::DomainMessage;
use crate::domains::ui::interaction_ui::InteractionMessage;
use crate::domains::ui::theme;
use crate::domains::ui::types::ViewState;
use crate::domains::ui::views::admin::{
    view_admin_dashboard, view_admin_users, view_library_management,
};
use crate::domains::ui::views::auth::view_auth;
#[cfg(test)]
use crate::domains::ui::views::detail::DetailTheaterPlateRect;
use crate::domains::ui::views::detail::{
    DetailArtAspect, DetailLayoutInput, DetailTheaterPlateLayout,
    solve_detail_layout,
};
use crate::domains::ui::views::header::view_header;
use crate::domains::ui::views::library::view_library;
use crate::domains::ui::views::library_controls_bar::view_library_controls_bar;
use crate::domains::ui::views::movies::view_movie_detail;
use crate::domains::ui::views::settings::view_unified_settings;
use crate::domains::ui::views::tenfoot::{
    detail::{is_tenfoot_detail_route, view_tenfoot_detail},
    home::{is_tenfoot_home_route, view_tenfoot_home},
    player_overlay::{
        view_loading_status as view_tenfoot_loading_status,
        view_player as view_tenfoot_player,
    },
};
use crate::domains::ui::views::tv::{
    view_episode_detail, view_season_detail, view_series_detail,
};
use crate::domains::ui::views::{view_loading_video, view_video_error};
use crate::domains::ui::widgets::BackgroundEffect;
use crate::domains::{player, ui};
use crate::infra::shader_widgets::background::{
    TheaterPlateGeometry, TheaterPlateScene,
};
use crate::state::State;
use ferrex_core::player_prelude::{
    EpisodeSize, ImageRequest, ImageSize, Media, MediaID, TheaterPlateColor,
    TheaterPlateViewport, theater_plate_backdrop_size_for_viewport,
};
use iced::widget::{Space, Stack, column, container, scrollable};
use iced::{Element, Font, Length, Theme};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view(
    state: &State,
    window_id: iced::window::Id,
) -> Element<'_, DomainMessage, Theme, iced::Renderer> {
    // Dedicated Search window content
    if state
        .windows
        .get(crate::domains::ui::windows::WindowKind::Search)
        .is_some_and(|id| id == window_id)
    {
        return crate::domains::ui::views::components::view_search_window(
            state,
        );
    }
    // debug timing disabled in tests to simplify renderer unification
    // Check for first-run setup
    // Check authentication state
    if !state.is_authenticated {
        let auth_content = view_auth(
            state,
            &state.domains.auth.state.auth_flow,
            state.domains.auth.state.user_permissions.as_ref(),
        )
        .map(DomainMessage::from);

        // Extend the background shader to auth views with a simple gradient
        let mut bg_shader = state
            .domains
            .ui
            .state
            .background_shader_state
            .build_shader(&state.domains.ui.state.view);

        bg_shader = bg_shader.effect(BackgroundEffect::Gradient);

        let bg_shader_element: Element<ui::messages::UiMessage> =
            bg_shader.into();
        let bg_shader_mapped: Element<DomainMessage> =
            bg_shader_element.map(DomainMessage::from);

        return Stack::new()
            .push(bg_shader_mapped)
            .push(auth_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }

    // Get the view content. In 10-foot mode, authenticated shell-like
    // views converge on the TV Home surface while detail/player paths keep
    // their dedicated routes.
    let tenfoot_home_active = is_tenfoot_home_route(state);
    let tenfoot_detail_active = is_tenfoot_detail_route(state);
    let content = if tenfoot_home_active {
        view_tenfoot_home(state).map(DomainMessage::from)
    } else if tenfoot_detail_active {
        view_tenfoot_detail(state).map(DomainMessage::from)
    } else {
        match &state.domains.ui.state.view {
            ViewState::Library => view_library(state).map(DomainMessage::from),
            ViewState::LibraryManagement => {
                view_library_management(state).map(DomainMessage::from)
            }
            ViewState::AdminDashboard => {
                view_admin_dashboard(state).map(DomainMessage::from)
            }
            ViewState::AdminUsers => {
                view_admin_users(state).map(DomainMessage::from)
            }
            ViewState::Player => view_player(state).map(DomainMessage::Player),
            ViewState::LoadingVideo { url } => {
                if state.interface_mode.is_tenfoot() {
                    view_tenfoot_loading_status(url).map(DomainMessage::Player)
                } else {
                    view_loading_video(state, url).map(DomainMessage::from)
                }
            }
            ViewState::VideoError { message } => {
                view_video_error(message).map(DomainMessage::from)
            }
            ViewState::MovieDetail { movie_id, .. } => {
                view_movie_detail(state, *movie_id).map(DomainMessage::from)
            }
            ViewState::SeriesDetail { series_id, .. } => {
                view_series_detail(state, *series_id).map(DomainMessage::from)
            }
            ViewState::SeasonDetail {
                series_id,
                season_id,
                ..
            } => view_season_detail(state, series_id, season_id)
                .map(DomainMessage::from),
            ViewState::EpisodeDetail { episode_id, .. } => {
                view_episode_detail(state, episode_id).map(DomainMessage::from)
            }
            ViewState::UserSettings => {
                view_unified_settings(state).map(DomainMessage::from)
            }
        }
    };

    // Add header if the view needs it
    let content_with_header = if !tenfoot_home_active
        && !tenfoot_detail_active
        && state.domains.ui.state.view.has_header()
    {
        let header = view_header(state).map(DomainMessage::from);

        // Wrap header in a container with opaque background
        let header_container = container(header)
            .width(Length::Fill)
            .style(theme::Container::Header.style());

        // Check if we need library controls bar
        let controls_bar = match &state.domains.ui.state.view {
            ViewState::Library => {
                if let Some(lib_id) = state.domains.ui.state.scope.lib_id()
                    && let Some(lib_type) = state.tab_manager.active_tab_type()
                {
                    view_library_controls_bar(state, lib_id, lib_type)
                        .map(|bar| bar.map(DomainMessage::from))
                } else {
                    None
                }
            }
            _ => None,
        };

        // Check if this is a detail view that needs scrollable content
        let scrollable_content = match &state.domains.ui.state.view {
            ViewState::MovieDetail { .. }
            | ViewState::SeriesDetail { .. }
            | ViewState::SeasonDetail { .. }
            | ViewState::EpisodeDetail { .. } => {
                // Wrap content in scrollable for detail views
                scrollable(content)
                    .on_scroll(|viewport| {
                        DomainMessage::Ui(
                            InteractionMessage::DetailViewScrolled(viewport)
                                .into(),
                        )
                    })
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            }
            _ => {
                // Library and other views already have their own scrollable
                content
            }
        };

        // Build a Stack so header (and optional controls bar) always renders on top of content
        let has_controls = controls_bar.is_some();
        let mut top_bars = column![header_container];
        if let Some(controls) = controls_bar {
            top_bars = top_bars.push(controls);
        }

        // Offset content downward by the height of the header + optional controls bar
        let top_padding: f32 =
            crate::domains::ui::views::library_controls_bar::calculate_top_bars_height(
                has_controls,
            );
        let content_with_offset = column![
            Space::new().height(Length::Fixed(top_padding)),
            scrollable_content,
        ]
        .width(Length::Fill)
        .height(Length::Fill);

        Stack::new()
            // Base layer: main content (offset and effectively shrunk by top bars height)
            .push(content_with_offset)
            // Top layer: header + optional controls bar; overlay ensures it draws last
            .push(top_bars)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        content
    };

    // Use ViewState helper methods for cleaner logic
    let result = if state.domains.ui.state.view.has_background() {
        // Note: Theme colors and backdrops are now handled in the update function
        // when view changes occur, updating state.background_shader_state

        // Create background shader from persistent state
        let mut bg_shader = state
            .domains
            .ui
            .state
            .background_shader_state
            .build_shader(&state.domains.ui.state.view);

        if let Some(context) = detail_theater_plate_context(state) {
            let source_request = context.source.request.as_ref();
            let visual_handle = source_request
                .filter(|_| {
                    context.source.role == TheaterPlateImageRole::Visual
                })
                .and_then(|request| state.image_service.get(request));
            let cache_key = source_request
                .map(theater_plate_cache_key)
                .unwrap_or_else(|| theater_plate_fallback_cache_key(state));
            let scene = source_request
                .and_then(|request| {
                    state.image_service.get_theater_plate_analysis(request)
                })
                .map(|analysis| {
                    TheaterPlateScene::from_analysis(cache_key, &analysis)
                })
                .unwrap_or_else(|| {
                    TheaterPlateScene::missing_backdrop_from_colors(
                        cache_key ^ 0x7a45_706c_6174_6521,
                        context.viewport,
                        context.source.poster_color,
                        context.source.theme_color,
                        context.source.default_color,
                    )
                })
                .with_geometry(theater_plate_geometry_from_layout(
                    context.layout,
                    if visual_handle.is_some() {
                        context.layout.backdrop_opacity
                    } else {
                        0.0
                    },
                ));

            bg_shader = bg_shader
                .effect(BackgroundEffect::TheaterPlate)
                .theater_plate(scene)
                .backdrop_aspect_mode(
                    state
                        .domains
                        .ui
                        .state
                        .background_shader_state
                        .backdrop_aspect_mode,
                );

            if let Some(handle) = visual_handle {
                bg_shader = bg_shader.backdrop(handle);
            }
        } else {
            bg_shader = bg_shader.effect(BackgroundEffect::Gradient);
        }
        // Create a stack with background as base layer
        // Convert bg_shader to Element first, then map from ui::Message to DomainMessage
        let bg_shader_element: Element<ui::messages::UiMessage> =
            bg_shader.into();
        let bg_shader_mapped: Element<DomainMessage> =
            bg_shader_element.map(DomainMessage::from);

        Stack::new()
            .push(bg_shader_mapped)
            .push(content_with_header)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        // For player view, no background
        content_with_header
    };

    let layered = {
        #[cfg(feature = "debug-cache-overlay")]
        {
            let overlay =
                crate::domains::ui::views::cache_debug_overlay::view_cache_debug_overlay(state);
            Stack::new()
                .push(result)
                .push(overlay.map(DomainMessage::from))
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        }

        #[cfg(not(feature = "debug-cache-overlay"))]
        {
            result
        }
    };

    let with_search_overlay =
        if state.domains.search.state.presentation.is_overlay() {
            if let Some(overlay) =
                crate::domains::ui::views::components::view_search_overlay(
                    state,
                )
            {
                Stack::new()
                    .push(layered)
                    .push(overlay)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            } else {
                layered
            }
        } else {
            layered
        };

    // Overlay toast notifications if any are active
    if state.domains.ui.state.toast_manager.has_toasts() {
        let toast_overlay =
            crate::domains::ui::views::toast_overlay::view_toast_overlay(state);
        Stack::new()
            .push(with_search_overlay)
            .push(toast_overlay.map(DomainMessage::from))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        with_search_overlay
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TheaterPlateImageRole {
    Visual,
    AmbientOnly,
}

#[derive(Debug, Clone)]
struct TheaterPlateSource {
    request: Option<ImageRequest>,
    role: TheaterPlateImageRole,
    poster_color: Option<TheaterPlateColor>,
    theme_color: Option<TheaterPlateColor>,
    default_color: TheaterPlateColor,
}

#[derive(Debug, Clone)]
struct DetailTheaterPlateContext {
    layout: DetailTheaterPlateLayout,
    viewport: TheaterPlateViewport,
    source: TheaterPlateSource,
}

fn detail_theater_plate_context(
    state: &State,
) -> Option<DetailTheaterPlateContext> {
    let view = &state.domains.ui.state.view;
    let aspect = match view {
        ViewState::EpisodeDetail { .. } => DetailArtAspect::Still,
        ViewState::MovieDetail { .. }
        | ViewState::SeriesDetail { .. }
        | ViewState::SeasonDetail { .. } => DetailArtAspect::Poster,
        _ => return None,
    };

    let viewport = TheaterPlateViewport::from_logical_size(
        state.window_size.width,
        state.window_size.height,
    );
    let layout_plan = solve_detail_layout(
        DetailLayoutInput::from_runtime(
            state.window_size.width,
            state.window_size.height,
            view.header_height().unwrap_or(0.0),
            state.interface_mode,
            &state.domains.ui.state.size_provider,
            &state.domains.ui.state.scaled_layout,
        )
        .with_hero_art_aspect(aspect),
    );
    let layout = layout_plan.theater_plate_layout(
        state.domains.ui.state.background_shader_state.scroll_offset,
    );

    Some(DetailTheaterPlateContext {
        layout,
        viewport,
        source: theater_plate_source_for_view(state, view, viewport),
    })
}

fn theater_plate_source_for_view(
    state: &State,
    view: &ViewState,
    viewport: TheaterPlateViewport,
) -> TheaterPlateSource {
    let backdrop_size = theater_plate_backdrop_size_for_viewport(viewport);
    let detail_poster_size =
        state.domains.settings.display.detail_poster_quality;
    let mut source = TheaterPlateSource {
        request: None,
        role: TheaterPlateImageRole::AmbientOnly,
        poster_color: None,
        theme_color: None,
        default_color: theater_plate_color_from_iced(
            state.domains.ui.state.background_shader_state.primary_color,
        )
        .unwrap_or(TheaterPlateColor::DEFAULT_STAGE),
    };

    match view {
        ViewState::MovieDetail { movie_id, .. } => {
            if let Ok(Media::Movie(movie)) = state
                .domains
                .ui
                .state
                .repo_accessor
                .get(&MediaID::Movie(*movie_id))
            {
                source.theme_color = movie
                    .theme_color
                    .as_deref()
                    .and_then(TheaterPlateColor::from_hex);
                if let Some(iid) = movie.details.primary_backdrop_iid {
                    source.request =
                        Some(ImageRequest::new(iid, backdrop_size));
                    source.role = TheaterPlateImageRole::Visual;
                } else if let Some(iid) = movie.details.primary_poster_iid {
                    source.request = Some(ImageRequest::new(
                        iid,
                        ImageSize::Poster(detail_poster_size),
                    ));
                    source.role = TheaterPlateImageRole::AmbientOnly;
                }
            }
        }
        ViewState::SeriesDetail { series_id, .. } => {
            if let Ok(Media::Series(series)) = state
                .domains
                .ui
                .state
                .repo_accessor
                .get(&MediaID::Series(*series_id))
            {
                source.theme_color = series
                    .theme_color
                    .as_deref()
                    .and_then(TheaterPlateColor::from_hex);
                if let Some(iid) = series.details.primary_backdrop_iid {
                    source.request =
                        Some(ImageRequest::new(iid, backdrop_size));
                    source.role = TheaterPlateImageRole::Visual;
                } else if let Some(iid) = series.details.primary_poster_iid {
                    source.request = Some(ImageRequest::new(
                        iid,
                        ImageSize::Poster(detail_poster_size),
                    ));
                    source.role = TheaterPlateImageRole::AmbientOnly;
                }
            }
        }
        ViewState::SeasonDetail {
            series_id,
            season_id,
            ..
        } => {
            if let Ok(Media::Season(season)) = state
                .domains
                .ui
                .state
                .repo_accessor
                .get(&MediaID::Season(*season_id))
            {
                source.theme_color = season
                    .theme_color
                    .as_deref()
                    .and_then(TheaterPlateColor::from_hex);
                if let Some(iid) = season.details.primary_poster_iid {
                    source.request = Some(ImageRequest::new(
                        iid,
                        ImageSize::Poster(detail_poster_size),
                    ));
                    source.role = TheaterPlateImageRole::AmbientOnly;
                }
            }
            if let Ok(Media::Series(series)) = state
                .domains
                .ui
                .state
                .repo_accessor
                .get(&MediaID::Series(*series_id))
            {
                source.theme_color = source.theme_color.or_else(|| {
                    series
                        .theme_color
                        .as_deref()
                        .and_then(TheaterPlateColor::from_hex)
                });
                if let Some(iid) = series.details.primary_backdrop_iid {
                    source.request =
                        Some(ImageRequest::new(iid, backdrop_size));
                    source.role = TheaterPlateImageRole::Visual;
                }
            }
        }
        ViewState::EpisodeDetail { episode_id, .. } => {
            if let Ok(Media::Episode(episode)) = state
                .domains
                .ui
                .state
                .repo_accessor
                .get(&MediaID::Episode(*episode_id))
            {
                if let Some(iid) = episode.details.primary_still_iid {
                    source.request = Some(ImageRequest::new(
                        iid,
                        ImageSize::Thumbnail(EpisodeSize::W512),
                    ));
                    source.role = TheaterPlateImageRole::Visual;
                }
                if let Ok(Media::Season(season)) = state
                    .domains
                    .ui
                    .state
                    .repo_accessor
                    .get(&MediaID::Season(episode.season_id))
                {
                    source.theme_color = season
                        .theme_color
                        .as_deref()
                        .and_then(TheaterPlateColor::from_hex);
                    if source.request.is_none()
                        && let Some(iid) = season.details.primary_poster_iid
                    {
                        source.request = Some(ImageRequest::new(
                            iid,
                            ImageSize::Poster(detail_poster_size),
                        ));
                        source.role = TheaterPlateImageRole::AmbientOnly;
                    }
                }
                if let Ok(Media::Series(series)) = state
                    .domains
                    .ui
                    .state
                    .repo_accessor
                    .get(&MediaID::Series(episode.series_id))
                {
                    source.theme_color = source.theme_color.or_else(|| {
                        series
                            .theme_color
                            .as_deref()
                            .and_then(TheaterPlateColor::from_hex)
                    });
                    if source.request.is_none()
                        && let Some(iid) = series.details.primary_poster_iid
                    {
                        source.request = Some(ImageRequest::new(
                            iid,
                            ImageSize::Poster(detail_poster_size),
                        ));
                        source.role = TheaterPlateImageRole::AmbientOnly;
                    }
                }
            }
        }
        _ => {}
    }

    source.poster_color = source.theme_color.or_else(|| {
        theater_plate_color_from_iced(
            state
                .domains
                .ui
                .state
                .background_shader_state
                .secondary_color,
        )
    });
    source
}

fn theater_plate_geometry_from_layout(
    layout: DetailTheaterPlateLayout,
    backdrop_opacity: f32,
) -> TheaterPlateGeometry {
    let center = layout.plate_rect.center();
    let half = layout.plate_rect.half_size();
    TheaterPlateGeometry {
        focused_plate: [center[0], center[1], half[0], half[1]],
        plate_mask: [
            layout.plate_opacity,
            layout.plate_radius_px,
            layout.plate_feather_px,
            layout.side_falloff,
        ],
        scrim_masks: [
            layout.scrim_opacity,
            layout.scrim_rect.y,
            layout.scrim_rect.bottom(),
            layout.side_falloff,
        ],
        hero_art_rect: [
            layout.hero_art_rect.x,
            layout.hero_art_rect.y,
            layout.hero_art_rect.width,
            layout.hero_art_rect.height,
        ],
        ambient_opacity_scale: layout.ambient_opacity_scale,
        vignette_opacity: layout.vignette_opacity,
        grain_opacity_scale: layout.grain_opacity_scale,
        backdrop_opacity,
    }
}

fn theater_plate_color_from_iced(
    color: iced::Color,
) -> Option<TheaterPlateColor> {
    if !color.r.is_finite() || !color.g.is_finite() || !color.b.is_finite() {
        return None;
    }

    Some(TheaterPlateColor::rgb(
        (color.r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.b.clamp(0.0, 1.0) * 255.0).round() as u8,
    ))
}

fn theater_plate_fallback_cache_key(state: &State) -> u64 {
    let mut hasher = DefaultHasher::new();
    "theater-plate-fallback".hash(&mut hasher);
    format!("{:?}", state.domains.ui.state.view).hash(&mut hasher);
    if let Some(color) = theater_plate_color_from_iced(
        state.domains.ui.state.background_shader_state.primary_color,
    ) {
        color.hash(&mut hasher);
    }
    hasher.finish()
}

fn theater_plate_cache_key(request: &ImageRequest) -> u64 {
    let mut hasher = DefaultHasher::new();
    request.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theater_plate_geometry_maps_hero_art_rect_to_uniforms() {
        let layout = DetailTheaterPlateLayout {
            content_rect: DetailTheaterPlateRect {
                x: 0.08,
                y: 0.12,
                width: 0.72,
                height: 0.44,
            },
            plate_rect: DetailTheaterPlateRect {
                x: 0.32,
                y: 0.18,
                width: 0.48,
                height: 0.30,
            },
            scrim_rect: DetailTheaterPlateRect {
                x: 0.0,
                y: 0.08,
                width: 1.0,
                height: 0.52,
            },
            hero_art_rect: DetailTheaterPlateRect {
                x: 0.11,
                y: 0.16,
                width: 0.20,
                height: 0.50,
            },
            plate_opacity: 0.5,
            plate_radius_px: 48.0,
            plate_feather_px: 116.0,
            scrim_opacity: 0.54,
            top_feather_uv: 0.12,
            bottom_feather_uv: 0.34,
            side_falloff: 0.38,
            ambient_opacity_scale: 0.9,
            vignette_opacity: 0.48,
            grain_opacity_scale: 1.0,
            backdrop_opacity: 0.8,
        };

        let geometry = theater_plate_geometry_from_layout(layout, 0.25);
        assert_eq!(geometry.hero_art_rect, [0.11, 0.16, 0.20, 0.50]);

        let scene = TheaterPlateScene::fallback_from_colors(
            7,
            iced::Color::from_rgb(0.1, 0.2, 0.3),
            iced::Color::from_rgb(0.4, 0.3, 0.2),
        )
        .with_geometry(geometry);

        assert_eq!(scene.uniforms.hero_art_rect, [0.11, 0.16, 0.20, 0.50]);
        assert_eq!(scene.uniforms.transition[1], 0.25);
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
fn view_player(state: &State) -> Element<'_, player::messages::PlayerMessage> {
    if state.interface_mode.is_tenfoot() {
        view_tenfoot_player(state)
    } else {
        state.domains.player.state.view()
    }
}

/// Get the lucide font
pub fn lucide_font() -> Font {
    Font::with_name("lucide")
}
