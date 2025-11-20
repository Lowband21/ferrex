//! ViewModel for the "All" view that shows carousels

use std::sync::{Arc, RwLock};
use uuid::Uuid;

use super::{MetadataNeeds, ViewModel, VisibleItems};
use crate::{
    domains::media::store::{MediaChangeEvent, MediaStore, MediaStoreSubscriber},
    domains::metadata::service::FetchPriority,
    domains::ui::views::carousel::CarouselState,
    infrastructure::api_types::{MediaId, MovieReference, SeriesReference},
};

/// ViewModel for the All view (shows movie and TV carousels)
#[derive(Debug)]
pub struct AllViewModel {
    /// Reference to the media store
    store: Arc<RwLock<MediaStore>>,

    /// Current library filter (None = all libraries)
    library_id: Option<Uuid>,

    /// Movie IDs in sorted order (lightweight indices, not cloned data)
    sorted_movie_ids: Vec<MediaId>,

    /// Series IDs in sorted order (lightweight indices, not cloned data)
    sorted_series_ids: Vec<MediaId>,
    
    /// Cached movies and series for rendering (only cloned when IDs change)
    cached_movies: Vec<MovieReference>,
    cached_series: Vec<SeriesReference>,

    /// Carousel state for movies
    movies_carousel: CarouselState,

    /// Carousel state for TV shows
    tv_carousel: CarouselState,
}

impl AllViewModel {
    /// Create a new AllViewModel
    pub fn new(store: Arc<RwLock<MediaStore>>) -> Self {
        let mut vm = Self {
            store,
            library_id: None,
            sorted_movie_ids: Vec::new(),
            sorted_series_ids: Vec::new(),
            cached_movies: Vec::new(),
            cached_series: Vec::new(),
            movies_carousel: CarouselState::new(0),
            tv_carousel: CarouselState::new(0),
        };

        // Initial load from store
        vm.refresh_from_store();

        vm
    }

    /// Set the library filter
    pub fn set_library_filter(&mut self, library_id: Option<Uuid>) {
        if self.library_id != library_id {
            self.library_id = library_id;
            self.refresh_from_store();  // AllViewModel doesn't use needs_refresh flag
        }
    }
    
    /// Get current library filter
    pub fn current_library_filter(&self) -> Option<Uuid> {
        self.library_id
    }

    /// Update carousel state for movies
    pub fn update_movies_carousel(&mut self, state: CarouselState) {
        self.movies_carousel = state;
    }

    /// Update carousel state for TV shows
    pub fn update_tv_carousel(&mut self, state: CarouselState) {
        self.tv_carousel = state;
    }

    /// Get movies carousel state
    pub fn movies_carousel(&self) -> &CarouselState {
        &self.movies_carousel
    }

    /// Get TV carousel state
    pub fn tv_carousel(&self) -> &CarouselState {
        &self.tv_carousel
    }

    /// Get all sorted movies (for rendering)
    /// Returns reference to cached movies
    pub fn all_movies(&self) -> &[MovieReference] {
        &self.cached_movies
    }

    /// Get all sorted series (for rendering)
    /// Returns reference to cached series
    pub fn all_series(&self) -> &[SeriesReference] {
        &self.cached_series
    }
}

