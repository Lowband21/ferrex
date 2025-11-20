//! ViewModel for the TV Shows grid view

use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::{
    api_types::{MediaId, SeriesReference},
    media_store::{MediaChangeEvent, MediaStore, MediaStoreSubscriber},
    metadata_service::FetchPriority,
    view_models::{MetadataNeeds, ViewModel, VisibleItems},
    views::grid::virtual_list::VirtualGridState,
};

/// ViewModel for the TV Shows grid view
#[derive(Debug)]
pub struct TvViewModel {
    /// Reference to the media store
    store: Arc<RwLock<MediaStore>>,

    /// Current library filter (None = all libraries)
    library_id: Option<Uuid>,

    /// Cached sorted series
    sorted_series: Vec<SeriesReference>,

    /// Virtual grid state for efficient rendering
    grid_state: VirtualGridState,

    /// Flag indicating data needs refresh
    needs_refresh: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl TvViewModel {
    /// Create a new TvViewModel
    pub fn new(store: Arc<RwLock<MediaStore>>) -> Self {
        let mut vm = Self {
            store,
            library_id: None,
            sorted_series: Vec::new(),
            grid_state: VirtualGridState::new(0, 5, crate::constants::virtual_grid::ROW_HEIGHT),
            needs_refresh: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
        };

        // Initial load from store
        vm.refresh_from_store();

        vm
    }

    /// Set the library filter
    pub fn set_library_filter(&mut self, library_id: Option<Uuid>) {
        if self.library_id != library_id {
            self.library_id = library_id;
            self.refresh_from_store();
        }
    }

    /// Update grid state (usually from scroll events)
    pub fn update_grid_state(&mut self, state: VirtualGridState) {
        self.grid_state = state;
    }

    /// Get grid state
    pub fn grid_state(&self) -> &VirtualGridState {
        &self.grid_state
    }

    /// Get all sorted series (for rendering)
    pub fn all_series(&self) -> &[SeriesReference] {
        &self.sorted_series
    }

    /// Update window size (affects grid columns)
    pub fn update_window_size(&mut self, width: f32, height: f32) {
        self.grid_state.viewport_width = width;
        self.grid_state.viewport_height = height;
        self.grid_state.update_columns(width);
        self.grid_state.calculate_visible_range();
    }

    /// Force a refresh from the store on next access
    pub fn mark_needs_refresh(&self) {
        self.needs_refresh
            .store(true, std::sync::atomic::Ordering::Release);
    }
}

impl ViewModel for TvViewModel {
    fn refresh_from_store(&mut self) {
        // TEMPORARY: Always refresh to debug issue
        // if !self.needs_refresh.load(std::sync::atomic::Ordering::Acquire) {
        //     return;
        // }

        // Get data from store
        if let Ok(store) = self.store.read() {
            // Get series
            let mut series: Vec<SeriesReference> = store
                .get_series(self.library_id)
                .into_iter()
                .cloned()
                .collect();

            // Sort by title (could be configurable later)
            series.sort_by(|a, b| a.title.as_str().cmp(b.title.as_str()));

            log::trace!(
                "TvViewModel: Refreshed with {} series from store",
                series.len()
            );

            // Update cached data
            self.sorted_series = series;

            // Update grid state
            self.grid_state.total_items = self.sorted_series.len();
            self.grid_state.calculate_visible_range();

            self.needs_refresh
                .store(false, std::sync::atomic::Ordering::Release);
        }
    }

    fn get_visible_items(&self) -> VisibleItems {
        // Get visible range from grid
        let visible_series: Vec<SeriesReference> = self
            .grid_state
            .visible_range
            .clone()
            .filter_map(|idx| self.sorted_series.get(idx).cloned())
            .collect();

        VisibleItems {
            movies: Vec::new(), // TV view doesn't show movies
            series: visible_series,
        }
    }

    fn get_metadata_needs(&self) -> MetadataNeeds {
        let mut items = Vec::new();

        // Check visible series (HIGH priority)
        for idx in self.grid_state.visible_range.clone() {
            if let Some(series) = self.sorted_series.get(idx) {
                if crate::api_types::needs_details_fetch(&series.details) {
                    items.push((MediaId::Series(series.id.clone()), FetchPriority::High));
                }
            }
        }

        // Preload range (MEDIUM priority)
        let preload_range = self.grid_state.get_preload_range(2);
        for idx in preload_range {
            if let Some(series) = self.sorted_series.get(idx) {
                if crate::api_types::needs_details_fetch(&series.details) {
                    items.push((MediaId::Series(series.id.clone()), FetchPriority::Medium));
                }
            }
        }

        // Items below current view (LOW priority)
        if !self.grid_state.visible_range.is_empty() {
            let visible_end = self.grid_state.visible_range.end;
            let preload_below_end =
                (visible_end + self.grid_state.columns * 4).min(self.sorted_series.len());

            for idx in visible_end..preload_below_end {
                if let Some(series) = self.sorted_series.get(idx) {
                    if crate::api_types::needs_details_fetch(&series.details) {
                        items.push((MediaId::Series(series.id.clone()), FetchPriority::Low));
                    }
                }
            }
        }

        MetadataNeeds { items }
    }

    fn update_visibility(&mut self) {
        // Grid visibility is updated through grid_state
        self.grid_state.calculate_visible_range();
    }
}

impl MediaStoreSubscriber for TvViewModel {
    fn on_media_changed(&self, _event: MediaChangeEvent) {
        // Mark for refresh on next access
        self.needs_refresh
            .store(true, std::sync::atomic::Ordering::Release);
    }

    fn on_batch_complete(&self) {
        // Batch complete - trigger refresh
        self.needs_refresh
            .store(true, std::sync::atomic::Ordering::Release);
    }
}
