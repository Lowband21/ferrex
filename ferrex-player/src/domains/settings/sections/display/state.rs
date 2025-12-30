//! Display section state
//!
//! Contains all state related to display/UI settings.
//! Many of these correspond to constants in infra::constants::layout

use ferrex_model::PosterSize;
use serde::{Deserialize, Serialize};

pub const DEFAULT_SCROLLBAR_SCROLLER_MIN_LENGTH_PX: f32 = 24.0;

fn default_grid_poster_gap() -> f32 {
    15.0
}

fn default_scrollbar_scroller_min_length_px() -> f32 {
    DEFAULT_SCROLLBAR_SCROLLER_MIN_LENGTH_PX
}

/// Display settings state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayState {
    // Theme subsection
    /// Current theme preference
    pub theme: ThemePreference,

    // Grid Layout subsection
    /// Grid size preset
    pub grid_size: GridSize,
    /// Show poster titles on hover
    pub poster_titles_on_hover: bool,
    /// Show recently watched section on home
    pub show_recently_watched: bool,
    /// Show continue watching section on home
    pub show_continue_watching: bool,
    /// Sidebar collapsed by default
    pub sidebar_collapsed: bool,

    // Poster subsection (from constants::layout::poster)
    /// Poster base width in pixels (default: 200.0)
    pub poster_base_width: f32,
    /// Poster base height in pixels (default: 300.0)
    pub poster_base_height: f32,
    /// Poster corner radius in pixels (default: 6.0)
    pub poster_corner_radius: f32,
    /// Poster text area height in pixels (default: 60.0)
    pub poster_text_area_height: f32,

    // Spacing subsection (from constants::layout::grid)
    /// Gap between poster columns in pixels (unscaled).
    ///
    /// This value is scaled by the application's effective scale at runtime.
    ///
    /// Backwards-compat: formerly stored as `grid_effective_spacing`.
    #[serde(
        default = "default_grid_poster_gap",
        alias = "grid_effective_spacing"
    )]
    pub grid_poster_gap: f32,
    /// Grid row spacing in pixels (default: 50.0)
    pub grid_row_spacing: f32,
    /// Minimum viewport padding in pixels (default: 40.0)
    pub grid_viewport_padding: f32,
    /// Grid top padding in pixels (default: 20.0)
    pub grid_top_padding: f32,
    /// Grid bottom padding in pixels (default: 100.0)
    pub grid_bottom_padding: f32,

    // Animation subsection (from constants::layout::animation)
    /// Hover scale factor (default: 1.05)
    pub animation_hover_scale: f32,
    /// Default animation duration in ms (default: 600)
    pub animation_default_duration_ms: u64,
    /// Initial texture fade duration in ms (default: 600)
    pub animation_texture_fade_initial_ms: u64,
    /// Texture fade duration in ms (default: 400)
    pub animation_texture_fade_ms: u64,

    // Poster Quality subsection
    /// Poster quality for library/carousel views (default: W342)
    pub library_poster_quality: PosterSize,
    /// Poster quality for detail views - hero poster (default: W780)
    pub detail_poster_quality: PosterSize,

    // Scrollbar subsection
    /// Minimum length (height/width) of scrollable scrollers in pixels.
    ///
    /// This applies globally to all scrollables at runtime.
    #[serde(default = "default_scrollbar_scroller_min_length_px")]
    pub scrollbar_scroller_min_length_px: f32,
}

impl Default for DisplayState {
    fn default() -> Self {
        Self {
            // Theme
            theme: ThemePreference::default(),

            // Grid Layout
            grid_size: GridSize::default(),
            poster_titles_on_hover: true,
            show_recently_watched: true,
            show_continue_watching: true,
            sidebar_collapsed: false,

            // Poster (matches constants::layout::poster defaults)
            poster_base_width: 200.0,
            poster_base_height: 300.0,
            poster_corner_radius: 6.0,
            poster_text_area_height: 60.0,

            // Spacing (matches constants::layout::grid defaults)
            grid_poster_gap: default_grid_poster_gap(),
            grid_row_spacing: 50.0,
            grid_viewport_padding: 40.0,
            grid_top_padding: 20.0,
            grid_bottom_padding: 100.0,

            // Animation (matches constants::layout::animation defaults)
            animation_hover_scale: 1.05,
            animation_default_duration_ms: 600,
            animation_texture_fade_initial_ms: 600,
            animation_texture_fade_ms: 400,

            // Poster Quality
            library_poster_quality: PosterSize::W185,
            detail_poster_quality: PosterSize::W780,

            // Scrollbar
            scrollbar_scroller_min_length_px:
                default_scrollbar_scroller_min_length_px(),
        }
    }
}

/// Theme preference
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemePreference {
    pub const ALL: [ThemePreference; 3] =
        [Self::System, Self::Light, Self::Dark];
}

impl std::fmt::Display for ThemePreference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::System => write!(f, "System"),
            Self::Light => write!(f, "Light"),
            Self::Dark => write!(f, "Dark"),
        }
    }
}

/// Grid size preset
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum GridSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl GridSize {
    pub const ALL: [GridSize; 3] = [Self::Small, Self::Medium, Self::Large];

    /// Get scale factor for this grid size
    pub fn scale_factor(&self) -> f32 {
        match self {
            Self::Small => 0.8,
            Self::Medium => 1.0,
            Self::Large => 1.2,
        }
    }
}

impl std::fmt::Display for GridSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Small => write!(f, "Small"),
            Self::Medium => write!(f, "Medium"),
            Self::Large => write!(f, "Large"),
        }
    }
}
