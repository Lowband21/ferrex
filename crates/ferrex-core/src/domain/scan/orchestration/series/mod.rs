pub mod folder_clues;
pub mod locator;
pub mod naming;
pub mod resolver;

pub use folder_clues::SeriesFolderClues;
pub use locator::SeriesLocator;
pub use naming::{
    clean_series_title, collapse_whitespace, slugify_series_title,
};
pub use resolver::{
    DefaultSeriesResolver, SeriesMetadataProvider, SeriesResolution,
    SeriesResolverPort,
};
