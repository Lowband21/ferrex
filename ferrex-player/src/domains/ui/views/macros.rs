//! Core macros for the media card system
//!
//! This module provides the main `media_card!` macro and supporting macros
//! for creating consistent, animated media cards across the application.

use iced::Color;

/// Parse a hex color string into an Iced Color
pub fn parse_hex_color(hex: &str) -> Result<Color, String> {
    let hex = hex.trim_start_matches('#');

    if hex.len() != 6 {
        return Err(format!("Invalid hex color length: {}", hex.len()));
    }

    let r =
        u8::from_str_radix(&hex[0..2], 16).map_err(|e| format!("Invalid red component: {}", e))?;
    let g = u8::from_str_radix(&hex[2..4], 16)
        .map_err(|e| format!("Invalid green component: {}", e))?;
    let b =
        u8::from_str_radix(&hex[4..6], 16).map_err(|e| format!("Invalid blue component: {}", e))?;

    Ok(Color::from_rgb8(r, g, b))
}

/// Truncate text to fit within a given width with ellipsis
pub fn truncate_text(text: &str, max_chars: usize) -> String {
    // Count actual characters, not bytes
    let char_count = text.chars().count();

    if char_count <= max_chars {
        text.to_string()
    } else {
        // Reserve space for "..."
        let target_chars = max_chars.saturating_sub(3);

        // Collect characters up to the target count
        let mut chars_collected = 0;
        let mut byte_index = 0;

        for (i, ch) in text.char_indices() {
            if chars_collected >= target_chars {
                byte_index = i;
                break;
            }
            chars_collected += 1;
        }

        // If we didn't break early, use the full string length
        if chars_collected < target_chars {
            byte_index = text.len();
        }

        // Try to break at a space for better readability
        let truncated = &text[..byte_index];
        if let Some(space_pos) = truncated.rfind(' ') {
            // Only use the space if it's not too far back (at least halfway)
            let space_chars = text[..space_pos].chars().count();
            if space_chars > target_chars / 2 {
                return format!("{}...", &text[..space_pos]);
            }
        }

        format!("{}...", truncated)
    }
}

/// Trait for accessing theme color on media references
pub trait ThemeColorAccess {
    fn theme_color(&self) -> Option<&str>;
}

// Implement for types that have theme_color
impl ThemeColorAccess for crate::infrastructure::api_types::MovieReference {
    fn theme_color(&self) -> Option<&str> {
        self.theme_color.as_deref()
    }
}

impl ThemeColorAccess for crate::infrastructure::api_types::SeriesReference {
    fn theme_color(&self) -> Option<&str> {
        self.theme_color.as_deref()
    }
}

impl ThemeColorAccess for crate::infrastructure::api_types::SeasonReference {
    fn theme_color(&self) -> Option<&str> {
        self.theme_color.as_deref()
    }
}

// Episodes don't have theme_color
impl ThemeColorAccess for crate::infrastructure::api_types::EpisodeReference {
    fn theme_color(&self) -> Option<&str> {
        None
    }
}

