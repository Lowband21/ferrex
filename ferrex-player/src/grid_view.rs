use crate::{
    components::default_movie_poster, media_library::MediaFile, models::TvShow,
    poster_cache::PosterCache, theme, virtual_list, widgets::AnimationType, Message,
};
use iced::{
    alignment,
    widget::{column, container, row, scrollable, text, Space},
    Element, Length,
};
use std::collections::{HashMap, HashSet};

/// Creates a responsive grid view of media items with new media sections
pub fn media_grid_view<'a>(
    movies: &'a [MediaFile],
    tv_shows: &'a HashMap<String, TvShow>,
    poster_cache: &'a PosterCache,
    window_width: f32,
    sort_by: crate::SortBy,
    _sort_order: crate::SortOrder,
    show_new_media: bool,
) -> Element<'a, Message> {
    let mut content = column![].spacing(30).padding(20);

    // Calculate items per row based on window width
    let item_width = 200.0;
    let item_spacing = 50.0;
    let padding = 100.0; // 20px on each side
    let available_width = window_width - padding;
    let items_per_row =
        ((available_width + item_spacing) / (item_width + item_spacing)).floor() as usize;
    let items_per_row = items_per_row.max(2).min(8); // Between 2 and 8 items per row

    // New media sections (when in All view)
    if show_new_media && (sort_by == crate::SortBy::DateAdded) {
        // Get recently added TV shows (most recent episodes)
        let recent_tv_episodes: Vec<_> = tv_shows
            .values()
            .flat_map(|show| {
                show.seasons
                    .values()
                    .flat_map(|season| season.episodes.values())
                    .map(move |ep| (show, ep))
            })
            .collect::<Vec<_>>();

        let mut sorted_tv_episodes = recent_tv_episodes;
        sorted_tv_episodes.sort_by(|a, b| b.1.created_at.cmp(&a.1.created_at));

        // Take up to 10 recent TV episodes, but only show unique shows
        let mut shown_shows = std::collections::HashSet::new();
        let recent_shows: Vec<_> = sorted_tv_episodes
            .into_iter()
            .filter(|(show, _)| shown_shows.insert(show.name.clone()))
            .take(10)
            .map(|(show, _)| show)
            .collect();

        let has_recent_shows = !recent_shows.is_empty();
        if has_recent_shows {
            content = content.push(
                text("New TV Shows")
                    .size(24)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
            );

            content = content.push(create_grid(
                recent_shows
                    .into_iter()
                    .map(|show| crate::components::tv_show_card(show, poster_cache, false))
                    .collect(),
                items_per_row,
                item_spacing,
            ));
        }

        // Get recently added movies
        let recent_movies: Vec<_> = movies.iter().take(10).collect();

        if !recent_movies.is_empty() {
            if has_recent_shows {
                content = content.push(Space::with_height(20));
            }

            content = content.push(
                text("New Movies")
                    .size(24)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
            );

            content = content.push(create_grid(
                recent_movies
                    .into_iter()
                    .map(|movie| crate::components::movie_card(movie, poster_cache, false))
                    .collect(),
                items_per_row,
                item_spacing,
            ));

            content = content.push(Space::with_height(30));
        }
    }

    // TV Shows section
    if !tv_shows.is_empty() {
        content = content.push(
            text("TV Shows")
                .size(24)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
        );

        let mut sorted_shows: Vec<_> = tv_shows.values().collect();
        sorted_shows.sort_by_key(|s| &s.name);

        content = content.push(create_grid(
            sorted_shows
                .into_iter()
                .map(|show| crate::components::tv_show_card(show, poster_cache, false))
                .collect(),
            items_per_row,
            item_spacing,
        ));
    }

    // Movies section
    if !movies.is_empty() {
        if !tv_shows.is_empty() {
            content = content.push(Space::with_height(20));
        }

        content = content.push(
            text("Movies")
                .size(24)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
        );

        content = content.push(create_grid(
            movies
                .iter()
                .map(|movie| crate::components::movie_card(movie, poster_cache, false))
                .collect(),
            items_per_row,
            item_spacing,
        ));
    }

    // Add some padding at the bottom
    content = content.push(Space::with_height(50));

    // Wrap in scrollable
    scrollable(
        container(content)
            .width(Length::Fill)
            .height(Length::Shrink)
            .padding(0),
    )
    .direction(scrollable::Direction::Vertical(
        scrollable::Scrollbar::default(),
    ))
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Creates a grid layout from a vector of elements
fn create_grid<'a>(
    items: Vec<Element<'a, Message>>,
    items_per_row: usize,
    spacing: f32,
) -> Element<'a, Message> {
    let mut rows = Vec::new();
    let mut current_row = Vec::new();
    let total_items = items.len();

    for (i, item) in items.into_iter().enumerate() {
        current_row.push(item);

        if current_row.len() >= items_per_row || i == total_items - 1 {
            // Track how many items are in this row before draining
            let items_in_row = current_row.len();

            // Create row with proper spacing
            let mut row_content = row![].spacing(spacing);
            for item in current_row.drain(..) {
                row_content = row_content.push(item);
            }

            // Add empty spaces to align last row properly if it's the last row
            if i == total_items - 1 && items_in_row < items_per_row {
                for _ in items_in_row..items_per_row {
                    row_content = row_content.push(Space::with_width(Length::Fixed(200.0)));
                }
            }

            rows.push(row_content.into());
        }
    }

    column(rows).spacing(spacing).width(Length::Fill).into()
}

