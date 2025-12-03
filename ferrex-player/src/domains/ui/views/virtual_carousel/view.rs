//! View builder for the virtual carousel (scaffold)
//!
//! This returns a lightweight placeholder layout for now. The full windowed
//! rendering, spacers, and on_scroll wiring will follow after the state and
//! controller are integrated with UI messages and registry.

use iced::{
    Background, Color, Element, Length, Shadow,
    widget::{
        Space, button, column, container, mouse_area, row, scrollable, text,
    },
};

use crate::domains::ui::{messages::UiMessage, theme};
use crate::infra::theme::accent;
use lucide_icons::Icon;

use super::{state::VirtualCarouselState, types::CarouselKey};
use crate::infra::constants::calculations::ScaledLayout;
use crate::infra::constants::virtual_carousel::layout as vcl;

/// Build a virtual carousel view for a given key/state. Placeholder only.
#[allow(unused_variables)]
pub fn virtual_carousel<'a, F>(
    key: CarouselKey,
    title: &'a str,
    total_items: usize,
    state: &VirtualCarouselState,
    create_item: F,
    is_active: bool,
    fonts: &crate::infra::design_tokens::FontTokens,
    scaled_layout: &ScaledLayout,
) -> Element<'a, UiMessage>
where
    F: Fn(usize) -> Option<Element<'a, UiMessage>>,
{
    // Header with title and nav buttons
    let can_left = state.scroll_x > 0.0;
    let can_right = state.scroll_x < state.max_scroll;

    let left_button = if can_left {
        button(
            text(icon_char(Icon::ChevronLeft))
                .font(lucide_font())
                .size(fonts.subtitle)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
        )
        .on_press(UiMessage::VirtualCarousel(
            super::messages::VirtualCarouselMessage::PrevItem(key.clone()),
        ))
        .padding(8)
        .style(theme::Button::Secondary.style())
    } else {
        button(
            text(icon_char(Icon::ChevronLeft))
                .font(lucide_font())
                .size(fonts.subtitle)
                .color(theme::MediaServerTheme::TEXT_DIMMED),
        )
        .padding(8)
        .style(theme::Button::Secondary.style())
    };

    let right_button = if can_right {
        button(
            text(icon_char(Icon::ChevronRight))
                .font(lucide_font())
                .size(fonts.subtitle)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
        )
        .on_press(UiMessage::VirtualCarousel(
            super::messages::VirtualCarouselMessage::NextItem(key.clone()),
        ))
        .padding(8)
        .style(theme::Button::Secondary.style())
    } else {
        button(
            text(icon_char(Icon::ChevronRight))
                .font(lucide_font())
                .size(fonts.subtitle)
                .color(theme::MediaServerTheme::TEXT_DIMMED),
        )
        .padding(8)
        .style(theme::Button::Secondary.style())
    };

    let title_color = if is_active {
        accent()
    } else {
        theme::MediaServerTheme::TEXT_PRIMARY
    };

    let h_padding = scaled_layout.min_viewport_padding();
    let header = container(
        row![
            text(title).size(fonts.title).color(title_color),
            Space::new().width(Length::Fill),
            row![left_button, Space::new().width(5), right_button]
                .align_y(iced::Alignment::Center)
        ]
        .align_y(iced::Alignment::Center)
        .width(Length::Fill),
    )
    .padding([0, h_padding as u16]);

    // Build windowed row with spacers
    let stride = state.item_width + state.item_spacing;
    let mut item_row = row![].spacing(0);
    let vr = state.visible_range.clone();

    // Left spacer for items before visible range
    if vr.start > 0 {
        let spacer_w = vr.start as f32 * stride;
        item_row = item_row.push(Space::new().width(Length::Fixed(spacer_w)));
    }

    let mut first_item = true;
    for idx in vr.clone() {
        if idx < total_items {
            if let Some(el) = create_item(idx) {
                if !first_item {
                    item_row = item_row.push(
                        Space::new().width(Length::Fixed(state.item_spacing)),
                    );
                }
                item_row = item_row.push(el);
                first_item = false;
            } else {
                // Placeholder for missing elements keeps alignment stable
                if !first_item {
                    item_row = item_row.push(
                        Space::new().width(Length::Fixed(state.item_spacing)),
                    );
                }
                item_row = item_row
                    .push(Space::new().width(Length::Fixed(state.item_width)));
                first_item = false;
            }
        }
    }

    // Right spacer for items after visible range
    if vr.end < total_items {
        let remaining = total_items - vr.end;
        let spacer_w = remaining as f32 * stride;
        item_row = item_row.push(Space::new().width(Length::Fixed(spacer_w)));
    }

    // Clone key for closures that need ownership
    let key_for_scroll = key.clone();
    let key_for_enter = key.clone();
    let key_for_exit = key;

    let scroll = scrollable(row![item_row])
        .id(state.scrollable_id.clone())
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new().width(0).scroller_width(0),
        ))
        .on_scroll(move |viewport| {
            UiMessage::VirtualCarousel(
                super::messages::VirtualCarouselMessage::ViewportChanged(
                    key_for_scroll.clone(),
                    viewport,
                ),
            )
        })
        .width(Length::Fill)
        .height(Length::Fixed(scaled_layout.row_height));

    // Apply horizontal padding; no additional styling for active state
    let scroll_padded = container(scroll).padding([0, h_padding as u16]);
    let header_spacing = vcl::HEADER_SCROLL_SPACING * scaled_layout.scale;
    let section =
        column![header, Space::new().height(header_spacing), scroll_padded]
            .width(Length::Fill);

    // Wrap section with mouse_area to track hover for focus
    let section_with_hover = mouse_area(section)
        .on_enter(UiMessage::VirtualCarousel(
            super::messages::VirtualCarouselMessage::FocusKey(key_for_enter),
        ))
        .on_exit(UiMessage::VirtualCarousel(
            super::messages::VirtualCarouselMessage::BlurKey(key_for_exit),
        ));

    if is_active {
        // Add a slim left accent rail matching the section height
        let rail_h = (vcl::HEADER_HEIGHT_EST + vcl::HEADER_SCROLL_SPACING)
            * scaled_layout.scale
            + scaled_layout.row_height;
        let rail = container(Space::new().height(Length::Fixed(rail_h)))
            .width(Length::Fixed(3.0))
            .style(rail_style);
        row![rail, section_with_hover].into()
    } else {
        section_with_hover.into()
    }
}

fn lucide_font() -> iced::Font {
    iced::Font::with_name("lucide")
}
fn icon_char(icon: Icon) -> String {
    icon.unicode().to_string()
}

fn rail_style(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        text_color: None,
        background: Some(Background::Color(accent())),
        border: iced::Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 0.0.into(),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}