impl ViewModel for AllViewModel {
    fn refresh_from_store(&mut self) {
        log::info!(
            "AllViewModel::refresh_from_store called, library_id={:?}",
            self.library_id
        );

        // Get data from store - only store IDs, not cloned data
        if let Ok(store) = self.store.read() {
            log::info!("AllViewModel: MediaStore has {} total items", store.len());

            // Get movies and extract just their IDs (lightweight operation)
            let movies_refs = store.get_movies(self.library_id);
            
            // Store only IDs - no cloning of full MovieReference objects
            let new_movie_ids: Vec<MediaId> = movies_refs
                .iter()
                .map(|movie| MediaId::Movie(movie.id.clone()))
                .collect();

            // Get series and extract just their IDs
            let series_refs = store.get_series(self.library_id);
            let new_series_ids: Vec<MediaId> = series_refs
                .iter()
                .map(|series| MediaId::Series(series.id.clone()))
                .collect();

            // Only update cached data if IDs changed
            if new_movie_ids != self.sorted_movie_ids {
                log::trace!("AllViewModel: Movie IDs changed, updating cached movies");
                self.cached_movies = movies_refs.into_iter().cloned().collect();
                self.sorted_movie_ids = new_movie_ids;
            }
            
            if new_series_ids != self.sorted_series_ids {
                log::trace!("AllViewModel: Series IDs changed, updating cached series");
                self.cached_series = series_refs.into_iter().cloned().collect();
                self.sorted_series_ids = new_series_ids;
            }

            log::info!(
                "AllViewModel: Stored {} movie IDs and {} series IDs (library_id={:?})",
                self.sorted_movie_ids.len(),
                self.sorted_series_ids.len(),
                self.library_id
            );

            // Update carousel item counts
            self.movies_carousel
                .set_total_items(self.sorted_movie_ids.len());
            self.tv_carousel.set_total_items(self.sorted_series_ids.len());
        }
    }

    fn get_visible_items(&self) -> VisibleItems {
        // Get visible range from carousels
        let movie_range = self.movies_carousel.get_visible_range();
        let series_range = self.tv_carousel.get_visible_range();

        // Extract visible items
        let visible_movies: Vec<MovieReference> = movie_range
            .filter_map(|idx| self.sorted_movie_ids.get(idx))
            .filter_map(|id| {
                if let Ok(store) = self.store.read() {
                    store.get(id).and_then(|media| media.as_movie()).cloned()
                } else {
                    None
                }
            })
            .collect();

        let visible_series: Vec<SeriesReference> = series_range
            .filter_map(|idx| self.sorted_series_ids.get(idx))
            .filter_map(|id| {
                if let Ok(store) = self.store.read() {
                    store.get(id).and_then(|media| media.as_series()).cloned()
                } else {
                    None
                }
            })
            .collect();

        VisibleItems {
            movies: visible_movies,
            series: visible_series,
        }
    }

    fn get_metadata_needs(&self) -> MetadataNeeds {
        let mut items = Vec::new();

        // Only acquire lock once for all metadata checks
        if let Ok(store) = self.store.read() {
            // Get visible ranges
            let movie_range = self.movies_carousel.get_visible_range();
            let series_range = self.tv_carousel.get_visible_range();

            // Check visible movies
            for idx in movie_range {
                if let Some(id) = self.sorted_movie_ids.get(idx) {
                    if let Some(media) = store.get(id) {
                        if let Some(movie) = media.as_movie() {
                            if crate::infrastructure::api_types::needs_details_fetch(&movie.details) {
                                items.push((id.clone(), FetchPriority::High));
                            }
                        }
                    }
                }
            }

            // Check visible series
            for idx in series_range {
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

            // Add preload items at medium priority
            // Movies preload (next page)
            let movie_preload_start = self.movies_carousel.visible_end;
            let movie_preload_end = (movie_preload_start + self.movies_carousel.items_per_page)
                .min(self.sorted_movie_ids.len());

            for idx in movie_preload_start..movie_preload_end {
                if let Some(id) = self.sorted_movie_ids.get(idx) {
                    if let Some(media) = store.get(id) {
                        if let Some(movie) = media.as_movie() {
                            if crate::infrastructure::api_types::needs_details_fetch(&movie.details) {
                                items.push((id.clone(), FetchPriority::Medium));
                            }
                        }
                    }
                }
            }

            // Series preload (next page)
            let series_preload_start = self.tv_carousel.visible_end;
            let series_preload_end =
                (series_preload_start + self.tv_carousel.items_per_page).min(self.sorted_series_ids.len());

            for idx in series_preload_start..series_preload_end {
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
        }

        MetadataNeeds { items }
    }

    fn update_visibility(&mut self) {
        // Visibility is updated when carousel states change
        // This is called after scroll events
    }
}