/// Creates a single scrollable grid view without sections
pub fn simple_media_grid<'a>(
    items: Vec<Element<'a, Message>>,
    window_width: f32,
) -> Element<'a, Message> {
    // Calculate items per row based on window width
    let item_width = 200.0;
    let item_spacing = 15.0;
    let padding = 40.0; // 20px on each side
    let available_width = window_width - padding;
    let items_per_row =
        ((available_width + item_spacing) / (item_width + item_spacing)).floor() as usize;
    let items_per_row = items_per_row.max(2).min(8); // Between 2 and 8 items per row

    let grid = create_grid(items, items_per_row, item_spacing);

    scrollable(container(grid).width(Length::Fill).padding(20))
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::default(),
        ))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Creates a virtual grid view with lazy loading
pub fn virtual_media_grid<'a>(
    items: &'a [MediaFile],
    grid_state: &virtual_list::VirtualGridState,
    poster_cache: &'a PosterCache,
    loading_posters: &HashSet<String>,
    hovered_media_id: &Option<String>,
    animation_types: &'a HashMap<String, (AnimationType, std::time::Instant)>,
    on_scroll: impl Fn(scrollable::Viewport) -> Message + 'a,
    fast_scrolling: bool,
) -> Element<'a, Message> {
    use crate::profiling::PROFILER;
    PROFILER.start("virtual_media_grid");

    let mut content = column![].spacing(0).width(Length::Fill);

    // Calculate total rows
    let total_rows = (items.len() + grid_state.columns - 1) / grid_state.columns;

    // Adjust visible range based on scroll speed
    let mut adjusted_state = grid_state.clone();
    if fast_scrolling {
        // Reduce overscan to 0 during fast scrolling for better performance
        adjusted_state.overscan_rows = 0;
        adjusted_state.calculate_visible_range();
    }

    // Add spacer for rows above viewport
    let start_row = adjusted_state.visible_range.start / adjusted_state.columns;
    if start_row > 0 {
        let spacer_height = start_row as f32 * adjusted_state.row_height;
        content = content.push(Space::with_height(Length::Fixed(spacer_height)));
    }

    // Render visible rows
    let end_row =
        (adjusted_state.visible_range.end + adjusted_state.columns - 1) / adjusted_state.columns;
    for row_idx in start_row..end_row.min(total_rows) {
        let mut row_content = row![].spacing(30); //.padding([0, 100]);

        for col in 0..adjusted_state.columns {
            let item_idx = row_idx * adjusted_state.columns + col;
            if item_idx < items.len() && item_idx < adjusted_state.visible_range.end {
                let media = &items[item_idx];
                let is_hovered = hovered_media_id.as_ref() == Some(&media.id);
                // Use fast card variant during fast scrolling
                let card = crate::components::movie_card_lazy(
                    media,
                    poster_cache,
                    is_hovered,
                    loading_posters.contains(&media.id),
                    animation_types,
                );
                row_content = row_content
                    .push(container(card).width(Length::Fixed(adjusted_state.item_width)));
            } else {
                // Empty space for incomplete rows
                row_content =
                    row_content.push(Space::with_width(Length::Fixed(adjusted_state.item_width)));
            }
        }

        content = content.push(
            container(row_content)
                .height(Length::Fixed(adjusted_state.row_height))
                .width(Length::Fill)
                .align_x(alignment::Horizontal::Center)
                .clip(true), // Clip each row to prevent overflow
        );
    }

    // Add spacer for rows below viewport
    let mut remaining_rows = total_rows.saturating_sub(end_row);

    // During fast scrolling, use a simple spacer instead of rendering placeholders
    if fast_scrolling && remaining_rows > 0 {
        let spacer_height = remaining_rows as f32 * adjusted_state.row_height;
        content = content.push(Space::with_height(Length::Fixed(spacer_height)));
    } else {
        // Normal scrolling - render placeholder rows
        while remaining_rows > 0 {
            let mut row_content = row![].spacing(30);
            let mut remaining_cols = adjusted_state.columns;
            while remaining_cols > 0 {
                row_content = row_content.push(default_movie_poster());
                remaining_cols -= 1;
            }
            content = content.push(
                container(row_content)
                    .height(Length::Fixed(adjusted_state.row_height))
                    .width(Length::Fill)
                    .align_x(alignment::Horizontal::Center),
            );
            remaining_rows -= 1;
        }
    }

    // Wrap scrollable in a clipping container to ensure no overflow
    let result = container(
        scrollable(content)
            .id(grid_state.scrollable_id.clone())
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::default(),
            ))
            .on_scroll(on_scroll)
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .clip(true) // Strict clipping for the entire scrollable area
    .into();

    PROFILER.end("virtual_media_grid");
    result
}

