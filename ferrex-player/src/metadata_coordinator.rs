//! MetadataCoordinator - Centralized metadata fetching coordination
//!
//! This module coordinates metadata fetching between ViewModels and the MetadataService,
//! ensuring proper prioritization and avoiding duplicate requests.

use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::{
    metadata_service::{FetchPriority, MetadataFetchService},
    state::ViewMode,
    view_models::ViewModel,
};

/// Coordinates metadata fetching across the application
#[derive(Debug)]
pub struct MetadataCoordinator {
    /// Reference to the metadata service
    metadata_service: Option<Arc<MetadataFetchService>>,
}

impl MetadataCoordinator {
    /// Create a new MetadataCoordinator
    pub fn new() -> Self {
        Self {
            metadata_service: None,
        }
    }

    /// Initialize with a metadata service
    pub fn set_metadata_service(&mut self, service: Arc<MetadataFetchService>) {
        self.metadata_service = Some(service);
    }

    /// Queue metadata fetching for the current view
    pub fn queue_metadata_for_view(&self, state: &crate::state::State, library_id: Uuid) {
        let Some(service) = &self.metadata_service else {
            log::warn!("MetadataCoordinator: No metadata service available");
            return;
        };

        log::info!(
            "Queuing metadata for {:?} view in library {}",
            state.view_mode,
            library_id
        );

        // Get metadata needs from the appropriate view model
        let metadata_needs = match state.view_mode {
            ViewMode::All => state.all_view_model.get_metadata_needs(),
            ViewMode::Movies => state.movies_view_model.get_metadata_needs(),
            ViewMode::TvShows => state.tv_view_model.get_metadata_needs(),
        };

        log::info!(
            "MetadataCoordinator: Got {} items needing metadata",
            metadata_needs.items.len()
        );

        // Group items by priority
        let mut items_by_priority: std::collections::HashMap<FetchPriority, Vec<_>> =
            std::collections::HashMap::new();

        for (media_id, priority) in metadata_needs.items {
            items_by_priority
                .entry(priority)
                .or_insert_with(Vec::new)
                .push((media_id, library_id));
        }

        // Queue items for each priority level
        for (priority, items) in items_by_priority {
            if !items.is_empty() {
                log::info!(
                    "Queueing {} {:?} priority items for library {}",
                    items.len(),
                    priority,
                    library_id
                );
                service.queue_items(items, priority);
            }
        }
    }

    /// Queue metadata for all views (used after library load)
    pub fn queue_metadata_for_all_views(&self, state: &crate::state::State, library_id: Uuid) {
        let Some(service) = &self.metadata_service else {
            log::warn!("MetadataCoordinator: No metadata service available");
            return;
        };

        log::info!("Queuing metadata for all views in library {}", library_id);

        // Collect all unique metadata needs from all view models
        let mut all_items: std::collections::HashMap<
            (crate::api_types::MediaId, FetchPriority),
            Uuid,
        > = std::collections::HashMap::new();

        // Get needs from AllViewModel
        let needs = state.all_view_model.get_metadata_needs();
        for (media_id, priority) in needs.items {
            all_items.insert((media_id, priority), library_id);
        }

        // Get needs from MoviesViewModel
        let needs = state.movies_view_model.get_metadata_needs();
        for (media_id, priority) in needs.items {
            all_items.insert((media_id, priority), library_id);
        }

        // Get needs from TvViewModel
        let needs = state.tv_view_model.get_metadata_needs();
        for (media_id, priority) in needs.items {
            all_items.insert((media_id, priority), library_id);
        }

        // Group by priority
        let mut items_by_priority: std::collections::HashMap<FetchPriority, Vec<_>> =
            std::collections::HashMap::new();

        for ((media_id, priority), library_id) in all_items {
            items_by_priority
                .entry(priority)
                .or_insert_with(Vec::new)
                .push((media_id, library_id));
        }

        // Queue items
        for (priority, items) in items_by_priority {
            if !items.is_empty() {
                log::info!(
                    "Queueing {} {:?} priority items across all views for library {}",
                    items.len(),
                    priority,
                    library_id
                );
                service.queue_items(items, priority);
            }
        }
    }

    /// Update visibility and queue metadata for the current view
    pub fn update_visibility_and_queue(&self, state: &mut crate::state::State, library_id: Uuid) {
        log::debug!(
            "Updating visibility and queuing metadata for {:?} view",
            state.view_mode
        );

        // Update visibility in the appropriate view model
        match state.view_mode {
            ViewMode::All => {
                state.all_view_model.update_visibility();
            }
            ViewMode::Movies => {
                state.movies_view_model.update_visibility();
            }
            ViewMode::TvShows => {
                state.tv_view_model.update_visibility();
            }
        }

        // Queue metadata for the updated view
        self.queue_metadata_for_view(state, library_id);
    }

    /// Get a reference to the metadata service (for compatibility during migration)
    pub fn metadata_service(&self) -> Option<&Arc<MetadataFetchService>> {
        self.metadata_service.as_ref()
    }
}
