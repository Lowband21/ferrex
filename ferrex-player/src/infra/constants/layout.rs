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

    /// Corner radius for poster/media card corners in pixels
    pub const CORNER_RADIUS: f32 = 6.0;

    /// Text area height below poster
    pub const TEXT_AREA_HEIGHT: f32 = 30.0;

    /// Total card height including text area
    pub const TOTAL_CARD_HEIGHT: f32 = BASE_HEIGHT + TEXT_AREA_HEIGHT;
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

        scale_expansion / 2.0 + EFFECT_PADDING
    }

    /// Calculate vertical animation padding for a given height
    pub fn calculate_vertical_padding(height: f32) -> f32 {
        // Same calculation as horizontal but using height
        let scale_expansion = height * (HOVER_SCALE - 1.0);

        scale_expansion / 2.0 + EFFECT_PADDING
    }

    /// Default animation duration in milliseconds
    pub const DEFAULT_DURATION_MS: u64 = 600;

    /// Duration of the texture opacity cross-fade (milliseconds)
    /// Quick fade for transitioning from placeholder to actual poster
    pub const TEXTURE_FADE_INITIAL_DURATION_MS: u64 = 600;
    pub const TEXTURE_FADE_DURATION_MS: u64 = 400;
}

/// Grid layout constants
pub mod grid {
    /// Effective spacing when using animated containers (reduced to accommodate larger containers)
    pub const EFFECTIVE_SPACING: f32 = 15.0;

    /// Minimum padding on each side of the viewport
    pub const MIN_VIEWPORT_PADDING: f32 = 10.0; // 40.0;

    /// Total horizontal padding (left + right)
    pub const TOTAL_HORIZONTAL_PADDING: f32 = MIN_VIEWPORT_PADDING * 2.0;

    /// Minimum number of columns in a grid
    pub const MIN_COLUMNS: usize = 1;

    /// Maximum number of columns in a grid
    pub const MAX_COLUMNS: usize = 32; // 16;

    /// Spacing between rows in virtual grid
    pub const ROW_SPACING: f32 = 15.0; // 50.0;

    /// Additional padding at bottom of grid
    /// Increased to ensure last row is fully visible with text area
    pub const BOTTOM_PADDING: f32 = 100.0;

    /// Top padding to prevent posters from touching the header
    pub const TOP_PADDING: f32 = 30.0;
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

    /// Number of rows above the viewport to include in the visible/preload window
    pub const PREFETCH_ROWS_ABOVE: usize = 1;
    // Number of rows below the viewport to include in the visible/preload window
    pub const PREFETCH_ROWS_BELOW: usize = 1;
    /// Additional rows beyond the preload window to treat as low-priority background work
    pub const BACKGROUND_ROWS_BELOW: usize = 0;

    /// Keep-alive duration after scroll (ms) to allow placeholder->texture swaps to complete
    pub const KEEP_ALIVE_MS: u64 = 50000;
}

/// Player controls layout constants
pub mod player_controls {
    /// Padding around control buttons container (all sides)
    pub const CONTROL_BUTTONS_PADDING: f32 = 40.0;

    /// Height of the control buttons row (based on volume slider container height)
    pub const CONTROL_BUTTONS_HEIGHT: f32 = 36.0;

    /// Seek bar hit zone height (clickable area)
    pub const SEEK_BAR_HIT_ZONE_HEIGHT: f32 = 30.0;

    /// Total height of the control buttons container including padding
    /// Bottom padding (40) + control buttons (36) + top padding (40) = 116px
    pub const CONTROL_CONTAINER_TOTAL_HEIGHT: f32 =
        CONTROL_BUTTONS_PADDING * 2.0 + CONTROL_BUTTONS_HEIGHT;

    /// Distance from bottom of screen to the visual center of the seek bar
    /// The seek bar sits directly above the control container
    /// Calculation: CONTROL_CONTAINER_TOTAL_HEIGHT + SEEK_BAR_HIT_ZONE_HEIGHT/2
    /// = 116 + 15 = 131px
    pub const SEEK_BAR_CENTER_FROM_BOTTOM: f32 =
        CONTROL_CONTAINER_TOTAL_HEIGHT + SEEK_BAR_HIT_ZONE_HEIGHT / 2.0;

