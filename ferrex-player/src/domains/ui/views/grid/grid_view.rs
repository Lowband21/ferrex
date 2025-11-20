use crate::domains::ui::messages::Message;
use crate::infrastructure::constants::{calculations, grid, poster, scale_presets};
use ferrex_core::{MovieReference, SeriesReference};
use iced::{
    widget::{column, container, row},
    Element, Length,
};

/// Calculate grid layout parameters for consistent spacing
fn calculate_grid_layout(window_width: f32) -> (usize, f32) {
    // Calculate columns using centralized logic with default scale
    let columns = calculations::calculate_columns(window_width, scale_presets::DEFAULT_SCALE);

    // Calculate padding for centered layout
    let padding =
        calculations::calculate_grid_padding(window_width, columns, scale_presets::DEFAULT_SCALE);

    (columns, padding)
}

/// Creates a grid from a list of elements
fn create_grid<'a>(
    items: Vec<Element<'a, Message>>,
    items_per_row: usize,
    spacing: f32,
) -> Element<'a, Message> {
    let mut rows = column![].spacing(grid::ROW_SPACING);
    let mut current_row = row![].spacing(spacing);

    let total_items = items.len();
    for (i, item) in items.into_iter().enumerate() {
        current_row = current_row.push(item);

        if (i + 1) % items_per_row == 0 || i == total_items - 1 {
            rows = rows.push(
                container(current_row)
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Center),
            );
            current_row = row![].spacing(spacing);
        }
    }

    rows.into()
}

// Helper functions with explicit lifetimes for the macro
fn create_movie_card<'a>(
    movie: &'a MovieReference,
    hovered_media_id: &Option<String>,
    is_visible: bool,
    state: &'a crate::state_refactored::State,
) -> Element<'a, Message> {
    let is_hovered = hovered_media_id
        .as_ref()
        .map(|id| id == movie.id.as_str())
        .unwrap_or(false);
    super::super::super::components::movie_reference_card_with_state(
        movie,
        is_hovered,
        is_visible,
        Some(state),
    )
}

fn create_series_card<'a>(
    series: &'a SeriesReference,
    hovered_media_id: &Option<String>,
    is_visible: bool,
    state: &'a crate::state_refactored::State,
) -> Element<'a, Message> {
    let is_hovered = hovered_media_id
        .as_ref()
        .map(|id| id == series.id.as_str())
        .unwrap_or(false);
    crate::domains::ui::components::series_reference_card_with_state(
        series,
        is_hovered,
        is_visible,
        Some(state),
    )
}

// Use the macro to generate virtual grid functions
virtual_reference_grid!(
    virtual_movie_references_grid,
    MovieReference,
    create_movie_card,
    "virtual_movie_references_grid"
);

virtual_reference_grid!(
    virtual_series_references_grid,
    SeriesReference,
    create_series_card,
    "virtual_series_references_grid"
);
