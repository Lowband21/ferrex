//! Root-level view composition

use crate::common::messages::DomainMessage;
use crate::domains::ui::theme;
use crate::domains::ui::types::ViewState;
use crate::domains::ui::views::admin::{view_admin_dashboard, view_library_management};
use crate::domains::ui::views::auth::view_auth;
use crate::domains::ui::views::header::view_header;
use crate::domains::ui::views::library::view_library;
use crate::domains::ui::views::library_controls_bar::view_library_controls_bar;
use crate::domains::ui::views::movies::view_movie_detail;
use crate::domains::ui::views::settings::view_user_settings;
use crate::domains::ui::views::tv::{view_episode_detail, view_season_detail, view_series_detail};
use crate::domains::ui::views::{view_loading_video, view_video_error};
use crate::domains::ui::widgets::BackgroundEffect;
use crate::domains::{player, ui};
use crate::state_refactored::State;
use ferrex_core::{BackdropKind, BackdropSize, ImageRequest, MediaIDLike};
use iced::widget::{Space, Stack, column, container, scrollable};
use iced::{Element, Font, Length};

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view(state: &State) -> Element<'_, DomainMessage> {
    let view = iced::debug::time("ferrex-player::view");
    // Check for first-run setup
    // Check authentication state
    if !state.is_authenticated {
        log::debug!("[Auth] Not authenticated, showing auth view");
        let auth_content = view_auth(
            &state.domains.auth.state.auth_flow,
            state.domains.auth.state.user_permissions.as_ref(),
        )
        .map(DomainMessage::from);
        return auth_content;
    }

    // Get the view content
    let content = match &state.domains.ui.state.view {
        ViewState::Library => view_library(state).map(DomainMessage::from),
        ViewState::LibraryManagement => view_library_management(state).map(DomainMessage::from),
        ViewState::AdminDashboard => view_admin_dashboard(state).map(DomainMessage::from),
        ViewState::Player => view_player(state).map(DomainMessage::Player),
        ViewState::LoadingVideo { url } => view_loading_video(state, url).map(DomainMessage::from),
        ViewState::VideoError { message } => view_video_error(message).map(DomainMessage::from),
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
        } => view_season_detail(state, series_id, season_id).map(DomainMessage::from),
        ViewState::EpisodeDetail { episode_id, .. } => {
            view_episode_detail(state, episode_id).map(DomainMessage::from)
        }
        ViewState::UserSettings => view_user_settings(state).map(DomainMessage::from),
    };

    // Add header if the view needs it
    let content_with_header = if state.domains.ui.state.view.has_header() {
        let header = view_header(state).map(DomainMessage::from);

        // Wrap header in a container with opaque background
        let header_container = container(header)
            .width(Length::Fill)
            .style(theme::Container::Header.style());

        // Check if we need library controls bar
        let selected_library = state
            .domains
            .library
            .state
            .current_library_id
            .as_ref()
            .map(|id| id.as_uuid());
        let controls_bar = match &state.domains.ui.state.view {
            ViewState::Library => view_library_controls_bar(state, selected_library)
                .map(|bar| bar.map(DomainMessage::from)),
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
                        DomainMessage::from(ui::messages::Message::DetailViewScrolled(viewport))
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

        // Get backdrop from image service based on current view (reactive approach)
        let (fade_start, fade_end) = state
            .domains
            .ui
            .state
            .background_shader_state
            .backdrop_fade();

        let backdrop_handle = match &state.domains.ui.state.view {
            ViewState::MovieDetail { movie_id, .. } => {
                let request = ImageRequest::backdrop(
                    movie_id.to_uuid(),
                    BackdropKind::Movie,
                    BackdropSize::Quality,
                );
                state.image_service.get(&request)
            }
            ViewState::SeriesDetail { series_id, .. } => {
                let request = ImageRequest::backdrop(
                    series_id.to_uuid(),
                    BackdropKind::Series,
                    BackdropSize::Quality,
                );
                state.image_service.get(&request)
            }
            ViewState::SeasonDetail { season_id, .. } => {
                let request = ImageRequest::backdrop(
                    season_id.to_uuid(),
                    BackdropKind::Season,
                    BackdropSize::Quality,
                );
                state.image_service.get(&request)
            }
            _ => None,
        };

        // Add backdrop if available
        if let Some(handle) = backdrop_handle {
            // Use BackdropGradient effect with configured fade window from persistent state
            bg_shader = bg_shader
                .backdrop_with_fade(handle, fade_start, fade_end)
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
        let bg_shader_element: Element<ui::messages::Message> = bg_shader.into();
        let bg_shader_mapped: Element<DomainMessage> = bg_shader_element.map(DomainMessage::from);

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

    view.finish();
    // Check search mode and render appropriate view
    if state.domains.search.state.mode == crate::domains::search::types::SearchMode::FullScreen {
        // Show full-screen search view
        crate::domains::ui::views::components::view_search_fullscreen(state)
    } else {
        match crate::domains::ui::views::components::view_search_dropdown(state) {
            Some(search_dropdown) => {
                // Wrap the main content in a stack with the search dropdown overlay
                Stack::new()
                    .push(result)
                    .push(search_dropdown)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            }
            _ => result,
        }
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
fn view_player(state: &State) -> Element<'_, player::messages::Message> {
    state.domains.player.state.view()
}

/// Get the lucide font
pub fn lucide_font() -> Font {
    Font::with_name("lucide")
}
