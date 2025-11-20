//! Style constants and theme integration for media cards

use iced::Color;
use once_cell::sync::Lazy;
use std::collections::HashMap;

/// Card theme configuration
#[derive(Debug, Clone)]
pub struct CardTheme {
    /// Background color for cards
    pub background: Color,
    /// Background color on hover
    pub hover_background: Color,
    /// Border color
    pub border_color: Color,
    /// Border width
    pub border_width: f32,
    /// Shadow configuration
    pub shadow: ShadowConfig,
    /// Text colors
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_dimmed: Color,
    /// Overlay colors
    pub overlay_background: Color,
    pub overlay_border: Color,
    /// Animation durations (in ms)
    pub hover_transition_ms: u64,
    pub load_animation_ms: u64,
    /// Scale factor on hover
    pub hover_scale: f32,
}

/// Shadow configuration
#[derive(Debug, Clone)]
pub struct ShadowConfig {
    pub color: Color,
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur_radius: f32,
}

/// Default card themes
pub static CARD_STYLES: Lazy<HashMap<&'static str, CardTheme>> = Lazy::new(|| {
    let mut themes = HashMap::new();

    // Dark theme (default)
    themes.insert(
        "dark",
        CardTheme {
            background: Color::from_rgb(0.11, 0.11, 0.14),
            hover_background: Color::from_rgb(0.15, 0.15, 0.18),
            border_color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
            border_width: 1.0,
            shadow: ShadowConfig {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.3),
                offset_x: 0.0,
                offset_y: 4.0,
                blur_radius: 8.0,
            },
            text_primary: Color::from_rgb(1.0, 1.0, 1.0),
            text_secondary: Color::from_rgba(1.0, 1.0, 1.0, 0.7),
            text_dimmed: Color::from_rgba(1.0, 1.0, 1.0, 0.5),
            overlay_background: Color::from_rgba(0.0, 0.0, 0.0, 0.6),
            overlay_border: Color::from_rgb(0.0, 0.47, 1.0), // Accent blue
            hover_transition_ms: 200,
            load_animation_ms: 600,
            hover_scale: 1.03,
        },
    );

    // Light theme
    themes.insert(
        "light",
        CardTheme {
            background: Color::from_rgb(1.0, 1.0, 1.0),
            hover_background: Color::from_rgb(0.96, 0.96, 0.98),
            border_color: Color::from_rgba(0.0, 0.0, 0.0, 0.1),
            border_width: 1.0,
            shadow: ShadowConfig {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.1),
                offset_x: 0.0,
                offset_y: 2.0,
                blur_radius: 4.0,
            },
            text_primary: Color::from_rgb(0.1, 0.1, 0.1),
            text_secondary: Color::from_rgba(0.0, 0.0, 0.0, 0.6),
            text_dimmed: Color::from_rgba(0.0, 0.0, 0.0, 0.4),
            overlay_background: Color::from_rgba(0.0, 0.0, 0.0, 0.7),
            overlay_border: Color::from_rgb(0.0, 0.47, 1.0),
            hover_transition_ms: 200,
            load_animation_ms: 600,
            hover_scale: 1.03,
        },
    );

    // High contrast theme
    themes.insert(
        "high_contrast",
        CardTheme {
            background: Color::BLACK,
            hover_background: Color::from_rgb(0.2, 0.2, 0.2),
            border_color: Color::WHITE,
            border_width: 2.0,
            shadow: ShadowConfig {
                color: Color::TRANSPARENT,
                offset_x: 0.0,
                offset_y: 0.0,
                blur_radius: 0.0,
            },
            text_primary: Color::WHITE,
            text_secondary: Color::from_rgb(0.9, 0.9, 0.9),
            text_dimmed: Color::from_rgb(0.7, 0.7, 0.7),
            overlay_background: Color::from_rgba(0.0, 0.0, 0.0, 0.8),
            overlay_border: Color::from_rgb(1.0, 1.0, 0.0), // Yellow for visibility
            hover_transition_ms: 100,
            load_animation_ms: 400,
            hover_scale: 1.05,
        },
    );

    themes
});

/// Get the current theme (can be made dynamic later)
pub fn current_theme() -> &'static CardTheme {
    CARD_STYLES.get("dark").expect("Default theme should exist")
}

/// Style constants for consistent spacing and sizing
pub mod constants {
    /// Spacing between cards in a grid
    pub const GRID_SPACING: f32 = 20.0;

    /// Padding inside cards
    pub const CARD_PADDING: f32 = 8.0;

    /// Text area heights for different card sizes
    pub const TEXT_HEIGHT_SMALL: f32 = 45.0;
    pub const TEXT_HEIGHT_MEDIUM: f32 = 60.0;
    pub const TEXT_HEIGHT_LARGE: f32 = 75.0;
    pub const TEXT_HEIGHT_WIDE: f32 = 65.0;

    /// Icon sizes
    pub const ICON_SIZE_SMALL: u16 = 16;
    pub const ICON_SIZE_MEDIUM: u16 = 20;
    pub const ICON_SIZE_LARGE: u16 = 32;

    /// Overlay button padding
    pub const OVERLAY_BUTTON_PADDING: u16 = 8;
    pub const OVERLAY_CENTER_BUTTON_PADDING: u16 = 16;

    /// Badge dimensions
    pub const BADGE_PADDING: u16 = 5;
    pub const BADGE_MIN_WIDTH: f32 = 40.0;

    /// Animation stagger delays (ms between items)
    pub const STAGGER_DELAY_MS: u64 = 50;

    /// Maximum cards per row (responsive)
    pub const MAX_CARDS_PER_ROW: usize = 8;
    pub const MIN_CARDS_PER_ROW: usize = 2;

    /// Minimum window width for responsive layouts
    pub const MIN_WINDOW_WIDTH: f32 = 600.0;
}

/// Badge style presets
pub mod badge_styles {
    use super::*;

    pub fn rating_badge() -> (Color, Color) {
        (
            Color::from_rgba(1.0, 0.84, 0.0, 0.2),
            Color::from_rgb(1.0, 0.84, 0.0),
        )
    }

    pub fn new_badge() -> (Color, Color) {
        (
            Color::from_rgba(0.0, 1.0, 0.0, 0.2),
            Color::from_rgb(0.0, 1.0, 0.0),
        )
    }

    pub fn episode_count_badge() -> (Color, Color) {
        (
            Color::from_rgba(0.0, 0.47, 1.0, 0.2),
            Color::from_rgb(0.4, 0.7, 1.0),
        )
    }
}

/// Helper functions for responsive sizing
pub mod responsive {
    use super::constants::*;

    /// Calculate number of columns based on window width
    pub fn calculate_columns(window_width: f32, card_width: f32, padding: f32) -> usize {
        // Delegate to centralized calculation
        crate::constants::calculations::calculate_columns(window_width, 1.0)
    }

    /// Calculate grid padding for centered layout
    pub fn calculate_grid_padding(window_width: f32, columns: usize, card_width: f32) -> f32 {
        // Delegate to centralized calculation
        crate::constants::calculations::calculate_grid_padding(window_width, columns, 1.0)
    }

    /// Determine card size based on viewport
    pub fn adaptive_card_size(window_width: f32) -> crate::views::cards::types::CardSize {
        use crate::views::cards::types::CardSize;

        if window_width < 800.0 {
            CardSize::Small
        } else if window_width < 1400.0 {
            CardSize::Medium
        } else {
            CardSize::Large
        }
    }
}
