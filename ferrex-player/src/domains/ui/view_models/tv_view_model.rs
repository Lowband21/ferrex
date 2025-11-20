//! ViewModel for the TV Shows grid view

use std::sync::{Arc, RwLock};
use uuid::Uuid;

use super::{MetadataNeeds, ViewModel, VisibleItems};
use crate::{
    domains::media::store::{MediaChangeEvent, MediaStore, MediaStoreSubscriber},
    domains::metadata::service::FetchPriority,
    domains::ui::views::grid::virtual_list::VirtualGridState,
    infrastructure::api_types::{MediaId, MovieReference, SeriesReference},
};

/// ViewModel for the TV Shows grid view
#[derive(Debug)]
pub struct TvViewModel {
    /// Reference to the media store
    store: Arc<RwLock<MediaStore>>,

    /// Current library filter (None = all libraries)
    library_id: Option<Uuid>,

    /// Series IDs in sorted order (lightweight indices, not cloned data)
    sorted_series_ids: Vec<MediaId>,
    
    /// Cached series for rendering (only cloned when IDs change)
    cached_series: Vec<SeriesReference>,

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
            sorted_series_ids: Vec::new(),
            cached_series: Vec::new(),
            grid_state: VirtualGridState::new(
                0,
                5,
                crate::infrastructure::constants::virtual_grid::ROW_HEIGHT,
            ),
            needs_refresh: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
        };

        // Initial load from store
        vm.refresh_from_store();

        vm
    }

    /// Set the library filter
    pub fn set_library_filter(&mut self, library_id: Option<Uuid>) {
        log::info!("TvViewModel::set_library_filter called with {:?} (was {:?})", library_id, self.library_id);
        if self.library_id != library_id {
            self.library_id = library_id;
            self.mark_needs_refresh();  // Important: mark that we need refresh before calling refresh_from_store
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
    /// Returns reference to cached series
    pub fn all_series(&self) -> &[SeriesReference] {
        &self.cached_series
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
    
    /// Get current library filter (for debugging)
    pub fn current_library_filter(&self) -> Option<Uuid> {
        self.library_id
    }
}

impl ViewModel for TvViewModel {
    fn refresh_from_store(&mut self) {
        if !self.needs_refresh.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }

        // Get data from store - only store IDs, not cloned data
        if let Ok(store) = self.store.read() {
            log::info!("TvViewModel::refresh_from_store - library_id: {:?}", self.library_id);
            
            // Get series and extract just their IDs (lightweight operation)
            let series_refs = store.get_series(self.library_id);
            log::info!(
                "TvViewModel: get_series({:?}) returned {} references",
                self.library_id,
                series_refs.len()
            );

            // Store only IDs - no cloning of full SeriesReference objects
            let new_ids: Vec<MediaId> = series_refs
                .iter()
                .map(|series| MediaId::Series(series.id.clone()))
                .collect();

            // Only update cached series if IDs changed
            if new_ids != self.sorted_series_ids {
                log::trace!(
                    "TvViewModel: IDs changed, updating cached series (cloning {} items)",
                    series_refs.len()
                );
                self.cached_series = series_refs.into_iter().cloned().collect();
                self.sorted_series_ids = new_ids;
            } else {
                log::trace!(
                    "TvViewModel: IDs unchanged, keeping cached series"
                );
            }

            // Update grid state
            self.grid_state.total_items = self.sorted_series_ids.len();
            self.grid_state.calculate_visible_range();

            self.needs_refresh
                .store(false, std::sync::atomic::Ordering::Release);
        }
    }

    fn get_visible_items(&self) -> VisibleItems {
        // Get visible range from grid and only clone visible items
        let visible_series: Vec<SeriesReference> = if let Ok(store) = self.store.read() {
            self.grid_state
                .visible_range
                .clone()
                .filter_map(|idx| self.sorted_series_ids.get(idx))
                .filter_map(|id| store.get(id))
                .filter_map(|media| media.as_series())
                .cloned()
                .collect()
        } else {
            Vec::new()
        };

        VisibleItems {
            movies: Vec::new(), // TV view doesn't show movies
            series: visible_series,
        }
    }

    fn get_metadata_needs(&self) -> MetadataNeeds {
        let mut items = Vec::new();

        // Only acquire lock once for all metadata checks
        if let Ok(store) = self.store.read() {
            // Check visible series (HIGH priority)
            for idx in self.grid_state.visible_range.clone() {
                if let Some(id) = self.sorted_series_ids.get(idx) {
                    if let Some(media) = store.get(id) {
                        if let Some(series) = media.as_series() {
                            if crate::infrastructure::api_types::needs_details_fetch(&series.details) {
                                items.push((id.clone(), FetchPriority::High));
                            }
                        }
                    }
                }
            }

            // Preload range (MEDIUM priority)
            let preload_range = self.grid_state.get_preload_range(2);
            for idx in preload_range {
                if let Some(id) = self.sorted_series_ids.get(idx) {
                    if let Some(media) = store.get(id) {
                        if let Some(series) = media.as_series() {
                            if crate::infrastructure::api_types::needs_details_fetch(&series.details) {
                                items.push((id.clone(), FetchPriority::Medium));
                            }
                        }
                    }
                }
            }

        // Items below current view (LOW priority)
        if !self.grid_state.visible_range.is_empty() {
            let visible_end = self.grid_state.visible_range.end;
            let preload_below_end =
                (visible_end + self.grid_state.columns * 4).min(self.sorted_series_ids.len());

            for idx in visible_end..preload_below_end {
                if let Some(id) = self.sorted_series_ids.get(idx) {
                    if let Some(media) = store.get(id) {
                        if let Some(series) = media.as_series() {
                            if crate::infrastructure::api_types::needs_details_fetch(&series.details) {
                                items.push((id.clone(), FetchPriority::Low));
                            }
                        }
                    }
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
