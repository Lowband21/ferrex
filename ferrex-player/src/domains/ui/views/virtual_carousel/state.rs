//! VirtualCarouselState: viewport-aware horizontal windowing state

use iced::widget::{Id as ScrollableId, scrollable};
use std::ops::Range;

use super::types::{CarouselConfig, WrapMode};

#[derive(Debug, Clone)]
pub struct VirtualCarouselState {
    // Content
    pub total_items: usize,

    // Viewport + layout
    pub viewport_width: f32,
    pub item_width: f32,
    pub item_spacing: f32,
    pub items_per_page: usize,

    // Windowing
    pub overscan_before: usize,
    pub overscan_after: usize,
    pub visible_range: Range<usize>,

    // Scrolling
    /// Index-based position (single source of truth). Represents the start index
    /// of the left-aligned item under the viewport, except at the right-aligned end
    /// where it still maps to `max_scroll`.
    pub index_position: f32,
    /// Stable reference index for navigation decisions (page/step base).
    /// This is updated only on discrete commits (e.g., when a snap finishes),
    /// and is not modified during kinetic holds so that subsequent motions
    /// base off a clean aligned index.
    pub reference_index: f32,
    pub scroll_x: f32,
    pub max_scroll: f32,
    pub scrollable_id: ScrollableId,

    // Behavior
    pub wrap_mode: WrapMode,
}

impl VirtualCarouselState {
    /// Create a new VirtualCarouselState with defaults from a config preset.
    ///
    /// The `scale` parameter is used to scale card dimensions when `config.card_size`
    /// is provided. Pass `1.0` for unscaled, or use the effective scale from
    /// `ScalingContext` for user-preference-aware scaling.
    pub fn new(
        total_items: usize,
        viewport_width: f32,
        config: CarouselConfig,
        scale: f32,
    ) -> Self {
        let computed_item_width = {
            if let Some(card_size) = config.card_size {
                let (w, _h) = card_size.scaled_dimensions(scale);
                if config.include_animation_padding {
                    let pad = crate::infra::constants::animation::calculate_horizontal_padding(w);
                    w + 2.0 * pad
                } else {
                    w
                }
            } else {
                // Scale the explicit item_width as well
                config.item_width * scale
            }
        };
        let mut s = Self {
            total_items,
            viewport_width,
            item_width: computed_item_width,
            item_spacing: config.item_spacing,
            items_per_page: 1, // computed below
            overscan_before: config.overscan_items_before,
            overscan_after: config.overscan_items_after,
            visible_range: 0..0,
            index_position: 0.0,
            reference_index: 0.0,
            scroll_x: 0.0,
            max_scroll: 0.0,
            scrollable_id: ScrollableId::unique(),
            wrap_mode: config.wrap_mode,
        };
        s.recompute_metrics();
        s
    }

    /// Create a new VirtualCarouselState with default scale (1.0).
    /// Convenience method for backwards compatibility.
    pub fn new_unscaled(
        total_items: usize,
        viewport_width: f32,
        config: CarouselConfig,
    ) -> Self {
        Self::new(total_items, viewport_width, config, 1.0)
    }

    /// Set absolute horizontal scroll offset and update index + visible range.
    /// This should be used when the scrollable reports viewport changes, or when
    /// the animator drives pixel-based tweening.
    pub fn set_scroll_x(&mut self, x: f32) {
        let clamped = x.clamp(0.0, self.max_scroll);
        self.scroll_x = clamped;
        self.index_position = self.scroll_to_index(clamped);
        self.recompute_visible_range();
    }

    /// Set the index-based position and derive scroll_x for rendering.
    pub fn set_index_position(&mut self, index: f32) {
        let max_i = self.max_start_index() as f32;
        let clamped = index.clamp(0.0, max_i);
        self.index_position = clamped;
        self.scroll_x = self.index_to_scroll(clamped);
        self.recompute_visible_range();
    }