/// Creates a virtual grid view for TV shows with lazy loading
pub fn virtual_tv_grid<'a>(
    shows: &'a [TvShow],
    grid_state: &virtual_list::VirtualGridState,
    poster_cache: &'a PosterCache,
    loading_posters: &HashSet<String>,
    hovered_media_id: &Option<String>,
    animation_types: &'a HashMap<String, (AnimationType, std::time::Instant)>,
    on_scroll: impl Fn(scrollable::Viewport) -> Message + 'a,
    fast_scrolling: bool,
) -> Element<'a, Message> {
    let mut content = column![].spacing(0).width(Length::Fill);

    // Calculate total rows
    let total_rows = (shows.len() + grid_state.columns - 1) / grid_state.columns;

    // Adjust visible range based on scroll speed
    let mut adjusted_state = grid_state.clone();
    if fast_scrolling {
        adjusted_state.overscan_rows = 0;
        adjusted_state.calculate_visible_range();
    }

    // Add spacer for rows above viewport
    let start_row = adjusted_state.visible_range.start / adjusted_state.columns;
    if start_row > 0 {
        let spacer_height = start_row as f32 * adjusted_state.row_height;
        content = content.push(Space::with_height(Length::Fixed(spacer_height)));
    }

    // Render visible rows
    let end_row =
        (adjusted_state.visible_range.end + adjusted_state.columns - 1) / adjusted_state.columns;
    for row_idx in start_row..end_row.min(total_rows) {
        let mut row_content = row![].spacing(15.0).padding([0, 20]);

        for col in 0..adjusted_state.columns {
            let item_idx = row_idx * adjusted_state.columns + col;
            if item_idx < shows.len() && item_idx < adjusted_state.visible_range.end {
                let show = &shows[item_idx];
                // Use fast card variant during fast scrolling
                let poster_id = show.get_poster_id().unwrap_or_else(|| show.name.clone());
                let is_hovered = hovered_media_id.as_ref() == Some(&poster_id);
                let card = crate::components::tv_show_card_lazy(
                    show,
                    poster_cache,
                    is_hovered,
                    loading_posters.contains(&show.name),
                    false,
                    animation_types,
                );
                row_content = row_content
                    .push(container(card).width(Length::Fixed(adjusted_state.item_width)));
            } else {
                // Empty space for incomplete rows
                row_content =
                    row_content.push(Space::with_width(Length::Fixed(adjusted_state.item_width)));
            }
        }

        content = content.push(
            container(row_content)
                .height(Length::Fixed(adjusted_state.row_height))
                .width(Length::Fill)
                .clip(true), // Clip to prevent overflow
        );
    }

    // Add spacer for rows below viewport
    let remaining_rows = total_rows.saturating_sub(end_row);
    if remaining_rows > 0 {
        let spacer_height = remaining_rows as f32 * adjusted_state.row_height;
        content = content.push(Space::with_height(Length::Fixed(spacer_height)));
    }

    // Wrap scrollable in a clipping container to ensure no overflow
    container(
        scrollable(content)
            .id(grid_state.scrollable_id.clone())
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::default(),
            ))
            .on_scroll(on_scroll)
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .clip(true) // Strict clipping for the entire scrollable area
    .into()
}
