//! Background shader state management

use crate::{
    common::ViewState,
    domains::ui::{
        types::BackdropAspectMode,
        views::library_controls_bar,
        widgets::{BackgroundEffect, DepthLayout, background_shader},
    },
    infra::{
        constants::layout::{backdrop, detail, header},
        shader_widgets::background::{
            BackgroundShader, DepthRegion, EdgeTransition,
            transitions::{
                BackdropTransitionState, ColorTransitionState,
                GradientTransitionState, generate_random_gradient_center,
            },
        },
    },
};

/// Computed backdrop dimensions - single source of truth for positioning
#[derive(Debug, Clone, Copy, Default)]
pub struct BackdropDimensions {
    /// Full backdrop height in pixels (window_width / display_aspect)
    pub height: f32,
    /// Y coordinate where content should start (at backdrop bottom)
    pub content_start_y: f32,
    /// Height for button container positioning within backdrop
    pub button_height: f32,
    /// Coverage in UV space (0.0-1.0) for shader
    pub coverage_uv: f32,
}

/// Persistent state for the background shader
#[derive(Debug, Clone)]
pub struct BackgroundShaderState {
    pub effect: BackgroundEffect,
    pub primary_color: iced::Color,
    pub secondary_color: iced::Color,
    pub backdrop_handle: Option<iced::widget::image::Handle>,
    pub backdrop_aspect_ratio: Option<f32>,
    pub backdrop_aspect_mode: BackdropAspectMode,
    pub backdrop_fade_start: f32,
    pub backdrop_fade_end: f32,
    pub scroll_offset: f32,
    pub gradient_center: (f32, f32),
    pub depth_layout: DepthLayout,

    // Transition states
    pub color_transitions: ColorTransitionState,
    pub backdrop_transitions: BackdropTransitionState,
    pub gradient_transitions: GradientTransitionState,
}

