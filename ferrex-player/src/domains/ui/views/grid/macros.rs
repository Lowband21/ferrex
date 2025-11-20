//! Macros for generating virtual grid views

use ferrex_core::player_prelude::{
    ArchivedEpisodeReference, ArchivedMovieReference, ArchivedSeasonReference,
    ArchivedSeriesReference, EpisodeReference, MovieReference, SeasonReference,
    SeriesReference,
};
/// Macro to generate virtual grid views for different reference types
/// This eliminates code duplication between movie and series grids
use iced::Color;
#[macro_export]
macro_rules! virtual_reference_grid {
    (
        $name:ident,
        $item_type:ty,
        $create_card:expr_2021,
        $profiler_label:literal
    ) => {
        pub fn $name<'a>(
            item_index: &[Uuid],
            grid_state: &super::VirtualGridState,
            hovered_media_id: &Option<Uuid>,
            on_scroll: impl Fn(iced::widget::scrollable::Viewport) -> $crate::domains::ui::messages::UiMessage + 'a,
            state: &'a $crate::state::State,
        ) -> iced::Element<'a, $crate::domains::ui::messages::UiMessage> {
            let len = item_index.len();
            //let reference_grid = iced::debug::time($profiler_label);
            use iced::{
                widget::{column, container, row, scrollable, text, Space},
                Length,
            };
            // Profile grid rendering operations
            #[cfg(any(feature = "profile-with-puffin", feature = "profile-with-tracy", feature = "profile-with-tracing"))]
            profiling::scope!("View::Grid::Total");

            // let is_scrolling = if let Some(last_scroll) = state.domains.ui.state.last_scroll_time {
            //     let elapsed = last_scroll.elapsed();
            //     elapsed < std::time::Duration::from_millis(
            //         $crate::infra::constants::performance_config::scrolling::SCROLL_STOP_DEBOUNCE_MS
            //     )
            // } else {
            //     false
            // };

            /*
            log::trace!("{}: rendering {} items (scroll={})",
                      $profiler_label, items.len(), is_scrolling);
            if !items.is_empty() {
                log::trace!("First item in grid");
            } */

            use $crate::infra::constants::{grid, poster};

            // Don't add spacing here since ROW_HEIGHT already includes spacing
            let mut content = column![].spacing(0).width(Length::Fill);

            // Defensive check: ensure we have valid columns
            if grid_state.columns == 0 {
                log::error!("Grid state has 0 columns! This should never happen.");
                return container(text("Grid configuration error"))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center)
                    .into();
            }

            // Add top padding to prevent content from touching header
            //content = content.push(Space::new().height(header::HEIGHT * 2.0));

            // Calculate total rows
            let total_rows = len.div_ceil(grid_state.columns);

            // Add spacer for rows above viewport
            let start_row = grid_state.visible_range.start / grid_state.columns;
            if start_row > 0 {
                let spacer_height = start_row as f32 * grid_state.row_height;
                content = content.push(Space::new().height(Length::Fixed(spacer_height)));
            }

            let watch_state_opt = state.domains.media.state.get_watch_state();

            // Render visible rows
            let end_row =
                grid_state.visible_range.end.div_ceil(grid_state.columns);
            for row_idx in start_row..end_row.min(total_rows) {
                let mut row_content = row![].spacing(grid::EFFECTIVE_SPACING);

                for col in 0..grid_state.columns {
                    let item_idx = row_idx * grid_state.columns + col;
                    if item_idx < len && item_idx < grid_state.visible_range.end {
                        //let item = &items[item_idx];

                        let is_visible =
                            item_idx >= grid_state.visible_range.start
                                && item_idx < grid_state.visible_range.end;

                        let item_id = item_index[item_idx];

                        let item_watch_progress = if let Some(watch_state) = watch_state_opt {
                            watch_state.get_watch_progress(&item_id)
                        } else {
                            None
                        };

                        // Call the card creation function with visibility info
                        let card = $create_card(
                            item_id,
                            hovered_media_id,
                            is_visible,
                            item_watch_progress,
                            state,
                        );

                        // Use container dimensions that account for animation padding
                        let (container_width, _container_height) =
                            $crate::infra::constants::calculations::get_container_dimensions(1.0);

                        // Debug logging to verify container dimensions
                        //if item_idx == 0 {
                        //    log::info!("Grid container dimensions: {}x{} (includes animation padding)", container_width, container_height);
                        //    log::info!("Base poster size: {}x{}", poster::BASE_WIDTH, poster::BASE_HEIGHT);
                        //    log::info!("Horizontal animation padding per side: {}", $crate::constants::animation::calculate_horizontal_padding(poster::BASE_WIDTH));
                        //}

                        // Use total card height with animation padding
                        let total_card_height = poster::TOTAL_CARD_HEIGHT
                            + 2.0 * $crate::infra::constants::animation::calculate_vertical_padding(poster::BASE_HEIGHT);

                        row_content = row_content.push(
                            container(card)
                                .width(Length::Fixed(container_width))
                                .height(Length::Fixed(total_card_height))
                                .clip(false),
                        );
                    } else if item_idx < len {
                        // Placeholder for items not yet visible but in the row
                        let (container_width, _) =
                            $crate::infra::constants::calculations::get_container_dimensions(1.0);
                        let total_card_height = poster::TOTAL_CARD_HEIGHT
                            + 2.0 * $crate::infra::constants::animation::calculate_vertical_padding(poster::BASE_HEIGHT);
                        row_content = row_content.push(
                            container(Space::new().width(
                                container_width,
                            ).height(
                                total_card_height,
                            ))
                            .style($crate::domains::ui::theme::Container::Default.style()),
                        );
                    }
                }

                // Fill remaining columns with empty space only if this is the last row and it's incomplete
                if row_idx == total_rows - 1 {
                    let items_in_last_row = len - (row_idx * grid_state.columns);
                    if items_in_last_row < grid_state.columns {
                        for _ in items_in_last_row..grid_state.columns {
                            let (container_width, _) =
                                $crate::infra::constants::calculations::get_container_dimensions(1.0);
                            row_content = row_content.push(Space::new().width(container_width));
                        }
                    }
                }

                // Row container with centered alignment
                let row_container = container(row_content)
                    .width(Length::Fill)
                    .height(Length::Fixed(grid_state.row_height))
                    .align_x(iced::alignment::Horizontal::Center)
                    .clip(false);

                content = content.push(row_container);
            }

            // Add spacer for rows below viewport
            let remaining_rows = total_rows.saturating_sub(end_row);
            if remaining_rows > 0 {
                let spacer_height = remaining_rows as f32 * grid_state.row_height;
                content = content.push(Space::new().height(Length::Fixed(spacer_height)));
            }

            // Add some padding at the bottom
            content = content.push(Space::new().height(grid::BOTTOM_PADDING));

            let total_height = total_rows as f32 * grid_state.row_height
                + grid::TOP_PADDING
                + grid::BOTTOM_PADDING;

            // Calculate horizontal padding for centering (matching VirtualGridState expectation)
            let horizontal_padding = grid::MIN_VIEWPORT_PADDING;

            let scrollable_content = scrollable(
                container(content)
                    .width(Length::Fill)
                    .height(Length::Fixed(total_height))
                    .padding([0, horizontal_padding as u16])
                    .clip(false),
            )
            .id(grid_state.scrollable_id.clone())
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::default(),
            ))
            .on_scroll(move |viewport| {
                on_scroll(viewport)
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .style($crate::domains::ui::theme::Scrollable::style());

            scrollable_content.into()
        }
    };
}