    /// Commit a new stable reference index without changing the current scroll.
    /// Use when a snap completes or when the app wants to re-base navigation.
    pub fn set_reference_index(&mut self, index: f32) {
        let max_i = self.max_start_index() as f32;
        self.reference_index = index.clamp(0.0, max_i);
    }

    /// Update total items and recompute visible ranges where needed.
    pub fn set_total_items(&mut self, total: usize) {
        self.total_items = total;
        self.recompute_visible_range();
        self.recompute_max_scroll();
        // Keep index as the source of truth after metrics change
        self.scroll_x = self.index_to_scroll(self.index_position);
    }

    /// Update viewport width; recompute items/page and max scroll.
    pub fn update_dimensions(&mut self, viewport_width: f32) {
        self.viewport_width = viewport_width;
        self.recompute_metrics();
    }

    /// Handle scroll viewport reporting (from Iced on_scroll viewport).
    pub fn update_scroll(&mut self, viewport: scrollable::Viewport) {
        let new_x = viewport.absolute_offset().x;
        let vw = viewport.bounds().width;
        self.scroll_x = new_x.clamp(0.0, self.max_scroll);
        self.index_position = self.scroll_to_index(self.scroll_x);
        if (vw - self.viewport_width).abs() > 0.5 {
            self.viewport_width = vw;
            self.recompute_metrics();
        } else {
            self.recompute_visible_range();
        }
    }

    /// Calculate the prefetch window (ahead of visible window).
    pub fn prefetch_range(&self, count: usize) -> Range<usize> {
        let start = self.visible_range.end.min(self.total_items);
        let end = (start + count).min(self.total_items);
        start..end
    }

    /// Additional background range beyond prefetch.
    pub fn background_range(
        &self,
        prefetch_count: usize,
        background_count: usize,
    ) -> Range<usize> {
        let prefetch_end =
            (self.visible_range.end + prefetch_count).min(self.total_items);
        let start = prefetch_end;
        let end = (start + background_count).min(self.total_items);
        start..end
    }

    /// Compute a page-sized absolute offset to the next page (index-aligned).
    /// Page stride is items_per_page - 1 (overlap the last fully visible item),
    /// except when only one item fits, then stride is 1.
    /// Special case: at the end of the list, right-align the final item by
    /// targeting raw `max_scroll` instead of the last aligned boundary.
    pub fn page_right_index_target(&self) -> f32 {
        let i0 = self.reference_index.floor() as usize;
        let page_stride = self.items_per_page.saturating_sub(1).max(1);
        let mut target_i = i0.saturating_add(page_stride);
        let max_i = self.max_start_index();
        if target_i > max_i {
            // Special case: right-align at the end
            return max_i as f32;
        }
        target_i as f32
    }

    /// Compute a page-sized absolute offset to the previous page (index-aligned).
    pub fn page_left_index_target(&self) -> f32 {
        let i0 = self.reference_index.floor() as usize;
        let page_stride = self.items_per_page.saturating_sub(1).max(1);
        let target_i = i0.saturating_sub(page_stride);
        target_i as f32
    }

    /// Compute a one-item step absolute offset to the right (index-aligned).
    /// Special case: stepping beyond the last aligned boundary goes to max_scroll.
    pub fn step_right_index_target(&self) -> f32 {
        let eps = 1e-4;
        let next_i = (self.reference_index + eps).ceil() as usize;
        let max_i = self.max_start_index();
        if next_i > max_i {
            return max_i as f32;
        }
        next_i as f32
    }

    /// Compute a one-item step absolute offset to the left (index-aligned).
    pub fn step_left_index_target(&self) -> f32 {
        let eps = 1e-4;
        // If we're at right-aligned end, first restore left-aligned boundary
        if (self.scroll_x - self.max_scroll).abs() <= eps {
            let max_i = self.max_start_index();
            let aligned = max_i as f32;
            if (self.reference_index - aligned).abs() > eps {
                return aligned;
            }
        }
        // Move to the previous aligned boundary: floor((i - eps))
        let target_i =
            ((self.reference_index - eps).floor() as isize).max(0) as usize;
        target_i as f32
    }

