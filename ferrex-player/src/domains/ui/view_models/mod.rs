//! View Models for transforming store data for specific views

pub mod home_view_model;

use ferrex_core::player_prelude::Media;
pub use home_view_model::HomeViewModel;

//use crate::domains::metadata::service::FetchPriority;

/// Items that are visible in a view and their priority for metadata fetching
#[derive(Debug, Clone)]
pub struct VisibleItems {
    pub movies: Vec<Media>,
    pub series: Vec<Media>,
}

// Items that need metadata fetched with their priorities
/*
#[derive(Debug, Clone)]
pub struct MetadataNeeds {
    pub items: Vec<(MediaID, FetchPriority)>,
} */

/// Common trait for all view models
pub trait ViewModel: Send + Sync {
    /// Update the view model from the media store
    //fn refresh_from_repo(&mut self);
    /// Get the currently visible items
    fn get_visible_items(&self) -> VisibleItems;

    /*
    /// Get items that need metadata with their fetch priorities
    fn get_metadata_needs(&self) -> MetadataNeeds;

    /// Handle scroll or other view changes that affect visibility
    fn update_visibility(&mut self); */
}
