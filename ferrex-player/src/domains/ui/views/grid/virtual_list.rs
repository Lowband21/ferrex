use iced::{
    Element, Length,
    widget::{Space, column, container, scrollable},
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

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
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
        // Profile the visibility calculation
        #[cfg(any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ))]
        profiling::scope!("UI::VirtualList::CalculateVisible");

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

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn virtual_list<'a, Message: 'a>(
    state: &VirtualListState,
    items: impl FnMut(usize) -> Element<'a, Message>,
    on_scroll: impl Fn(scrollable::Viewport) -> Message + 'a,
) -> Element<'a, Message> {
    // Profile the virtual list rendering
    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    profiling::scope!(crate::infrastructure::profiling_scopes::scopes::VIRTUAL_LIST_RENDER);

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
