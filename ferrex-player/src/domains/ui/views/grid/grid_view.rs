use crate::domains::ui::messages::Message;
use crate::infrastructure::api_types::{MovieReference, SeriesReference, WatchProgress};
use iced::{
    widget::{column, row},
    Element,
};
use tokio::sync::watch;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
fn create_movie_card<'a>(
    movie: &'a MovieReference,
    hovered_media_id: &Option<String>,
    is_visible: bool,
    watch_progress: Option<WatchProgress>,
) -> Element<'a, Message> {
    let is_hovered = hovered_media_id
        .as_ref()
        .map(|id| id == movie.id.as_str())
        .unwrap_or(false);
    super::super::super::components::movie_reference_card_with_state(
        movie,
        is_hovered,
        is_visible,
        watch_progress,
    )
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
fn create_series_card<'a>(
    series: &'a SeriesReference,
    hovered_media_id: &Option<String>,
    is_visible: bool,
    watch_progress: Option<WatchProgress>,
) -> Element<'a, Message> {
    let is_hovered = hovered_media_id
        .as_ref()
        .map(|id| id == series.id.as_str())
        .unwrap_or(false);
    crate::domains::ui::components::series_reference_card_with_state(
        series,
        is_hovered,
        is_visible,
        watch_progress,
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
