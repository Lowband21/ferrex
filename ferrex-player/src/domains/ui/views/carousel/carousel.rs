use crate::domains::ui::messages::Message;
use crate::domains::ui::theme;
use crate::domains::ui::views::carousel::state::CarouselState;
use iced::{
    Element, Length,
    widget::{Space, button, column, container, row, scrollable, text},
};
use lucide_icons::Icon;

/// Message for carousel navigation
#[derive(Debug, Clone)]
pub enum CarouselMessage {
    Previous(String),                       // Section ID
    Next(String),                           // Section ID
    Scrolled(String, scrollable::Viewport), // Section ID, viewport info
}

/// Create a Netflix-style media carousel
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn media_carousel<'a>(
    section_id: String,
    title: &'a str,
    all_items: Vec<Element<'a, Message>>,
    state: &CarouselState,
) -> Element<'a, Message> {
    if all_items.is_empty() {
        return container(Space::new().height(0)).into();
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

    // Create the item row with only visible items for virtualization
    let mut item_row = row![].spacing(state.item_spacing);

    // Get visible range
    let visible_range = state.get_visible_range();

    // Add spacer for items before visible range
    if visible_range.start > 0 {
        let spacer_width = visible_range.start as f32
            * (state.item_width + state.item_spacing);
        item_row =
            item_row.push(Space::new().width(Length::Fixed(spacer_width)));
    }

    // Add only visible items
    for (index, item) in all_items.into_iter().enumerate() {
        if visible_range.contains(&index) {
            item_row = item_row.push(item);
        }
    }

    // Add spacer for items after visible range
    if visible_range.end < state.total_items {
        let remaining_items = state.total_items - visible_range.end;
        let spacer_width =
            remaining_items as f32 * (state.item_width + state.item_spacing);
        item_row =
            item_row.push(Space::new().width(Length::Fixed(spacer_width)));
    }

    // Create horizontal scrollable for items
    let items_scrollable = scrollable(row![
        Space::new().width(20), // Left padding to start from container edge
        item_row
    ])
    .id(state.scrollable_id.clone())
    .direction(scrollable::Direction::Horizontal(
        scrollable::Scrollbar::new()
            .width(0) // Hide scrollbar
            .scroller_width(0),
    ))
    .on_scroll(move |viewport| {
        Message::CarouselNavigation(CarouselMessage::Scrolled(
            section_id.clone(),
            viewport,
        ))
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
                    Space::new().width(Length::Fill),
                    // Navigation buttons
                    row![left_button, Space::new().width(5), right_button,]
                        .align_y(iced::Alignment::Center),
                ]
                .align_y(iced::Alignment::Center)
                .width(Length::Fill)
            )
            .padding([0, 20]) // Horizontal padding
        )
        .padding([20, 0]), // Vertical padding
        Space::new().height(15),
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
