use crate::{theme, Message};
use iced::{
    widget::scrollable::Id as ScrollableId,
    widget::{button, column, container, row, scrollable, text, Space},
    Element, Length,
};
use lucide_icons::Icon;

/// State for a media carousel
#[derive(Debug, Clone)]
pub struct CarouselState {
    /// Scrollable widget ID for programmatic scrolling
    pub scrollable_id: ScrollableId,
    /// Current scroll position in pixels
    pub scroll_position: f32,
    /// Maximum scroll position (content width - viewport width)
    pub max_scroll: f32,
    /// Number of items to show at once (for button scrolling)
    pub items_per_page: usize,
    /// Total number of items
    pub total_items: usize,
}

impl CarouselState {
    pub fn new(total_items: usize) -> Self {
        Self {
            scrollable_id: ScrollableId::unique(),
            scroll_position: 0.0,
            max_scroll: 0.0,
            items_per_page: 5, // Default to 5 items visible
            total_items,
        }
    }

    /// Update items per page based on available width
    /// Each item is 200px wide with 15px spacing
    pub fn update_items_per_page(&mut self, available_width: f32) {
        const ITEM_WIDTH: f32 = 200.0;
        const ITEM_SPACING: f32 = 15.0;
        const BUTTON_SPACE: f32 = 100.0; // Space for nav buttons
        const MIN_ITEMS: usize = 2;
        const MAX_ITEMS: usize = 8;

        let usable_width = available_width - BUTTON_SPACE;
        let items_that_fit = ((usable_width + ITEM_SPACING) / (ITEM_WIDTH + ITEM_SPACING)) as usize;

        self.items_per_page = items_that_fit.clamp(MIN_ITEMS, MAX_ITEMS);
    }

    pub fn can_go_left(&self) -> bool {
        self.scroll_position > 0.0
    }

    pub fn can_go_right(&self) -> bool {
        self.scroll_position < self.max_scroll
    }

    pub fn go_left(&mut self) {
        if self.can_go_left() {
            // Scroll by roughly one page worth (items * (width + spacing))
            const ITEM_WIDTH: f32 = 200.0;
            const ITEM_SPACING: f32 = 15.0;
            let scroll_amount = self.items_per_page as f32 * (ITEM_WIDTH + ITEM_SPACING);
            self.scroll_position = (self.scroll_position - scroll_amount).max(0.0);
        }
    }

    pub fn go_right(&mut self) {
        if self.can_go_right() {
            // Scroll by roughly one page worth (items * (width + spacing))
            const ITEM_WIDTH: f32 = 200.0;
            const ITEM_SPACING: f32 = 15.0;
            let scroll_amount = self.items_per_page as f32 * (ITEM_WIDTH + ITEM_SPACING);
            // Note: We'll clamp to max in the scrolled handler when we know content width
            self.scroll_position = self.scroll_position + scroll_amount;
        }
    }

    pub fn get_scroll_offset(&self) -> scrollable::AbsoluteOffset {
        scrollable::AbsoluteOffset {
            x: self.scroll_position,
            y: 0.0,
        }
    }

    /// Update the total number of items
    pub fn set_total_items(&mut self, total: usize) {
        self.total_items = total;
    }
}

/// Message for carousel navigation
#[derive(Debug, Clone)]
pub enum CarouselMessage {
    Previous(String),                       // Section ID
    Next(String),                           // Section ID
    Scrolled(String, scrollable::Viewport), // Section ID, viewport info
}

/// Create a Netflix-style media carousel
pub fn media_carousel<'a>(
    section_id: String,
    title: &'a str,
    all_items: Vec<Element<'a, Message>>,
    state: &CarouselState,
) -> Element<'a, Message> {
    if all_items.is_empty() {
        return container(Space::with_height(0)).into();
    }

    // Create navigation buttons
    let left_button = if state.can_go_left() {
        button(
            text(icon_char(Icon::ChevronLeft))
                .font(lucide_font())
                .size(20)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
        )
        .on_press(Message::CarouselNavigation(CarouselMessage::Previous(
            section_id.clone(),
        )))
        .padding(8)
        .style(theme::Button::Secondary.style())
    } else {
        button(
            text(icon_char(Icon::ChevronLeft))
                .font(lucide_font())
                .size(20)
                .color(theme::MediaServerTheme::TEXT_DIMMED),
        )
        .padding(8)
        .style(theme::Button::Secondary.style())
    };

    let right_button = if state.can_go_right() {
        button(
            text(icon_char(Icon::ChevronRight))
                .font(lucide_font())
                .size(20)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
        )
        .on_press(Message::CarouselNavigation(CarouselMessage::Next(
            section_id.clone(),
        )))
        .padding(8)
        .style(theme::Button::Secondary.style())
    } else {
        button(
            text(icon_char(Icon::ChevronRight))
                .font(lucide_font())
                .size(20)
                .color(theme::MediaServerTheme::TEXT_DIMMED),
        )
        .padding(8)
        .style(theme::Button::Secondary.style())
    };

    // Create the item row with all items
    let mut item_row = row![].spacing(15);

    // Add all items - no wrapping needed since scrollable handles sizing
    for item in all_items {
        item_row = item_row.push(item);
    }

    // Create horizontal scrollable for items
    let items_scrollable = scrollable(row![
        Space::with_width(20), // Left padding to start from container edge
        item_row
    ])
    .id(state.scrollable_id.clone())
    .direction(scrollable::Direction::Horizontal(
        scrollable::Scrollbar::new()
            .width(0) // Hide scrollbar
            .scroller_width(0),
    ))
    .on_scroll(move |viewport| {
        Message::CarouselNavigation(CarouselMessage::Scrolled(section_id.clone(), viewport))
    })
    .width(Length::Fill)
    .height(Length::Fixed(370.0));

    // Build layout with carousel extending to edges
    column![
        // Header with title and navigation buttons (with padding)
        container(
            container(
                row![
                    text(title)
                        .size(24)
                        .color(theme::MediaServerTheme::TEXT_PRIMARY),
                    Space::with_width(Length::Fill),
                    // Navigation buttons
                    row![left_button, Space::with_width(5), right_button,]
                        .align_y(iced::Alignment::Center),
                ]
                .align_y(iced::Alignment::Center)
                .width(Length::Fill)
            )
            .padding([0, 20]) // Horizontal padding
        )
        .padding([20, 0]), // Vertical padding
        Space::with_height(15),
        // Scrollable carousel content extending to edges
        items_scrollable,
    ]
    .width(Length::Fill)
    .into()
}

// Helper to get lucide font
fn lucide_font() -> iced::Font {
    iced::Font::with_name("lucide")
}

// Helper to get icon character
fn icon_char(icon: Icon) -> String {
    icon.unicode().to_string()
}
