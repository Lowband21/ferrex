use iced::{
    widget::{column, container, scrollable, Space},
    Element, Length,
};
use std::ops::Range;

/// Virtual list state for efficient rendering of large lists
#[derive(Debug, Clone)]
pub struct VirtualListState {
    /// Total number of items
    pub total_items: usize,
    /// Height of each item
    pub item_height: f32,
    /// Current scroll position
    pub scroll_position: f32,
    /// Viewport height
    pub viewport_height: f32,
    /// Number of items to render outside viewport (for smooth scrolling)
    pub overscan: usize,
    /// Currently visible range
    pub visible_range: Range<usize>,
    /// Scrollable ID
    pub scrollable_id: scrollable::Id,
}

impl VirtualListState {
    pub fn new(total_items: usize, item_height: f32) -> Self {
        Self {
            total_items,
            item_height,
            scroll_position: 0.0,
            viewport_height: 800.0, // Default, will be updated
            overscan: 5,            // Render 5 items above and below viewport
            visible_range: 0..0,
            scrollable_id: scrollable::Id::unique(),
        }
    }

    /// Calculate which items should be visible based on scroll position
    pub fn calculate_visible_range(&mut self) -> Range<usize> {
        if self.total_items == 0 {
            self.visible_range = 0..0;
            return self.visible_range.clone();
        }

        // Calculate first and last visible items
        let first_visible = (self.scroll_position / self.item_height).floor() as usize;
        let visible_count = (self.viewport_height / self.item_height).ceil() as usize;
        let last_visible = (first_visible + visible_count).min(self.total_items);

        // Add overscan
        let start = first_visible.saturating_sub(self.overscan);
        let end = (last_visible + self.overscan).min(self.total_items);

        self.visible_range = start..end;
        self.visible_range.clone()
    }

    /// Update scroll position and recalculate visible range
    pub fn update_scroll(&mut self, viewport: scrollable::Viewport) {
        self.scroll_position = viewport.absolute_offset().y;
        self.viewport_height = viewport.bounds().height;
        self.calculate_visible_range();
    }

    /// Get items that need to be pre-loaded (slightly ahead of current position)
    pub fn get_preload_range(&self, preload_ahead: usize) -> Range<usize> {
        let end = (self.visible_range.end + preload_ahead).min(self.total_items);
        self.visible_range.end..end
    }
}

/// Create a virtual list widget
pub fn virtual_list<'a, Message: 'a>(
    state: &VirtualListState,
    items: impl FnMut(usize) -> Element<'a, Message>,
    on_scroll: impl Fn(scrollable::Viewport) -> Message + 'a,
) -> Element<'a, Message> {
    let mut content = column![].spacing(0).width(Length::Fill);

    // Add spacer for items above viewport
    if state.visible_range.start > 0 {
        let spacer_height = state.visible_range.start as f32 * state.item_height;
        content = content.push(Space::with_height(Length::Fixed(spacer_height)));
    }

    // Add visible items
    let mut item_fn = items;
    for index in state.visible_range.clone() {
        content = content.push(
            container(item_fn(index))
                .height(Length::Fixed(state.item_height))
                .width(Length::Fill),
        );
    }

    // Add spacer for items below viewport
    if state.visible_range.end < state.total_items {
        let remaining_items = state.total_items - state.visible_range.end;
        let spacer_height = remaining_items as f32 * state.item_height;
        content = content.push(Space::with_height(Length::Fixed(spacer_height)));
    }

    scrollable(content)
        .id(state.scrollable_id.clone())
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::default(),
        ))
        .on_scroll(on_scroll)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

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
    /// Number of rows to render outside viewport
    pub overscan_rows: usize,
    /// Currently visible item range
    pub visible_range: Range<usize>,
    /// Scrollable ID
    pub scrollable_id: scrollable::Id,
    /// Item width (calculated from viewport width)
    pub item_width: f32,
}

impl VirtualGridState {
    pub fn new(total_items: usize, columns: usize, row_height: f32) -> Self {
        Self {
            total_items,
            columns,
            row_height,
            scroll_position: 0.0,
            viewport_height: 800.0,
            overscan_rows: 2,
            visible_range: 0..0,
            scrollable_id: scrollable::Id::unique(),
            item_width: 200.0, // Default
        }
    }

    /// Update columns based on viewport width
    pub fn update_columns(&mut self, viewport_width: f32) {
        const MIN_ITEM_WIDTH: f32 = 200.0;
        const MAX_ITEM_WIDTH: f32 = 200.0;
        const SPACING: f32 = 30.0;
        const PADDING: f32 = 30.0; // Left and right padding

        let available_width = viewport_width - PADDING;

        // Calculate optimal number of columns
        self.columns = ((available_width + SPACING) / (MIN_ITEM_WIDTH + SPACING)).floor() as usize;
        self.columns = self.columns.max(1);

        // Calculate actual item width
        self.item_width = ((available_width - (self.columns - 1) as f32 * SPACING)
            / self.columns as f32)
            .min(MAX_ITEM_WIDTH);

        // Recalculate visible range with new columns
        self.calculate_visible_range();
    }

    /// Calculate visible item range
    pub fn calculate_visible_range(&mut self) -> Range<usize> {
        if self.total_items == 0 || self.columns == 0 {
            self.visible_range = 0..0;
            return self.visible_range.clone();
        }

        let total_rows = (self.total_items + self.columns - 1) / self.columns;
        let first_visible_row = (self.scroll_position / self.row_height).floor() as usize;
        let visible_rows = (self.viewport_height / self.row_height).ceil() as usize;
        let last_visible_row = (first_visible_row + visible_rows).min(total_rows);

        // Add overscan
        let start_row = first_visible_row.saturating_sub(self.overscan_rows);
        let end_row = (last_visible_row + self.overscan_rows).min(total_rows);

        // Convert to item indices
        let start_item = start_row * self.columns;
        let end_item = (end_row * self.columns).min(self.total_items);

        self.visible_range = start_item..end_item;
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
        let end = (self.visible_range.end + preload_items).min(self.total_items);
        self.visible_range.end..end
    }
}
