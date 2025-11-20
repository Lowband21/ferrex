//! Windowed carousel implementation for performance optimization

use crate::domains::ui::messages::Message;
use crate::domains::ui::theme;
use iced::{
    widget::{button, column, container, row, scrollable, text, Space},
    Element, Length,
};
use lucide_icons::Icon;

use super::{CarouselMessage, CarouselState};

/// Create a windowed media carousel that only renders visible items
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn windowed_media_carousel<'a, F>(
    section_id: String,
    title: &'a str,
    total_items: usize,
    state: &CarouselState,
    create_card: F,
) -> Element<'a, Message>
where
    F: Fn(usize) -> Option<Element<'a, Message>>,
{
    if total_items == 0 {
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

    // Create the item row with windowing
    let mut item_row = row![].spacing(15);

    // Add spacer for items before visible range
    if state.visible_start > 0 {
        const ITEM_WIDTH: f32 = 200.0;
        const ITEM_SPACING: f32 = 15.0;
        let spacer_width = state.visible_start as f32 * (ITEM_WIDTH + ITEM_SPACING);
        item_row = item_row.push(Space::with_width(Length::Fixed(spacer_width)));
    }

    // Only create cards for visible items
    for idx in state.visible_start..state.visible_end {
        if let Some(card) = create_card(idx) {
            item_row = item_row.push(card);
        } else {
            // Add placeholder for missing items
            item_row = item_row
                .push(container(Space::new(200.0, 370.0)).style(theme::Container::Default.style()));
        }
    }

    // Add spacer for items after visible range
    if state.visible_end < total_items {
        const ITEM_WIDTH: f32 = 200.0;
        const ITEM_SPACING: f32 = 15.0;
        let remaining_items = total_items - state.visible_end;
        let spacer_width = remaining_items as f32 * (ITEM_WIDTH + ITEM_SPACING);
        item_row = item_row.push(Space::with_width(Length::Fixed(spacer_width)));
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