/// Main macro for creating media cards with consistent styling and behavior
#[macro_export]
macro_rules! media_card {
    (
        // Required parameters
        type: $card_type:ident,
        data: $data:expr,

        // Card configuration block
        {
            id: $id:expr,
            title: $title:expr,
            subtitle: $subtitle:expr,
            image: {
                key: $image_key:expr,
                type: $image_type:ident,
                fallback: $fallback:expr,
            },
            size: $size:ident,

            // Actions
            on_click: $click_msg:expr,
            on_play: $play_msg:expr,

            // Optional fields
            $(hover_icon: $hover_icon:expr,)?
            $(badge: $badge:expr,)?
            $(animation: $animation:expr,)?
            $(loading_text: $loading_text:expr,)?
            $(is_hovered: $is_hovered:expr,)?
            $(priority: $priority:expr,)?
        }
    ) => {{
        use $crate::domains::ui::views::cards::types::*;
        use $crate::domains::ui::widgets::{AnimationType as WidgetAnimationType};
        use $crate::domains::ui::theme;
        use iced::{
            widget::{button, column, container, text},
            Length,
        };

        // Extract dimensions from card size
        let card_size = CardSize::$size;
        let (width, height) = card_size.dimensions();
        let radius = card_size.radius();
        let (title_size, subtitle_size) = card_size.text_sizes();

        // Determine if card is hovered
        let is_hovered = {
            #[allow(unused)]
            let mut hovered = false;
            $(hovered = $is_hovered;)?
            hovered
        };

        // Get animation config
        let animation_config = {
            #[allow(unused_mut)]
            let mut config = AnimationConfig::default();
            $(config = $animation;)?
            config
        };

        // Determine widget animation type early so it can be used for both image and overlay
        let widget_anim = match animation_config.animation_type {
            AnimationType::Flip => WidgetAnimationType::enhanced_flip(), // Use enhanced flip for better effect
            AnimationType::FadeIn | AnimationType::FadeScale => WidgetAnimationType::Fade {
                duration: animation_config.duration
            },
            _ => WidgetAnimationType::None,
        };

        // Create the main image/poster element using image_for
        let image_element: Element<'_, $crate::domains::ui::messages::Message> = {
            use $crate::domains::ui::widgets::image_for;

            // Determine image size based on card size
            let image_size = match card_size {
                CardSize::Small => $crate::domains::metadata::image_types::ImageSize::Thumbnail,
                CardSize::Medium => $crate::domains::metadata::image_types::ImageSize::Poster,
                CardSize::Large => $crate::domains::metadata::image_types::ImageSize::Full,
                CardSize::Wide => $crate::domains::metadata::image_types::ImageSize::Backdrop,
                CardSize::Custom(_, _) => $crate::domains::metadata::image_types::ImageSize::Poster,
            };

            //// Map priority if provided
            let priority = $crate::domains::metadata::image_types::Priority::Preload;
            $(let priority = $priority;)?

            // Create the image widget
            let mut img = image_for($id)
                .size(image_size)
                .rounded(radius)
                .width(Length::Fixed(width))
                .height(Length::Fixed(height))
                .animation(widget_anim)
                .placeholder($fallback.chars().next().map(|c| {
                    // Convert emoji to appropriate icon
                    match c {
                        '🎬' => lucide_icons::Icon::Film,
                        '📺' => lucide_icons::Icon::Tv,
                        '🎞' => lucide_icons::Icon::Play,
                        _ => lucide_icons::Icon::Image,
                    }
                }).unwrap_or(lucide_icons::Icon::Image))
                .priority(priority)
                .is_hovered(is_hovered)
                .on_play($play_msg)
                .on_click($click_msg);

            // Add loading text if provided
            $(img = img.placeholder_text($loading_text);)?

            // Add theme color if available
            use $crate::domains::ui::views::macros::ThemeColorAccess;
            if let Some(theme_color_str) = $data.theme_color() {
                //log::info!("Card for {} has theme_color: {}", $title, theme_color_str);
                if let Ok(color) = $crate::domains::ui::views::macros::parse_hex_color(theme_color_str) {
                    img = img.theme_color(color);
                } else {
                    log::warn!("Could not parse theme_color_str {} for {}", theme_color_str, $title);
                }
            }

            img.into()
        };

        // Wrap the image element with precise hover detection
        // This tracks only the actual poster bounds, not the container
        let image_with_hover = iced::widget::mouse_area(image_element)
            .on_enter($crate::domains::ui::messages::Message::MediaHovered($image_key))
            .on_exit($crate::domains::ui::messages::Message::MediaUnhovered($image_key));

        // Create the poster element
        // Always wrap in button for non-hover clicks, but the shader handles its own overlay buttons
        let poster_element = button(image_with_hover)
            .on_press($click_msg)
            .padding(0)
            .style(theme::Button::MediaCard.style())
            .into();

        // Calculate proper container dimensions based on animation type
        let (container_width, container_height) = if matches!(widget_anim, WidgetAnimationType::EnhancedFlip { .. }) {
            // For enhanced flip, use expanded dimensions to accommodate animation
            use $crate::infrastructure::constants::animation;
            let h_padding = animation::calculate_horizontal_padding(width);
            let v_padding = animation::calculate_vertical_padding(height);
            (width + h_padding * 2.0, height + v_padding * 2.0)
        } else {
            // For other animations, use standard dimensions
            (width, height)
        };

        // For enhanced flip, we need to center the poster within the container
        let poster_with_overlay_element = if matches!(widget_anim, WidgetAnimationType::EnhancedFlip { .. }) {
            // Center the poster within the larger container
            // The shader handles all hover detection internally based on actual poster bounds
            let centered_poster: Element<'_, $crate::domains::ui::messages::Message> = container(poster_element)
                .width(Length::Fixed(container_width))
                .height(Length::Fixed(container_height))
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .into();
            centered_poster
        } else {
            // For non-animated posters, return directly
            // The shader handles all hover detection internally
            poster_element
        };

        // Calculate max characters based on card width and text size
        // Rough estimate: ~7-8 pixels per character for typical fonts
        let title_max_chars = ((width - 10.0) / (title_size as f32 * 0.6)) as usize;
        let subtitle_max_chars = ((width - 10.0) / (subtitle_size as f32 * 0.6)) as usize;

        // Truncate title and subtitle to prevent wrapping
        let truncated_title = $crate::domains::ui::views::macros::truncate_text($title, title_max_chars);
        let truncated_subtitle = $crate::domains::ui::views::macros::truncate_text($subtitle, subtitle_max_chars);

        // Create the complete card with text
        let card_content = column![
            poster_with_overlay_element,
            // Text container
            container(
                column![
                    text(truncated_title)
                        .size(title_size)
                        .color(theme::MediaServerTheme::TEXT_PRIMARY),
                    text(truncated_subtitle)
                        .size(subtitle_size)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY)
                ]
                .spacing(2)
            )
            .padding(5)
            .width(Length::Fixed(width))
            .height(Length::Fixed(60.0))
            .clip(true)
        ]
        .spacing(5);

        // Final container (no mouse area here anymore)
        container(card_content)
            .width(Length::Fixed(container_width))
            .height(Length::Fixed(container_height + 65.0)) // Image + text height
            .clip(false) // Allow animation overflow
            .into()
    }};
}

