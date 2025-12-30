//! Root-level view composition

use crate::common::messages::DomainMessage;
use crate::domains::ui::interaction_ui::InteractionMessage;
use crate::domains::ui::theme;
use crate::domains::ui::types::ViewState;
use crate::domains::ui::views::admin::{
    view_admin_dashboard, view_admin_users, view_library_management,
};
use crate::domains::ui::views::auth::view_auth;
use crate::domains::ui::views::header::view_header;
use crate::domains::ui::views::library::view_library;
use crate::domains::ui::views::library_controls_bar::view_library_controls_bar;
use crate::domains::ui::views::movies::view_movie_detail;
use crate::domains::ui::views::settings::view_unified_settings;
use crate::domains::ui::views::tv::{
    view_episode_detail, view_season_detail, view_series_detail,
};
use crate::domains::ui::views::{view_loading_video, view_video_error};
use crate::domains::ui::widgets::BackgroundEffect;
use crate::domains::{player, ui};
use crate::state::State;
use ferrex_core::player_prelude::{ImageRequest, ImageSize, Media, MediaID};
use iced::widget::{Space, Stack, column, container, scrollable};
use iced::{Element, Font, Length, Theme};

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

    // Get the view content
    let content = match &state.domains.ui.state.view {
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
            view_loading_video(state, url).map(DomainMessage::from)
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
    };

    // Add header if the view needs it
    let content_with_header = if state.domains.ui.state.view.has_header() {
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

        let backdrop_iid = match &state.domains.ui.state.view {
            ViewState::MovieDetail { movie_id, .. } => state
                .domains
                .ui
                .state
                .repo_accessor
                .get(&MediaID::Movie(*movie_id))
                .ok()
                .and_then(|m| match m {
                    Media::Movie(mr) => mr.details.primary_backdrop_iid,
                    _ => None,
                }),
            ViewState::SeriesDetail { series_id, .. } => state
                .domains
                .ui
                .state
                .repo_accessor
                .get(&MediaID::Series(*series_id))
                .ok()
                .and_then(|m| match m {
                    Media::Series(sr) => sr.details.primary_backdrop_iid,
                    _ => None,
                }),
            ViewState::SeasonDetail { series_id, .. } => state
                .domains
                .ui
                .state
                .repo_accessor
                .get(&MediaID::Series(*series_id))
                .ok()
                .and_then(|m| match m {
                    Media::Series(sr) => sr.details.primary_backdrop_iid,
                    _ => None,
                }),
            _ => None,
        };

        let backdrop_handle = backdrop_iid.and_then(|iid| {
            let request = ImageRequest::new(iid, ImageSize::backdrop());
            state.image_service.get(&request)
        });

        // Add backdrop if available
        if let Some(handle) = backdrop_handle {
            bg_shader = bg_shader
                .effect(BackgroundEffect::BackdropGradient)
                .backdrop(handle)
                .backdrop_aspect_mode(
                    state
                        .domains
                        .ui
                        .state
                        .background_shader_state
                        .backdrop_aspect_mode,
                );
        } else {
            // No backdrop, use gradient effect
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

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
fn view_player(state: &State) -> Element<'_, player::messages::PlayerMessage> {
    state.domains.player.state.view()
}

/// Get the lucide font
pub fn lucide_font() -> Font {
    Font::with_name("lucide")
}
