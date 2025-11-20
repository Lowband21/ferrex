use iced::widget::scrollable::Id as ScrollableId;

// Carousel state for managing carousel UI
#[derive(Debug, Clone)]
pub struct CarouselState {
    pub current_index: usize,
    pub items_per_page: usize,
    pub total_items: usize,
    // For performance: track visible range
    pub visible_start: usize,
    pub visible_end: usize,
    // Scrollable widget ID for programmatic scrolling
    pub scrollable_id: ScrollableId,
    // Current scroll position in pixels
    pub scroll_position: f32,
    // Maximum scroll position (content width - viewport width)
    pub max_scroll: f32,
    // Item dimensions
    pub item_width: f32,
    pub item_spacing: f32,
}

impl Default for CarouselState {
    fn default() -> Self {
        Self {
            current_index: 0,
            items_per_page: 5,
            total_items: 0,
            visible_start: 0,
            visible_end: 5,
            scrollable_id: ScrollableId::unique(),
            scroll_position: 0.0,
            max_scroll: 0.0,
            item_width: 200.0, // Default for movie/TV posters
            item_spacing: 15.0,
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
impl CarouselState {
    /// Create a new carousel state with the given total items
    pub fn new(total_items: usize) -> Self {
        let items_per_page = 5; // Default, will be updated based on width
        Self {
            current_index: 0,
            items_per_page,
            total_items,
            visible_start: 0,
            visible_end: items_per_page.min(total_items),
            scrollable_id: ScrollableId::unique(),
            scroll_position: 0.0,
            max_scroll: 0.0,
            item_width: 200.0, // Default
            item_spacing: 15.0,
        }
    }

    /// Create a new carousel state with custom item dimensions
    pub fn new_with_dimensions(total_items: usize, item_width: f32, item_spacing: f32) -> Self {
        let items_per_page = 5; // Default, will be updated based on width
        Self {
            current_index: 0,
            items_per_page,
            total_items,
            visible_start: 0,
            visible_end: items_per_page.min(total_items),
            scrollable_id: ScrollableId::unique(),
            scroll_position: 0.0,
            max_scroll: 0.0,
            item_width,
            item_spacing,
        }
    }

    /// Set the total number of items
    pub fn set_total_items(&mut self, total: usize) {
        self.total_items = total;
        self.update_visible_range();
    }

    /// Update items per page based on available width
    pub fn update_items_per_page(&mut self, available_width: f32) {
        const MIN_ITEMS: usize = 3;
        const MAX_ITEMS: usize = 10;

        let item_total_width = self.item_width + self.item_spacing;
        let calculated_items = (available_width / item_total_width).floor() as usize;
        self.items_per_page = calculated_items.clamp(MIN_ITEMS, MAX_ITEMS);
        self.update_visible_range();
    }

    /// Update the visible range based on current index and items per page
    fn update_visible_range(&mut self) {
        // Add some buffer for smooth scrolling (preload adjacent items)
        const BUFFER: usize = 2;

        let start = self.current_index.saturating_sub(BUFFER);
        let end = (self.current_index + self.items_per_page + BUFFER).min(self.total_items);

        self.visible_start = start;
        self.visible_end = end;

        log::debug!(
            "Carousel visible range updated: {}..{} (current_index={}, items_per_page={}, total={})",
            start,
            end,
            self.current_index,
            self.items_per_page,
            self.total_items
        );
    }

    /// Get the items that should be visible (for performance optimization)
    pub fn get_visible_range(&self) -> std::ops::Range<usize> {
        self.visible_start..self.visible_end
    }

    /// Navigate to next page
    pub fn next_page(&mut self) {
        if self.current_index + self.items_per_page < self.total_items {
            self.current_index += self.items_per_page;
            self.update_visible_range();
        }
    }

    /// Navigate to previous page
    pub fn previous_page(&mut self) {
        if self.current_index >= self.items_per_page {
            self.current_index -= self.items_per_page;
        } else {
            self.current_index = 0;
        }
        self.update_visible_range();
    }

    /// Check if can navigate to next page
    pub fn can_go_next(&self) -> bool {
        self.current_index + self.items_per_page < self.total_items
    }

    /// Check if can navigate to previous page
    pub fn can_go_previous(&self) -> bool {
        self.current_index > 0
    }

    // Compatibility methods from carousel.rs
    pub fn can_go_left(&self) -> bool {
        self.scroll_position > 0.0
    }

    pub fn can_go_right(&self) -> bool {
        self.scroll_position < self.max_scroll
    }

    pub fn go_left(&mut self) {
        if self.can_go_left() {
            // Scroll by roughly one page worth (items * (width + spacing))
            let scroll_amount = self.items_per_page as f32 * (self.item_width + self.item_spacing);
            self.scroll_position = (self.scroll_position - scroll_amount).max(0.0);
            self.update_visible_range_from_scroll();
        }
    }

    pub fn go_right(&mut self) {
        if self.can_go_right() {
            // Scroll by roughly one page worth (items * (width + spacing))
            let scroll_amount = self.items_per_page as f32 * (self.item_width + self.item_spacing);
            self.scroll_position = self.scroll_position + scroll_amount;
            self.update_visible_range_from_scroll();
        }
    }

    pub fn get_scroll_offset(&self) -> iced::widget::scrollable::AbsoluteOffset {
        iced::widget::scrollable::AbsoluteOffset {
            x: self.scroll_position,
            y: 0.0,
        }
    }

    /// Update visible range based on scroll position
    pub fn update_visible_range_from_scroll(&mut self) {
        const BUFFER: usize = 2;

        // Calculate which item is at the current scroll position
        let item_with_spacing = self.item_width + self.item_spacing;
        let first_visible = (self.scroll_position / item_with_spacing).floor() as usize;

        // Update current index
        self.current_index = first_visible;

        // Update visible range with buffer
        self.visible_start = first_visible.saturating_sub(BUFFER);
        self.visible_end = (first_visible + self.items_per_page + BUFFER).min(self.total_items);
    }
}
