//! Macros for generating virtual grid views

/// Macro to generate virtual grid views for different reference types
/// This eliminates code duplication between movie and series grids
#[macro_export]
macro_rules! virtual_reference_grid {
    (
        $name:ident,
        $item_type:ty,
        $create_card:expr,
        $profiler_label:literal
    ) => {
        pub fn $name<'a>(
            items: &'a [$item_type],
            grid_state: &$crate::domains::ui::views::grid::virtual_list::VirtualGridState,
            hovered_media_id: &Option<String>,
            on_scroll: impl Fn(iced::widget::scrollable::Viewport) -> $crate::domains::ui::messages::Message + 'a,
            fast_scrolling: bool,
            state: &'a $crate::state_refactored::State,
        ) -> iced::Element<'a, $crate::domains::ui::messages::Message> {
            use iced::{
                widget::{column, container, row, scrollable, text, Space},
                Length,
            };
            use $crate::infrastructure::profiling_scopes::scopes;

            // Scroll-aware profiling: sample less during scrolling to reduce overhead
            let is_scrolling = if let Some(last_scroll) = state.domains.ui.state.last_scroll_time {
                let elapsed = last_scroll.elapsed();
                elapsed < std::time::Duration::from_millis(
                    $crate::infrastructure::performance_config::scrolling::SCROLL_STOP_DEBOUNCE_MS
                )
            } else {
                false
            };

            // Profile grid rendering operations
            #[cfg(any(feature = "profile-with-puffin", feature = "profile-with-tracy", feature = "profile-with-tracing"))]
            profiling::scope!(scopes::GRID_RENDER);
            
            let layout_start = std::time::Instant::now();

            log::trace!("{}: rendering {} items (scroll={})", 
                      $profiler_label, items.len(), is_scrolling);
            if !items.is_empty() {
                log::trace!("First item in grid");
            }

            use $crate::infrastructure::constants::{grid, poster};

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
            content = content.push(Space::with_height(grid::TOP_PADDING));

            // Calculate total rows
            let total_rows = (items.len() + grid_state.columns - 1) / grid_state.columns;

            // Add spacer for rows above viewport
            let start_row = grid_state.visible_range.start / grid_state.columns;
            if start_row > 0 {
                let spacer_height = start_row as f32 * grid_state.row_height;
                content = content.push(Space::with_height(Length::Fixed(spacer_height)));
            }

            // Render visible rows
            let end_row =
                (grid_state.visible_range.end + grid_state.columns - 1) / grid_state.columns;
            for row_idx in start_row..end_row.min(total_rows) {
                let mut row_content = row![].spacing(grid::EFFECTIVE_SPACING);

                for col in 0..grid_state.columns {
                    let item_idx = row_idx * grid_state.columns + col;
                    if item_idx < items.len() && item_idx < grid_state.visible_range.end {
                        let item = &items[item_idx];

                        // Determine if this item is truly visible (not just in overscan area)
                        // During scrolling, NEVER mark items as visible to ensure they get Preload priority
                        // Check if we're within the debounce window of the last scroll event
                        let is_scrolling = if let Some(last_scroll) = state.domains.ui.state.last_scroll_time {
                            let elapsed = last_scroll.elapsed();
                            elapsed < std::time::Duration::from_millis(
                                $crate::infrastructure::performance_config::scrolling::SCROLL_STOP_DEBOUNCE_MS
                            )
                        } else {
                            false
                        };

                        let is_visible = if is_scrolling {
                            false  // Always use Preload priority while scrolling
                        } else {
                            item_idx >= grid_state.visible_range.start
                                && item_idx < grid_state.visible_range.end
                        };

                        /*
                        // Log priority decision for first item in range for debugging
                        if item_idx == grid_state.visible_range.start {
                            let _elapsed_ms = state.domains.ui.state.last_scroll_time
                                .map(|t| t.elapsed().as_millis())
                                .unwrap_or(999999);
                            log::debug!("Grid item priority: scrolling={}, elapsed={}ms, is_visible={} -> priority={}",
                                is_scrolling,
                                elapsed_ms,
                                is_visible,
                                if is_visible { "VISIBLE" } else { "PRELOAD" });
                        }
                        */

                        // Profile poster loading for visible items
                        #[cfg(any(feature = "profile-with-puffin", feature = "profile-with-tracy", feature = "profile-with-tracing"))]
                        if is_visible {
                            profiling::scope!(scopes::POSTER_LOAD);
                        }
                        
                        // Call the card creation function with visibility info
                        let card = $create_card(item, hovered_media_id, is_visible, state);

                        // Use container dimensions that account for animation padding
                        let (container_width, container_height) =
                            $crate::infrastructure::constants::calculations::get_container_dimensions(1.0);

                        // Debug logging to verify container dimensions
                        //if item_idx == 0 {
                        //    log::info!("Grid container dimensions: {}x{} (includes animation padding)", container_width, container_height);
                        //    log::info!("Base poster size: {}x{}", poster::BASE_WIDTH, poster::BASE_HEIGHT);
                        //    log::info!("Horizontal animation padding per side: {}", $crate::constants::animation::calculate_horizontal_padding(poster::BASE_WIDTH));
                        //}

                        // Use total card height with animation padding
                        let total_card_height = poster::TOTAL_CARD_HEIGHT
                            + 2.0 * $crate::infrastructure::constants::animation::calculate_vertical_padding(poster::BASE_HEIGHT);

                        row_content = row_content.push(
                            container(card)
                                .width(Length::Fixed(container_width))
                                .height(Length::Fixed(total_card_height))
                                .clip(false),
                        );
                    } else if item_idx < items.len() {
                        // Placeholder for items not yet visible but in the row
                        let (container_width, _) =
                            $crate::infrastructure::constants::calculations::get_container_dimensions(1.0);
                        let total_card_height = poster::TOTAL_CARD_HEIGHT
                            + 2.0 * $crate::infrastructure::constants::animation::calculate_vertical_padding(poster::BASE_HEIGHT);
                        row_content = row_content.push(
                            container(Space::new(
                                container_width,
                                total_card_height,
                            ))
                            .style($crate::domains::ui::theme::Container::Default.style()),
                        );
                    }
                }

                // Fill remaining columns with empty space only if this is the last row and it's incomplete
                if row_idx == total_rows - 1 {
                    let items_in_last_row = items.len() - (row_idx * grid_state.columns);
                    if items_in_last_row < grid_state.columns {
                        for _ in items_in_last_row..grid_state.columns {
                            let (container_width, _) =
                                $crate::infrastructure::constants::calculations::get_container_dimensions(1.0);
                            row_content = row_content.push(Space::with_width(container_width));
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
                content = content.push(Space::with_height(Length::Fixed(spacer_height)));
            }

            // Add some padding at the bottom
            content = content.push(Space::with_height(grid::BOTTOM_PADDING));

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
            .on_scroll(on_scroll)
            .width(Length::Fill)
            .height(Length::Fill)
            .style($crate::domains::ui::theme::Scrollable::style());

            // Log if layout took too long
            let layout_duration = layout_start.elapsed();
            $crate::infrastructure::profiling_scopes::log_if_slow(scopes::GRID_LAYOUT, layout_duration);
            
            // Profiling scopes will automatically close
            scrollable_content.into()
        }
    };
}
