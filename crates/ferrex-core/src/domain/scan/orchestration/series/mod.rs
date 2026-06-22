pub mod bundle_tracker;
pub mod coordinator;
pub mod folder_clues;
pub mod locator;
pub mod naming;
pub mod resolver;

pub use bundle_tracker::{SeriesBundleFinalization, SeriesBundleTracker};
pub use coordinator::{
    EpisodeDependencyDecision, SeriesCoordinator, SeriesDependencyReleaser,
    SeriesDiscoveryOutcome,
};
pub use folder_clues::SeriesFolderClues;
pub use locator::SeriesLocator;
pub use naming::{
    clean_series_title, collapse_whitespace, slugify_series_title,
};
pub use resolver::{
    DefaultSeriesResolver, SeriesMetadataProvider, SeriesResolution,
    SeriesResolverPort,
};
