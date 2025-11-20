//! Centralized layout constants for the Ferrex media player
//!
//! This module defines all spacing, sizing, and layout-related constants
//! to ensure consistency across the application and enable future
//! user-configurable scaling preferences.

/// Core poster/card dimensions
pub mod poster {
    /// Base width of a poster/media card in pixels
    pub const BASE_WIDTH: f32 = 200.0;

    /// Base height of a poster/media card in pixels
    pub const BASE_HEIGHT: f32 = 300.0;

    /// Text area height below poster
    pub const TEXT_AREA_HEIGHT: f32 = 60.0;

    /// Total card height including text area
    pub const TOTAL_CARD_HEIGHT: f32 = BASE_HEIGHT + TEXT_AREA_HEIGHT + 5.0; // 365px
}

/// Animation-related constants
pub mod animation {
    /// Scale factor for poster hover/animation effects
    pub const HOVER_SCALE: f32 = 1.05;

    /// Additional padding for shadow/glow effects in pixels
    pub const EFFECT_PADDING: f32 = 10.0;

    /// Calculate horizontal animation padding for a given width
    pub fn calculate_horizontal_padding(width: f32) -> f32 {
        // For a 1.05x scale, the scaled width is width * 1.05
        // The extra width needed is: (width * 1.05 - width) = width * 0.05
        // This needs to be split between left and right, so each side needs: width * 0.05 / 2 = width * 0.025
        let scale_expansion = width * (HOVER_SCALE - 1.0);
        let padding_per_side = scale_expansion / 2.0 + EFFECT_PADDING;
        padding_per_side
    }

    /// Calculate vertical animation padding for a given height
    pub fn calculate_vertical_padding(height: f32) -> f32 {
        // Same calculation as horizontal but using height
        let scale_expansion = height * (HOVER_SCALE - 1.0);
        let padding_per_side = scale_expansion / 2.0 + EFFECT_PADDING;
        padding_per_side
    }

    /// Default animation duration in milliseconds
    pub const DEFAULT_DURATION_MS: u64 = 1000;
}

/// Grid layout constants
pub mod grid {
    /// Effective spacing when using animated containers (reduced to accommodate larger containers)
    pub const EFFECTIVE_SPACING: f32 = 15.0;

    /// Minimum padding on each side of the viewport
    pub const MIN_VIEWPORT_PADDING: f32 = 40.0;

    /// Total horizontal padding (left + right)
    pub const TOTAL_HORIZONTAL_PADDING: f32 = MIN_VIEWPORT_PADDING * 2.0;

    /// Minimum number of columns in a grid
    pub const MIN_COLUMNS: usize = 1;

    /// Maximum number of columns in a grid
    pub const MAX_COLUMNS: usize = 16;

    /// Spacing between rows in virtual grid
    pub const ROW_SPACING: f32 = 50.0;

    /// Additional padding at bottom of grid
    /// Increased to ensure last row is fully visible with text area
    pub const BOTTOM_PADDING: f32 = 100.0;

    /// Top padding to prevent posters from touching the header
    pub const TOP_PADDING: f32 = 20.0;
}

/// Virtual list/grid specific constants
pub mod virtual_grid {
    use super::{animation, poster};

    /// Row height calculation: total card height with animation padding
    /// This matches the actual rendered height from media_card! macro
    /// Animation padding calculation: height * (HOVER_SCALE - 1.0) / 2.0 + EFFECT_PADDING
    /// For BASE_HEIGHT of 300: 300 * 0.05 / 2.0 + 10 = 7.5 + 10 = 17.5 per side
    pub const ROW_HEIGHT: f32 = poster::TOTAL_CARD_HEIGHT
        + 2.0
            * (poster::BASE_HEIGHT * (animation::HOVER_SCALE - 1.0) / 2.0
                + animation::EFFECT_PADDING);
}

/// User preference scales
pub mod scale_presets {
    /// Normal scale factor
    pub const SCALE_NORMAL: f32 = 1.0;

    /// Default scale factor
    pub const DEFAULT_SCALE: f32 = SCALE_NORMAL;
}

/// Backdrop layout constants
pub mod backdrop {
    /// Original aspect ratio of backdrop images (16:9)
    pub const SOURCE_ASPECT: f32 = 16.0 / 9.0; // 1.777...

    /// Target aspect ratio for display (21:9 ultrawide)
    pub const DISPLAY_ASPECT: f32 = 21.0 / 9.0; // 2.333...

    /// Crop factor when converting from 16:9 to 21:9
    /// This represents what percentage of height to keep when cropping
    pub const CROP_FACTOR: f32 = 16.0 / 21.0; // 0.762 (~76.2% of original height)

    /// Crop bias - how much to favor the top of the image when cropping
    /// 0.3 means take 30% of the crop from top, 70% from bottom
    /// This preserves more of the upper content where subjects are typically located
    pub const CROP_BIAS_TOP: f32 = 0.3;

    /// Ultra-wide target aspect ratio for display (30:9 or 10:3)
    pub const DISPLAY_ASPECT_ULTRAWIDE: f32 = 30.0 / 9.0; // 3.333...

    /// Crop factor when converting from 16:9 to 30:9
    /// This represents what percentage of height to keep when cropping
    pub const CROP_FACTOR_ULTRAWIDE: f32 = 16.0 / 30.0; // 0.533 (~53.3% of original height)

    /// Crop bias for ultra-wide - more aggressive top cropping
    /// 0.05 means take only 5% of the crop from top, 95% from bottom
    /// This shows mostly the top portion of the image (titles, logos, sky)
    pub const CROP_BIAS_TOP_ULTRAWIDE: f32 = 0.05;
}

/// Header constants
pub mod header {
    /// Fixed height of the header in pixels
    pub const HEIGHT: f32 = 50.0;
}

/// Helper functions for layout calculations
pub mod calculations {
    use super::{grid, poster};

    /// Calculate the number of columns that fit in a given viewport width
    pub fn calculate_columns(viewport_width: f32, scale: f32) -> usize {
        // Get the actual container dimensions (includes padding for animations)
        let (container_width, _) = get_container_dimensions(scale);
        let available_width = viewport_width - grid::TOTAL_HORIZONTAL_PADDING;

        // Use reduced spacing since containers are larger
        let effective_spacing = grid::EFFECTIVE_SPACING;

        let max_columns = ((available_width + effective_spacing)
            / (container_width + effective_spacing))
            .floor() as usize;

        max_columns.clamp(grid::MIN_COLUMNS, grid::MAX_COLUMNS)
    }

    /// Calculate padding for centered grid layout
    pub fn calculate_grid_padding(viewport_width: f32, columns: usize, scale: f32) -> f32 {
        let (container_width, _) = get_container_dimensions(scale);
        let effective_spacing = grid::EFFECTIVE_SPACING;

        let content_width = columns as f32 * container_width
            + (columns.saturating_sub(1)) as f32 * effective_spacing;

        ((viewport_width - content_width) / 2.0).max(grid::MIN_VIEWPORT_PADDING)
    }

    /// Get container dimensions including animation padding
    pub fn get_container_dimensions(scale: f32) -> (f32, f32) {
        let base_width = poster::BASE_WIDTH * scale;
        let base_height = poster::BASE_HEIGHT * scale;

        // Calculate animation padding for each dimension
        let h_padding = super::animation::calculate_horizontal_padding(base_width);
        let v_padding = super::animation::calculate_vertical_padding(base_height);

        // Container must fit the base poster plus animation padding
        (base_width + h_padding * 2.0, base_height + v_padding * 2.0)
    }
}
