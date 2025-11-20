//! View Models for transforming store data for specific views

pub mod all_view_model;

pub use all_view_model::AllViewModel;

use crate::domains::metadata::service::FetchPriority;
use crate::infrastructure::api_types::MediaId;

/// Items that are visible in a view and their priority for metadata fetching
#[derive(Debug, Clone)]
pub struct VisibleItems {
    pub movies: Vec<crate::infrastructure::api_types::MovieReference>,
    pub series: Vec<crate::infrastructure::api_types::SeriesReference>,
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