    /// Distance from bottom of screen to the bottom edge of the seek bar hit zone
    /// This is where the seek bar's clickable area begins
    pub const SEEK_BAR_BOTTOM_EDGE: f32 = CONTROL_CONTAINER_TOTAL_HEIGHT;

    /// Padding around the top bar (title and navigation)
    pub const TOP_BAR_PADDING: f32 = 15.0;
}

/// Search window layout constants
pub mod search {
    /// Fixed width of the dedicated search window
    pub const WINDOW_WIDTH: f32 = 620.0;

    /// Fixed height of the dedicated search window
    pub const WINDOW_HEIGHT: f32 = 440.0;

    /// Vertical offset below the main header when spawning the window
    pub const WINDOW_VERTICAL_OFFSET: f32 = 8.0;

    const WINDOW_VERTICAL_PADDING: f32 = 16.0 * 2.0; // Container padding (top + bottom)
    const SECTION_SPACING: f32 = 16.0; // Spacing between header/input/results

    const HEADER_ICON_SIZE: f32 = 36.0;
    const HEADER_VERTICAL_PADDING: f32 = 12.0 * 2.0;

    /// Total rendered height of the header block
    pub const HEADER_HEIGHT: f32 = HEADER_ICON_SIZE + HEADER_VERTICAL_PADDING;

    const INPUT_BUTTON_HEIGHT: f32 = 46.0;
    const INPUT_PANEL_VERTICAL_PADDING: f32 = 12.0 * 2.0;

    /// Total rendered height of the input/search controls block
    pub const INPUT_PANEL_HEIGHT: f32 =
        INPUT_BUTTON_HEIGHT + INPUT_PANEL_VERTICAL_PADDING;

    /// Effective viewport height available for the scrollable results area
    pub const RESULTS_VIEWPORT_HEIGHT: f32 = WINDOW_HEIGHT
        - WINDOW_VERTICAL_PADDING
        - HEADER_HEIGHT
        - INPUT_PANEL_HEIGHT
        - SECTION_SPACING * 2.0;

    const RESULT_ICON_SIZE: f32 = 48.0;
    const RESULT_VERTICAL_PADDING: f32 = 14.0 * 2.0;

    /// Height of a single result row (without inter-row spacing)
    pub const RESULT_ROW_HEIGHT: f32 =
        RESULT_ICON_SIZE + RESULT_VERTICAL_PADDING;

    /// Spacing applied between result rows within the column layout
    pub const RESULT_ROW_SPACING: f32 = 6.0;

    /// Height of the footer summary row that shows total results
    pub const RESULTS_FOOTER_HEIGHT: f32 = 29.0;

    /// Half viewport step used for snap scrolling increments
    pub const RESULTS_HALF_STEP: f32 = RESULTS_VIEWPORT_HEIGHT / 2.0;
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

    /// Full coverage factor - display the complete calculated backdrop height
    pub const COVERAGE_FACTOR: f32 = 1.0;

    /// Margin from backdrop bottom edge for button positioning (pixels)
    pub const BUTTON_BOTTOM_MARGIN: f32 = 20.0;
}

/// Detail view layout constants
pub mod detail {
    /// Width of the primary poster in detail views
    pub const POSTER_WIDTH: f32 = 300.0;
    /// Height of the primary poster in detail views
    pub const POSTER_HEIGHT: f32 = 450.0;
    /// Vertical padding below the poster for shadow/border spacing
    pub const POSTER_PADDING: f32 = 10.0;
    /// Additional horizontal spacing reserved for metadata columns
    pub const POSTER_METADATA_GAP: f32 = 37.5;
    /// Vertical offset applied to metadata blocks relative to poster bottom
    pub const METADATA_OFFSET: f32 = 150.0;
}

/// Carousel layout constants (e.g., cast carousels)
pub mod carousel {
    /// Spacing between items in horizontal carousels (pixels)
    pub const ITEM_SPACING: f32 = 15.0;

    /// Total horizontal padding applied around carousel content (left + right)
    /// Matches container padding([5, 10]) => 10 per side = 20 total
    pub const HORIZONTAL_PADDING_TOTAL: f32 = 20.0;
}

/// Header constants
pub mod header {
    /// Fixed height of the header in pixels
    pub const HEIGHT: f32 = 50.0;
}

/// Helper functions for layout calculations
pub mod calculations {
    use super::{animation, grid, poster};