/// Parse a hex color string into an Iced Color
pub fn parse_hex_color(hex: &str) -> Result<Color, String> {
    let hex = hex.trim_start_matches('#');

    if hex.len() != 6 {
        return Err(format!("Invalid hex color length: {}", hex.len()));
    }

    let r = u8::from_str_radix(&hex[0..2], 16)
        .map_err(|e| format!("Invalid red component: {}", e))?;
    let g = u8::from_str_radix(&hex[2..4], 16)
        .map_err(|e| format!("Invalid green component: {}", e))?;
    let b = u8::from_str_radix(&hex[4..6], 16)
        .map_err(|e| format!("Invalid blue component: {}", e))?;

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

        for (i, _ch) in text.char_indices() {
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

// Theme color access for media references
impl ThemeColorAccess for MovieReference {
    fn theme_color(&self) -> Option<&str> {
        self.theme_color.as_deref()
    }
}

impl ThemeColorAccess for SeriesReference {
    fn theme_color(&self) -> Option<&str> {
        self.theme_color.as_deref()
    }
}

impl ThemeColorAccess for SeasonReference {
    fn theme_color(&self) -> Option<&str> {
        self.theme_color.as_deref()
    }
}

impl ThemeColorAccess for EpisodeReference {
    fn theme_color(&self) -> Option<&str> {
        None
    }
}

// Archived references (yoked)
impl ThemeColorAccess for ArchivedMovieReference {
    fn theme_color(&self) -> Option<&str> {
        self.theme_color.as_deref()
    }
}

impl ThemeColorAccess for ArchivedSeriesReference {
    fn theme_color(&self) -> Option<&str> {
        self.theme_color.as_deref()
    }
}

impl ThemeColorAccess for ArchivedSeasonReference {
    fn theme_color(&self) -> Option<&str> {
        self.theme_color.as_deref()
    }
}

impl ThemeColorAccess for ArchivedEpisodeReference {
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
        data: $data:expr_2021,

        // Card configuration block
        {
            id: $id:expr_2021,
            title: $title:expr_2021,
            subtitle: $subtitle:expr_2021,
            image: {
                key: $image_key:expr_2021,
                type: $image_type:ident,
                fallback: $fallback:expr_2021,
            },
            size: $size:ident,

            // Actions
            on_click: $click_msg:expr_2021,
            on_play: $play_msg:expr_2021,

            // Optional fields
            $(hover_icon: $hover_icon:expr_2021,)?
            $(badge: $badge:expr_2021,)?
            $(animation: $animation:expr_2021,)?
            $(loading_text: $loading_text:expr_2021,)?
            $(is_hovered: $is_hovered:expr_2021,)?
            $(priority: $priority:expr_2021,)?
        }
    ) => {{
        use $crate::domains::ui::views::grid::types::*;
        use $crate::domains::ui::widgets::poster::poster_animation_types::PosterAnimationType;
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
            AnimationType::Flip => PosterAnimationType::flip(),
            AnimationType::FadeIn | AnimationType::FadeScale => PosterAnimationType::Fade {
                duration: animation_config.duration
            },
            // For unsupported poster-shader animations (SlideIn, ScaleIn, etc.),
            // prefer opacity over none for the primary animation.
            _ => PosterAnimationType::Fade { duration: animation_config.duration },
        };

        // Create the main image/poster element using image_for
        let image_element: Element<'_, $crate::domains::ui::messages::UiMessage> = {
            use $crate::domains::ui::widgets::image_for;

            // Determine requested image category from macro parameter (Poster/Backdrop/Thumbnail/Profile/Full)
            // Width/height continue to follow card_size; this only controls the server fetch category.
            let image_size = ferrex_core::player_prelude::ImageSize::$image_type;

            //// Map priority if provided
            let priority = ferrex_core::player_prelude::Priority::Preload;
            $(let priority = $priority;)?

            // Create the image widget
            let mut img = image_for($id)
                .size(image_size)
                .image_type(ferrex_core::player_prelude::ImageType::$card_type)
                .radius(radius)
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
                .skip_request(true)
                .is_hovered(is_hovered)
                .on_play($play_msg)
                .on_click($click_msg);

            // Add loading text if provided
            $(img = img.placeholder_text($loading_text);)?

            // Add theme color if available
            use $crate::domains::ui::views::grid::macros::ThemeColorAccess;
            if let Some(theme_color_str) = $data.theme_color() {
                //log::info!("Card for {} has theme_color: {}", $title, theme_color_str);
                if let Ok(color) = $crate::domains::ui::views::grid::macros::parse_hex_color(theme_color_str) {
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
            .on_enter($crate::domains::ui::messages::UiMessage::MediaHovered($image_key))
            .on_exit($crate::domains::ui::messages::UiMessage::MediaUnhovered($image_key));

        // Create the poster element
        // Always wrap in button for non-hover clicks, but the shader handles its own overlay buttons
        let poster_element: Element<'_, $crate::domains::ui::messages::UiMessage> = button(image_with_hover)
            .on_press($click_msg)
            .padding(0)
            .style(theme::Button::MediaCard.style())
            .into();

        // Calculate container dimensions including animation padding so layout width
        // matches the shader's animated bounds for all animation types.
        use $crate::infra::constants::animation;
        let h_padding = animation::calculate_horizontal_padding(width);
        let v_padding = animation::calculate_vertical_padding(height);
        let container_width = width + h_padding * 2.0;
        let container_height = height + v_padding * 2.0;

        // Center the poster within the container to account for padding
        let poster_with_overlay_element: Element<'_, $crate::domains::ui::messages::UiMessage> = container(poster_element)
            .width(Length::Fixed(container_width))
            .height(Length::Fixed(container_height))
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .into();

        // Calculate max characters based on card width and text size
        // Rough estimate: ~7-8 pixels per character for typical fonts
        let title_max_chars = ((width - 10.0) / (title_size as f32 * 0.6)) as usize;
        let subtitle_max_chars = ((width - 10.0) / (subtitle_size as f32 * 0.6)) as usize;

        // Truncate title and subtitle to prevent wrapping
        let truncated_title = $crate::domains::ui::views::grid::macros::truncate_text($title, title_max_chars);
        let truncated_subtitle = $crate::domains::ui::views::grid::macros::truncate_text($subtitle, subtitle_max_chars);

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