impl Default for BackgroundShaderState {
    fn default() -> Self {
        use crate::domains::ui::theme::MediaServerTheme;
        let primary = MediaServerTheme::LIBRARY_BG_PRIMARY;
        let secondary = MediaServerTheme::LIBRARY_BG_SECONDARY;
        let initial_center = generate_random_gradient_center();
        Self {
            effect: BackgroundEffect::Gradient,
            primary_color: primary,
            secondary_color: secondary,
            backdrop_handle: None,
            backdrop_aspect_ratio: Some(
                crate::infra::constants::layout::backdrop::SOURCE_ASPECT,
            ),
            backdrop_aspect_mode: BackdropAspectMode::default(),
            backdrop_fade_start: 0.92,
            backdrop_fade_end: 1.0,
            scroll_offset: 0.0,
            gradient_center: initial_center,
            depth_layout: DepthLayout {
                regions: Vec::new(),
                ambient_light_direction: iced::Vector::new(0.707, 0.707), // Light from bottom-right
                base_depth: 0.0,
                shadow_intensity: 0.4,
                shadow_distance: 40.0,
            },

            // Initialize transition states
            color_transitions: ColorTransitionState::new(primary, secondary),
            backdrop_transitions: BackdropTransitionState::new(),
            gradient_transitions: GradientTransitionState::new(initial_center),
        }
    }
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl BackgroundShaderState {
    /// Build a configured background shader instance for the provided view state.
    /// All shared properties (color, offsets, depth layout) are sourced from this persistent state
    /// so call sites do not need to manually wire them each frame.
    pub fn build_shader(&self, view: &ViewState) -> BackgroundShader {
        let mut shader = background_shader()
            .colors(self.primary_color, self.secondary_color)
            .scroll_offset(self.scroll_offset)
            .gradient_center(self.gradient_center)
            .backdrop_aspect_mode(self.backdrop_aspect_mode)
            .backdrop_aspect_ratio(self.backdrop_aspect_ratio)
            .effect(self.effect.clone());

        if !self.depth_layout.regions.is_empty() {
            shader = shader.with_depth_layout(self.depth_layout.clone());
        }

        if let Some(handle) = self.backdrop_handle.clone() {
            shader = shader.backdrop(handle);
        }

        if matches!(
            view,
            ViewState::MovieDetail { .. }
                | ViewState::SeriesDetail { .. }
                | ViewState::SeasonDetail { .. }
                | ViewState::EpisodeDetail { .. }
        ) {
            shader = shader.header_offset(header::HEIGHT);
        }

        shader
    }

    /// Retrieve the configured fade window for backdrop images.
    pub fn backdrop_fade(&self) -> (f32, f32) {
        (self.backdrop_fade_start, self.backdrop_fade_end)
    }

    /// Updates depth regions based on the current view and window size
    pub fn update_depth_lines(
        &mut self,
        view: &ViewState,
        window_width: f32,
        window_height: f32,
        current_library_id: Option<uuid::Uuid>,
    ) {
        self.depth_layout.regions.clear();

        log::debug!(
            "Updating depth lines for view: {:?}, window: {}x{}",
            view,
            window_width,
            window_height
        );

        match view {
            ViewState::Library => {
                let content_start =
                    library_controls_bar::calculate_top_bars_height(
                        current_library_id.is_some(),
                    );

                // Content region (flat)
                self.depth_layout.regions.push(DepthRegion {
                    bounds: iced::Rectangle {
                        x: 0.0,
                        y: content_start,
                        //y: 0.0,
                        width: window_width,
                        height: window_height - content_start,
                        //height: window_height,
                    },
                    depth: -10.0, // Content is flat
                    edge_transition: EdgeTransition::Soft { width: 5.0 },
                    edge_overrides: Default::default(),
                    shadow_enabled: true,
                    shadow_intensity: 1.0,
                    z_order: 1,
                    border: None,
                });

                log::debug!(
                    "Added library regions with controls: {}, start: {}",
                    current_library_id.is_some(),
                    content_start
                );
            }
            ViewState::MovieDetail { .. }
            | ViewState::SeriesDetail { .. }
            | ViewState::SeasonDetail { .. }
            | ViewState::EpisodeDetail { .. } => {
                // Account for scroll offset
                let scroll_offset = self.scroll_offset;
                // Use centralized backdrop dimensions calculation
                let backdrop_dims = self
                    .calculate_backdrop_dimensions(window_width, window_height);
                let backdrop_height = backdrop_dims.height;
                // Content top is backdrop height minus scroll offset
                let content_top = backdrop_dims.content_start_y - scroll_offset;
                let poster_width = detail::POSTER_WIDTH;
                let poster_height = detail::POSTER_HEIGHT;
                let poster_padding = detail::POSTER_PADDING;
                let poster_left = 0.0;
                let poster_right =
                    poster_left + poster_width + detail::POSTER_METADATA_GAP;
                let poster_bottom =
                    content_top + poster_height + poster_padding;

                // Backdrop region (flat, no shadows)
                self.depth_layout.regions.push(DepthRegion {
                    bounds: iced::Rectangle {
                        x: 0.0,
                        y: 0.0, // Now starts at top since header is outside scrollable
                        width: window_width,
                        height: backdrop_height - scroll_offset,
                    },
                    depth: 0.0,
                    edge_transition: EdgeTransition::Sharp,
                    edge_overrides: Default::default(),
                    shadow_enabled: false,
                    shadow_intensity: 0.0,
                    z_order: 1,
                    border: None,
                });

                // Poster region (sunken)
                self.depth_layout.regions.push(DepthRegion {
                    bounds: iced::Rectangle {
                        x: poster_left,
                        y: content_top,
                        width: poster_right,
                        height: poster_height + 30.0,
                    },
                    depth: -2.0,
                    edge_transition: EdgeTransition::Sharp,
                    edge_overrides: Default::default(),
                    shadow_enabled: true,
                    shadow_intensity: 1.0,
                    z_order: 2,
                    border: None,
                });

                // Content region (flat)
                self.depth_layout.regions.push(DepthRegion {
                    bounds: iced::Rectangle {
                        x: 0.0,
                        y: content_top,
                        width: window_width,
                        height: poster_height + 30.0,
                    },
                    depth: 0.0,
                    edge_transition: EdgeTransition::Sharp,
                    edge_overrides: Default::default(),
                    shadow_enabled: true,
                    shadow_intensity: 1.0,
                    z_order: 1,
                    border: None,
                });

                log::debug!(
                    "Added detail view regions - backdrop_height: {}, content_top: {}, poster_bottom: {}",
                    backdrop_height,
                    content_top,
                    poster_bottom
                );
            }
            _ => {
                // Other views have no special depth regions
            }
        }
    }

    /*/// Calculate the aspect ratio to use for backdrop display
    pub fn calculate_display_aspect(&self, window_width: f32, window_height: f32) -> f32 {
        match self.backdrop_aspect_mode {
            BackdropAspectMode::Auto => {
                let window_aspect = window_width / window_height;
                // If window is wider than 16:9, use backdrop's natural aspect ratio or 16:9
                // Otherwise use 21:9 to ensure proper coverage
                if window_aspect > 1.777 {
                    self.backdrop_aspect_ratio.unwrap_or(1.777) // 16:9 default
                } else {
                    2.37 // 21:9 for narrower windows
                }
            }
            BackdropAspectMode::Force21x9 => 2.37, // Always use 21:9
        }
    }*/
    /// Calculate all backdrop dimensions - SINGLE SOURCE OF TRUTH
    ///
    /// This method should be used by both UI layout code and shader setup
    /// to ensure consistent positioning across the entire system.
    pub fn calculate_backdrop_dimensions(
        &self,
        window_width: f32,
        window_height: f32,
    ) -> BackdropDimensions {
        let display_aspect =
            self.calculate_display_aspect(window_width, window_height);
        let height = window_width / display_aspect;
        let coverage_uv = (height / window_height).min(1.0);

        BackdropDimensions {
            height,
            content_start_y: height, // Content starts exactly at backdrop bottom
            button_height: height - backdrop::BUTTON_BOTTOM_MARGIN,
            coverage_uv,
        }
    }

    /// Calculate content offset for detail views based on backdrop dimensions
    pub fn calculate_content_offset_height(
        &self,
        window_width: f32,
        window_height: f32,
    ) -> f32 {
        let dims =
            self.calculate_backdrop_dimensions(window_width, window_height);
        dims.content_start_y - header::HEIGHT
    }

    /// Calculate the display aspect ratio based on mode and window dimensions
    pub fn calculate_display_aspect(
        &self,
        window_width: f32,
        window_height: f32,
    ) -> f32 {
        use crate::infra::constants::layout::backdrop;

        match self.backdrop_aspect_mode {
            BackdropAspectMode::Force21x9 => backdrop::DISPLAY_ASPECT,
            BackdropAspectMode::Auto => {
                // Use 30:9 for wide windows, 21:9 for tall windows
                if window_width >= window_height {
                    backdrop::DISPLAY_ASPECT_ULTRAWIDE
                } else {
                    backdrop::DISPLAY_ASPECT
                }
            }
        }
    }

    /// Reset colors to library view defaults with smooth transition
    pub fn reset_to_library_colors(&mut self) {
        use crate::domains::ui::theme::MediaServerTheme;
        self.color_transitions.transition_to(
            MediaServerTheme::LIBRARY_BG_PRIMARY,
            MediaServerTheme::LIBRARY_BG_SECONDARY,
        );
    }

    /// Reset colors to specific view defaults
    pub fn reset_to_view_colors(&mut self, view: &ViewState) {
        match view {
            ViewState::Library
            | ViewState::LibraryManagement
            | ViewState::AdminDashboard
            | ViewState::UserSettings => {
                // All these views use library default colors
                self.reset_to_library_colors();
            }
            // Detail views keep their media-specific colors
            _ => {
                // No color reset for detail views - they maintain their media colors
            }
        }
    }
}