/// Macro for creating loading shimmer effects
#[macro_export]
macro_rules! loading_shimmer {
    ($width:expr, $height:expr, $text:expr, $radius:expr) => {{
        use iced::{
            alignment,
            widget::{column, container, shader, text},
            Color, Element, Length,
        };
        use std::time::Instant;
        use $crate::domains::ui::theme;

        // Create shimmer shader
        let shimmer = shader(|bounds, _size, _cursor| {
            use iced::widget::shader::{Mesh2D, Primitive, SolidVertex2D};
            use iced::Renderer;

            let time = Instant::now().elapsed().as_secs_f32();
            let shimmer_position = (time * 0.5).sin() * 0.5 + 0.5;

            // Create gradient effect
            let vertices = vec![
                SolidVertex2D {
                    position: [0.0, 0.0],
                    color: [0.2, 0.2, 0.2, 1.0],
                },
                SolidVertex2D {
                    position: [bounds.width, 0.0],
                    color: [0.3, 0.3, 0.3, 1.0],
                },
                SolidVertex2D {
                    position: [bounds.width, bounds.height],
                    color: [0.2, 0.2, 0.2, 1.0],
                },
                SolidVertex2D {
                    position: [0.0, bounds.height],
                    color: [0.25, 0.25, 0.25, 1.0],
                },
            ];

            let indices = vec![0, 1, 2, 0, 2, 3];

            Primitive::Mesh2D(Mesh2D { vertices, indices })
        })
        .width(Length::Fixed($width))
        .height(Length::Fixed($height));

        // Overlay loading text
        iced::widget::Stack::new()
            .push(shimmer)
            .push(
                container(
                    column![text("⏳").size(32), text($text).size(12)]
                        .align_x(alignment::Alignment::Center)
                        .spacing(5),
                )
                .width(Length::Fixed($width))
                .height(Length::Fixed($height))
                .align_x(alignment::Horizontal::Center)
                .align_y(alignment::Vertical::Center),
            )
            .into()
    }};
}

