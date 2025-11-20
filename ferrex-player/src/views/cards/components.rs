//! Helper components for the media card macro system

use crate::{messages::ui::Message, theme};
use iced::{
    alignment,
    widget::{button, container, text, Space},
    Color, Element, Length,
};

/// Create a shimmer loading effect
pub fn shimmer_effect(width: f32, height: f32, _radius: f32) -> Element<'static, Message> {
    // Shimmer effect temporarily removed due to shader type compatibility
    // Using simple placeholder instead
    container(Space::new(Length::Fill, Length::Fill))
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .style(theme::Container::Card.style())
        .into()
}

/// Linear interpolation between two colors
fn lerp_color(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

/// Create an icon button for overlays
pub fn icon_button(
    icon: lucide_icons::Icon,
    size: u16,
    action: Message,
    padding: u16,
) -> Element<'static, Message> {
    button(
        text(icon.unicode())
            .font(iced::Font::with_name("lucide"))
            .size(size as u32)
            .color(Color::WHITE),
    )
    .on_press(action)
    .padding(padding)
    .style(theme::Button::Icon.style())
    .into()
}

/// Create a play button overlay
pub fn play_overlay_button(action: Message) -> Element<'static, Message> {
    button(
        text(lucide_icons::Icon::Play.unicode())
            .font(iced::Font::with_name("lucide"))
            .size(32)
            .color(Color::WHITE),
    )
    .on_press(action)
    .padding(16)
    .style(theme::Button::PlayOverlay.style())
    .into()
}

/// Create a badge element
pub fn badge_element(
    content: String,
    bg_color: Color,
    text_color: Color,
) -> Element<'static, Message> {
    container(text(content).size(12).color(text_color))
        .padding(5)
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Background::Color(bg_color)),
            border: iced::Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// Create a rating badge
pub fn rating_badge(rating: f32) -> Element<'static, Message> {
    use crate::views::cards::styles::badge_styles;
    let (bg, fg) = badge_styles::rating_badge();
    badge_element(format!("★ {:.1}", rating), bg, fg)
}

/// Create a season/episode count badge
pub fn count_badge(count: usize, label: &str) -> Element<'static, Message> {
    use crate::views::cards::styles::badge_styles;
    let (bg, fg) = badge_styles::episode_count_badge();
    badge_element(format!("{} {}", count, label), bg, fg)
}

/// Create a "NEW" badge
pub fn new_badge() -> Element<'static, Message> {
    use crate::views::cards::styles::badge_styles;
    let (bg, fg) = badge_styles::new_badge();
    badge_element("NEW".to_string(), bg, fg)
}

/// Position a badge on a card
pub fn position_badge(
    badge: Element<'static, Message>,
    position: crate::views::cards::types::BadgePosition,
    card_width: f32,
    card_height: f32,
) -> Element<'static, Message> {
    use crate::views::cards::types::BadgePosition;

    let padding: [f32; 2] = match position {
        BadgePosition::TopLeft => [-8.0, 8.0],
        BadgePosition::TopRight => [8.0, 8.0],
        BadgePosition::BottomLeft => [-8.0, -8.0],
        BadgePosition::BottomRight => [8.0, -8.0],
    };

    let (h_align, v_align) = match position {
        BadgePosition::TopLeft => (alignment::Horizontal::Left, alignment::Vertical::Top),
        BadgePosition::TopRight => (alignment::Horizontal::Right, alignment::Vertical::Top),
        BadgePosition::BottomLeft => (alignment::Horizontal::Left, alignment::Vertical::Bottom),
        BadgePosition::BottomRight => (alignment::Horizontal::Right, alignment::Vertical::Bottom),
    };

    container(badge)
        .width(Length::Fixed(card_width))
        .height(Length::Fixed(card_height))
        .padding(padding)
        .align_x(h_align)
        .align_y(v_align)
        .into()
}

/// Create an animated entrance for cards
pub fn entrance_animation(
    content: Element<'static, Message>,
    animation_type: crate::views::cards::types::AnimationType,
    progress: f32,
) -> Element<'static, Message> {
    use crate::views::cards::types::{AnimationType, Direction};

    match animation_type {
        AnimationType::FadeIn => {
            // Simple fade
            container(content)
                .style(move |_| iced::widget::container::Style {
                    background: None,
                    border: iced::Border::default(),
                    shadow: iced::Shadow::default(),
                    text_color: None,
                    snap: false,
                })
                .into()
        }
        AnimationType::ScaleIn => {
            // Scale from center
            let scale = progress;
            container(content)
                .style(move |_| iced::widget::container::Style {
                    background: None,
                    border: iced::Border::default(),
                    shadow: iced::Shadow::default(),
                    text_color: None,
                    snap: false,
                })
                .into()
        }
        AnimationType::SlideIn(direction) => {
            // Slide from direction
            let offset = match direction {
                Direction::Left => (-50.0 * (1.0 - progress), 0.0),
                Direction::Right => (50.0 * (1.0 - progress), 0.0),
                Direction::Top => (0.0, -50.0 * (1.0 - progress)),
                Direction::Bottom => (0.0, 50.0 * (1.0 - progress)),
            };

            container(content)
                .style(move |_| iced::widget::container::Style {
                    background: None,
                    border: iced::Border::default(),
                    shadow: iced::Shadow::default(),
                    text_color: None,
                    snap: false,
                })
                .into()
        }
        _ => content, // Flip and FadeScale need special handling
    }
}

/// Helper to calculate staggered animation delays
pub fn calculate_stagger_delay(index: usize, stagger_ms: u64) -> std::time::Duration {
    std::time::Duration::from_millis(index as u64 * stagger_ms)
}

/// Create a skeleton loader for text
pub fn text_skeleton(width: f32, height: f32) -> Element<'static, Message> {
    container("")
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .style(|_| iced::widget::container::Style {
            background: Some(iced::Background::Color(Color::from_rgba(
                0.3, 0.3, 0.3, 0.3,
            ))),
            border: iced::Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// Create a media type indicator
pub fn media_type_indicator(
    media_type: crate::views::cards::types::MediaType,
) -> Element<'static, Message> {
    let icon = media_type.hover_icon();
    let color = match media_type {
        crate::views::cards::types::MediaType::Movie => Color::from_rgb(1.0, 0.5, 0.0),
        crate::views::cards::types::MediaType::Series => Color::from_rgb(0.0, 0.7, 1.0),
        crate::views::cards::types::MediaType::Season => Color::from_rgb(0.0, 0.7, 1.0),
        crate::views::cards::types::MediaType::Episode => Color::from_rgb(0.5, 0.5, 1.0),
    };

    text(icon.unicode())
        .font(iced::Font::with_name("lucide"))
        .size(16)
        .color(color)
        .into()
}