    /// Runtime-computed layout values for current scale
    ///
    /// This struct pre-computes all scale-dependent layout values to avoid
    /// repeated calculations during rendering. It's the single source of truth
    /// for scaled dimensions used by virtual grids, carousels, and cards.
    ///
    /// ## Usage
    ///
    /// Create once when scale changes and store in UI state:
    /// ```rust,ignore
    /// state.domains.ui.state.scaled_layout = ScaledLayout::new(scale);
    /// ```
    ///
    /// Access during rendering:
    /// ```rust,ignore
    /// let width = state.domains.ui.state.scaled_layout.poster_width;
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct ScaledLayout {
        /// The scale factor used to compute these values
        pub scale: f32,
        /// Scaled poster width (BASE_WIDTH * scale)
        pub poster_width: f32,
        /// Scaled poster height (BASE_HEIGHT * scale)
        pub poster_height: f32,
        /// Scaled text area height below poster
        pub text_area_height: f32,
        /// Total card height including text area (poster + text + gap)
        pub poster_total_height: f32,
        /// Row height for virtual grid (includes animation padding)
        pub row_height: f32,
        /// Container width including animation padding
        pub container_width: f32,
        /// Container height including animation padding
        pub container_height: f32,
        /// Horizontal animation padding (for hover effects)
        pub h_animation_padding: f32,
        /// Vertical animation padding (for hover effects)
        pub v_animation_padding: f32,
        /// Scaled corner radius for poster cards
        pub corner_radius: f32,
    }

    impl ScaledLayout {
        /// Create a new ScaledLayout for the given scale factor
        pub fn new(scale: f32) -> Self {
            let poster_width = poster::BASE_WIDTH * scale;
            let poster_height = poster::BASE_HEIGHT * scale;
            let text_area_height = poster::TEXT_AREA_HEIGHT * scale;
            let corner_radius = poster::CORNER_RADIUS * scale;

            // Match the TOTAL_CARD_HEIGHT calculation: BASE_HEIGHT + TEXT_AREA_HEIGHT + 5.0
            let poster_total_height =
                poster_height + text_area_height + 5.0 * scale;

            // Calculate animation padding for hover effects
            let h_padding =
                animation::calculate_horizontal_padding(poster_width);
            let v_padding =
                animation::calculate_vertical_padding(poster_height);

            // Container dimensions include animation padding on both sides
            let container_width = poster_width + h_padding * 2.0;
            let container_height = poster_height + v_padding * 2.0;

            // Row height matches virtual_grid::ROW_HEIGHT formula but scaled
            let row_height = poster_total_height + 2.0 * v_padding;

            Self {
                scale,
                poster_width,
                poster_height,
                text_area_height,
                poster_total_height,
                row_height,
                container_width,
                container_height,
                h_animation_padding: h_padding,
                v_animation_padding: v_padding,
                corner_radius,
            }
        }

        /// Create layout at default scale (1.0)
        pub fn default_scale() -> Self {
            Self::new(1.0)
        }

        /// Get effective spacing for grid (currently not scaled)
        pub fn grid_spacing(&self) -> f32 {
            grid::EFFECTIVE_SPACING
        }

        /// Get minimum viewport padding (currently not scaled)
        pub fn min_viewport_padding(&self) -> f32 {
            grid::MIN_VIEWPORT_PADDING
        }

        /// Calculate columns for a given viewport width using this layout's scale
        pub fn calculate_columns(&self, viewport_width: f32) -> usize {
            calculate_columns(viewport_width, self.scale)
        }

        /// Calculate grid padding for centering
        pub fn calculate_grid_padding(
            &self,
            viewport_width: f32,
            columns: usize,
        ) -> f32 {
            calculate_grid_padding(viewport_width, columns, self.scale)
        }
    }

    impl Default for ScaledLayout {
        fn default() -> Self {
            Self::default_scale()
        }
    }

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
    pub fn calculate_grid_padding(
        viewport_width: f32,
        columns: usize,
        scale: f32,
    ) -> f32 {
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
        let h_padding =
            super::animation::calculate_horizontal_padding(base_width);
        let v_padding =
            super::animation::calculate_vertical_padding(base_height);

        // Container must fit the base poster plus animation padding
        (base_width + h_padding * 2.0, base_height + v_padding * 2.0)
    }
}