/// Macro for creating hover overlays with action buttons
#[macro_export]
macro_rules! hover_overlay {
    (
        $width:expr,
        $height:expr,
        $radius:expr,
        {
            center: ($center_icon:expr, $center_action:expr),
            $(top_left: ($tl_icon:expr, $tl_action:expr),)?
            $(top_right: ($tr_icon:expr, $tr_action:expr),)?
            $(bottom_left: ($bl_icon:expr, $bl_action:expr),)?
            $(bottom_right: ($br_icon:expr, $br_action:expr),)?
        }
    ) => {{
        use iced::{
            widget::{button, column, container, row, Space, Stack},
            Color, Length, alignment,
        };
        use $crate::domains::ui::theme;

        // Helper to create icon text - use a macro instead of closure
        macro_rules! icon_text {
            ($icon:expr) => {
                text($icon.unicode())
                    .font(iced::Font::with_name("lucide"))
                    .color(Color::WHITE)
            };
        }

        // Dark overlay background
        let overlay_bg = container("")
            .width(Length::Fixed($width))
            .height(Length::Fixed($height))
            .style(move |_| iced::widget::container::Style {
                background: Some(iced::Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
                border: iced::Border {
                    color: theme::MediaServerTheme::ACCENT_BLUE,
                    width: 3.0,
                    radius: $radius.into(),
                },
                shadow: iced::Shadow {
                    color: theme::MediaServerTheme::ACCENT_BLUE_GLOW,
                    offset: iced::Vector { x: 0.0, y: 0.0 },
                    blur_radius: 15.0,
                },
                ..Default::default()
            });

        // Center button
        let center_button = button(icon_text($center_icon).size(32))
            .on_press($center_action)
            .padding(16)
            .style(theme::Button::PlayOverlay.style());

        // Build overlay layout
        let mut top_row = row![];
        let mut bottom_row = row![];

        // Top left button
        $(
            let tl_button = button(icon_text($tl_icon).size(20))
                .on_press($tl_action)
                .padding(8)
                .style(theme::Button::Icon.style());
            top_row = top_row.push(tl_button);
        )?
        top_row = top_row.push(Space::with_width(Length::Fill));

        // Top right button
        $(
            let tr_button = button(icon_text($tr_icon).size(20))
                .on_press($tr_action)
                .padding(8)
                .style(theme::Button::Icon.style());
            top_row = top_row.push(tr_button);
        )?

        // Bottom left button
        $(
            let bl_button = button(icon_text($bl_icon).size(20))
                .on_press($bl_action)
                .padding(8)
                .style(theme::Button::Icon.style());
            bottom_row = bottom_row.push(bl_button);
        )?
        bottom_row = bottom_row.push(Space::with_width(Length::Fill));

        // Bottom right button
        $(
            let br_button = button(icon_text($br_icon).size(20))
                .on_press($br_action)
                .padding(8)
                .style(theme::Button::Icon.style());
            bottom_row = bottom_row.push(br_button);
        )?

        // Compose overlay content
        let overlay_content = container(
            column![
                top_row.width(Length::Fill),
                container(center_button)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(alignment::Horizontal::Center)
                    .align_y(alignment::Vertical::Center),
                bottom_row.width(Length::Fill)
            ]
            .width(Length::Fill)
            .height(Length::Fill)
        )
        .width(Length::Fixed($width))
        .height(Length::Fixed($height))
        .padding(5); // Reduced padding to prevent height reduction

        Element::from(Stack::new()
            .push(overlay_bg)
            .push(overlay_content)
            .width(Length::Fixed($width))
            .height(Length::Fixed($height)))
    }};
}

/// Macro for creating a grid of media cards
#[macro_export]
macro_rules! media_grid {
    (
        items: $items:expr,
        card_type: $card_type:ident,
        columns: $columns:expr,
        spacing: $spacing:expr,
    ) => {{
        use iced::{
            widget::{column, row, Space},
            Element, Length,
        };

        let mut rows = Vec::new();
        let mut current_row = Vec::new();
        let items_count = $items.len();

        for (idx, item) in $items.into_iter().enumerate() {
            let card = $crate::$card_type!(item);
            current_row.push(card);

            if current_row.len() >= $columns || idx == items_count - 1 {
                // Pad the last row if needed
                while current_row.len() < $columns && idx == items_count - 1 {
                    current_row.push(Space::with_width(Length::Fixed(200.0)).into());
                }

                let row = row(current_row).spacing($spacing).into();
                rows.push(row);
                current_row = Vec::new();
            }
        }

        column(rows).spacing($spacing).into()
    }};
}
