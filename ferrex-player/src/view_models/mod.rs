//! View Models for transforming store data for specific views

pub mod all_view_model;
pub mod movies_view_model;
pub mod tv_view_model;

pub use all_view_model::AllViewModel;
pub use movies_view_model::MoviesViewModel;
pub use tv_view_model::TvViewModel;

use crate::api_types::MediaId;
use crate::metadata_service::FetchPriority;

/// Items that are visible in a view and their priority for metadata fetching
#[derive(Debug, Clone)]
pub struct VisibleItems {
    pub movies: Vec<crate::api_types::MovieReference>,
    pub series: Vec<crate::api_types::SeriesReference>,
}

/// Items that need metadata fetched with their priorities
#[derive(Debug, Clone)]
pub struct MetadataNeeds {
    pub items: Vec<(MediaId, FetchPriority)>,
}

/// Common trait for all view models
pub trait ViewModel: Send + Sync {
    /// Update the view model from the media store
    fn refresh_from_store(&mut self);

    /// Get the currently visible items
    fn get_visible_items(&self) -> VisibleItems;

    /// Get items that need metadata with their fetch priorities
    fn get_metadata_needs(&self) -> MetadataNeeds;

    /// Handle scroll or other view changes that affect visibility
    fn update_visibility(&mut self);
}
