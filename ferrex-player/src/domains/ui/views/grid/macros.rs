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
            use $crate::infrastructure::profiling::PROFILER;

            PROFILER.start($profiler_label);

            log::trace!("{}: rendering {} items", $profiler_label, items.len());
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
                        let is_visible = item_idx >= grid_state.visible_range.start
                            && item_idx < grid_state.visible_range.end;

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
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::default(),
            ))
            .on_scroll(on_scroll)
            .width(Length::Fill)
            .height(Length::Fill)
            .style($crate::domains::ui::theme::Scrollable::style());

            PROFILER.end($profiler_label);
            scrollable_content.into()
        }
    };
}