    /// Recompute items/page, visible range, and max scroll from current dimensions.
    fn recompute_metrics(&mut self) {
        let w = self.item_width.max(1.0);
        let s = self.item_spacing.max(0.0);
        let stride = (w + s).max(1.0);
        // Count of fully visible items: floor((viewport + s) / (w + s))
        let raw = ((self.viewport_width + s) / stride).floor() as usize;
        self.items_per_page = raw.max(1);
        self.recompute_visible_range();
        self.recompute_max_scroll();
        // Re-derive scroll position from index after metric changes
        self.scroll_x = self.index_to_scroll(self.index_position);
    }

    fn recompute_visible_range(&mut self) {
        if self.total_items == 0 {
            self.visible_range = 0..0;
            return;
        }
        let stride = self.stride();
        let viewport_end = self.scroll_x + self.viewport_width;
        let first_start_idx = (self.scroll_x / stride).floor() as usize;
        // Items are included if their start < viewport_end (right-partial included)
        let last_start_idx = ((viewport_end - 1e-6) / stride).floor() as usize;
        let start = first_start_idx.saturating_sub(self.overscan_before);
        let end_base = last_start_idx.saturating_add(1);
        let end = (end_base + self.overscan_after).min(self.total_items);
        self.visible_range = start..end;
    }

    fn recompute_max_scroll(&mut self) {
        let content_width = self.content_width();
        let max = if content_width > self.viewport_width {
            content_width - self.viewport_width
        } else {
            0.0
        };
        self.max_scroll = max.max(0.0);
        // Clamp index first, then derive scroll
        let max_i = self.max_start_index() as f32;
        if self.index_position > max_i {
            self.index_position = max_i;
        }
        if self.reference_index > max_i {
            self.reference_index = max_i;
        }
        self.scroll_x = self.index_to_scroll(self.index_position);
    }

    #[inline]
    fn stride(&self) -> f32 {
        (self.item_width + self.item_spacing).max(1.0)
    }

    #[inline]
    fn content_width(&self) -> f32 {
        if self.total_items == 0 {
            return 0.0;
        }
        let w = self.item_width.max(0.0);
        let s = self.item_spacing.max(0.0);
        self.total_items as f32 * w
            + (self.total_items.saturating_sub(1)) as f32 * s
    }

    /// Maximum start index so that a fully aligned item start does not exceed max_scroll.
    #[inline]
    pub fn max_start_index(&self) -> usize {
        let stride = self.stride();
        if stride <= 0.0 {
            return 0;
        }
        (self.max_scroll / stride).floor() as usize
    }

    /// Right-most aligned scroll not exceeding max_scroll.
    #[inline]
    pub fn max_aligned_scroll(&self) -> f32 {
        let stride = self.stride();
        (self.max_scroll / stride).floor() * stride
    }

    /// Map an index position to a scroll offset in pixels. Applies the special-case
    /// right alignment at the end of the list.
    #[inline]
    pub fn index_to_scroll(&self, index: f32) -> f32 {
        let stride = self.stride();
        let max_i = self.max_start_index() as f32;
        if index >= max_i {
            return self.max_scroll;
        }
        (index * stride).clamp(0.0, self.max_scroll)
    }

    /// Inverse mapping from scroll offset to index position. If at the far right
    /// (within epsilon of max_scroll), returns the max start index.
    #[inline]
    pub fn scroll_to_index(&self, scroll: f32) -> f32 {
        let stride = self.stride();
        if self.max_scroll <= 0.0 || stride <= 0.0 {
            return 0.0;
        }
        let eps = 1e-4;
        if (scroll - self.max_scroll).abs() <= eps {
            return self.max_start_index() as f32;
        }
        (scroll / stride).clamp(0.0, self.max_start_index() as f32)
    }
}
