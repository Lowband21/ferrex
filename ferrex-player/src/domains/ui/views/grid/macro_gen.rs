use crate::domains::ui::views::grid::{create_movie_card, create_series_card};
use iced::widget::{column, row};
use uuid::Uuid;

// Use the macro to generate virtual grid functions
virtual_reference_grid!(
    virtual_movie_references_grid,
    &'a ArchivedMovieReference,
    create_movie_card,
    "virtual_movie_references_grid"
);

virtual_reference_grid!(
    virtual_series_references_grid,
    &'a ArchivedSeriesReference,
    create_series_card,
    "virtual_series_references_grid"
);
