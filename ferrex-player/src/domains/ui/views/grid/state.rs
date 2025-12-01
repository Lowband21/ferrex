use iced::widget::{Id, scrollable};

use crate::infra::constants::layout::calculations::ScaledLayout;
use std::ops::Range;

/// Grid-based virtual list for media cards
#[derive(Debug, Clone)]
pub struct VirtualGridState {
    /// Total number of items
    pub total_items: usize,
    /// Number of columns
    pub columns: usize,
    /// Height of each row
    pub row_height: f32,
    /// Current scroll position
    pub scroll_position: f32,
    /// Viewport height
    pub viewport_height: f32,
    /// Viewport width
    pub viewport_width: f32,
    /// Number of rows to render outside viewport (above)
    pub overscan_rows_above: usize,
    /// Number of rows to render outside viewport (below)
    pub overscan_rows_below: usize,
    /// Currently visible item range
    pub visible_range: Range<usize>,
    /// Scrollable ID
    pub scrollable_id: Id,
    /// Item width (calculated from viewport width)
    pub item_width: f32,
    /// Force refresh flag (for resize events)
    pub needs_refresh: bool,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl VirtualGridState {
    pub fn new(total_items: usize, columns: usize, row_height: f32) -> Self {
        Self::with_id(total_items, columns, row_height, Id::unique())
    }

    /// Create a new VirtualGridState with a specific scrollable ID
    pub fn with_id(
        total_items: usize,
        columns: usize,
        row_height: f32,
        scrollable_id: Id,
    ) -> Self {
        let mut grid = Self {
            total_items,
            columns,
            row_height,
            scroll_position: 0.0,
            viewport_height: 800.0,
            viewport_width: 1200.0, // Default
            overscan_rows_above:
                crate::infra::constants::virtual_grid::PREFETCH_ROWS_ABOVE,
            overscan_rows_below:
                crate::infra::constants::virtual_grid::PREFETCH_ROWS_BELOW,
            visible_range: 0..0,
            scrollable_id,
            item_width: 200.0, // Default
            needs_refresh: false,
        };

        // Calculate initial visible range
        grid.calculate_visible_range();

        grid
    }

    /// Update columns based on viewport width
    pub fn update_columns(&mut self, viewport_width: f32) {
        use crate::infra::constants::{calculations, poster, scale_presets};

        // Calculate columns using centralized logic
        let scale = scale_presets::DEFAULT_SCALE;
        self.columns = calculations::calculate_columns(viewport_width, scale);

        // Keep item width based on scale
        self.item_width = poster::BASE_WIDTH * scale;

        // DO NOT call calculate_visible_range() here - viewport_height may not be updated yet
        // The scrollable widget will trigger this via update_scroll()
    }

    /// Calculate visible item range
    pub fn calculate_visible_range(&mut self) -> Range<usize> {
        if self.total_items == 0
            || self.columns == 0
            || self.viewport_height <= 0.0
        {
            log::debug!(
                "Empty visible range: items={}, cols={}, viewport_height={}",
                self.total_items,
                self.columns,
                self.viewport_height
            );
            self.visible_range = 0..0;
            return self.visible_range.clone();
        }

        let total_rows = self.total_items.div_ceil(self.columns);

        let mut first_visible_row =
            (self.scroll_position / self.row_height).floor() as usize;
        if first_visible_row >= total_rows {
            first_visible_row = total_rows.saturating_sub(1);
            let content_height = total_rows as f32 * self.row_height;
            let max_scroll = if content_height > self.viewport_height {
                content_height - self.viewport_height
            } else {
                0.0
            };
            self.scroll_position = self.scroll_position.min(max_scroll);
        }

        let visible_rows =
            (self.viewport_height / self.row_height).ceil() as usize;
        let mut last_visible_row =
            (first_visible_row + visible_rows).min(total_rows);
        if last_visible_row <= first_visible_row {
            last_visible_row = (first_visible_row + 1).min(total_rows);
        }

        // Add overscan (configurable prefetch zone)
        let start_row = first_visible_row
            .saturating_sub(self.overscan_rows_above)
            .min(total_rows);
        let end_row =
            (last_visible_row + self.overscan_rows_below).min(total_rows);

        // Convert to item indices
        let start_item = start_row * self.columns;
        let end_item = (end_row * self.columns).min(self.total_items);

        self.visible_range = start_item..end_item;

        /*
        log::debug!(
            "Visible range calculated: {}..{} (rows {}-{}, viewport_height={})",
            start_item,
            end_item,
            start_row,
            end_row,
            self.viewport_height
        ); */

        self.visible_range.clone()
    }

    /// Update scroll position
    pub fn update_scroll(&mut self, viewport: scrollable::Viewport) {
        self.scroll_position = viewport.absolute_offset().y;
        self.viewport_height = viewport.bounds().height;
        self.calculate_visible_range();
    }

    /// Get items to preload
    pub fn get_preload_range(&self, preload_rows: usize) -> Range<usize> {
        let preload_items = preload_rows * self.columns;
        let end =
            (self.visible_range.end + preload_items).min(self.total_items);
        self.visible_range.end..end
    }

    /// Get items that fall into the trailing background window beyond the preload range.
    pub fn get_background_range(
        &self,
        preload_rows: usize,
        background_rows: usize,
    ) -> Range<usize> {
        let preload_items = preload_rows * self.columns;
        let background_items = background_rows * self.columns;
        let preload_end =
            (self.visible_range.end + preload_items).min(self.total_items);
        let background_end =
            (preload_end + background_items).min(self.total_items);
        preload_end..background_end
    }

    /// Update columns on window resize
    pub fn resize(&mut self, width: f32) {
        log::debug!("Resize: updating columns for width {}", width);

        use crate::infra::constants::{calculations, scale_presets};

        let scale = scale_presets::DEFAULT_SCALE;
        let old_columns = self.columns;
        self.columns = calculations::calculate_columns(width, scale);

        // If columns changed, log it but don't recalculate visible range yet
        // The scrollable widget needs to report its viewport dimensions first
        if old_columns != self.columns {
            log::debug!(
                "Columns changed from {} to {}, visible range will be updated when scrollable reports viewport",
                old_columns,
                self.columns
            );
            // DO NOT call calculate_visible_range() here - viewport_height may be stale
        }
    }

    /// Update grid dimensions based on a new scale factor
    ///
    /// This method should be called when the UI scale changes (user preference
    /// or system DPI). It updates row height and item width, then recalculates
    /// columns based on the current viewport width.
    pub fn update_for_scale(&mut self, scaled_layout: &ScaledLayout) {
        let old_row_height = self.row_height;
        let old_item_width = self.item_width;
        let old_columns = self.columns;

        // Update dimensions from scaled layout
        self.row_height = scaled_layout.row_height;
        self.item_width = scaled_layout.poster_width;

        // Recalculate columns for current viewport
        self.columns = scaled_layout.calculate_columns(self.viewport_width);

        log::info!(
            "Grid scale updated: row_height {} -> {}, item_width {} -> {}, columns {} -> {}",
            old_row_height,
            self.row_height,
            old_item_width,
            self.item_width,
            old_columns,
            self.columns
        );

        // Recalculate visible range with new dimensions
        self.calculate_visible_range();
        self.needs_refresh = true;
    }

    /// Update columns on window resize using a ScaledLayout
    pub fn resize_with_scale(
        &mut self,
        width: f32,
        scaled_layout: &ScaledLayout,
    ) {
        log::debug!("Resize with scale: updating columns for width {}", width);

        let old_columns = self.columns;
        self.columns = scaled_layout.calculate_columns(width);
        self.item_width = scaled_layout.poster_width;
        self.row_height = scaled_layout.row_height;

        if old_columns != self.columns {
            log::debug!(
                "Columns changed from {} to {}, visible range will be updated when scrollable reports viewport",
                old_columns,
                self.columns
            );
        }
    }
}
