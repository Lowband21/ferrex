//! ViewModel for the "All" view that shows carousels

use std::sync::{Arc, RwLock, Weak};
use uuid::Uuid;

use crate::{
    api_types::{MediaId, MovieReference, SeriesReference},
    media_store::{MediaChangeEvent, MediaStore, MediaStoreSubscriber},
    metadata_service::FetchPriority,
    view_models::{MetadataNeeds, ViewModel, VisibleItems},
    views::carousel::CarouselState,
};

/// ViewModel for the All view (shows movie and TV carousels)
#[derive(Debug)]
pub struct AllViewModel {
    /// Reference to the media store
    store: Arc<RwLock<MediaStore>>,

    /// Current library filter (None = all libraries)
    library_id: Option<Uuid>,

    /// Cached sorted movies
    sorted_movies: Vec<MovieReference>,

    /// Cached sorted series
    sorted_series: Vec<SeriesReference>,

    /// Carousel state for movies
    movies_carousel: CarouselState,

    /// Carousel state for TV shows
    tv_carousel: CarouselState,

    /// Flag indicating data needs refresh
    needs_refresh: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl AllViewModel {
    /// Create a new AllViewModel
    pub fn new(store: Arc<RwLock<MediaStore>>) -> Self {
        let mut vm = Self {
            store,
            library_id: None,
            sorted_movies: Vec::new(),
            sorted_series: Vec::new(),
            movies_carousel: CarouselState::new(0),
            tv_carousel: CarouselState::new(0),
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
    pub fn all_movies(&self) -> &[MovieReference] {
        &self.sorted_movies
    }

    /// Get all sorted series (for rendering)
    pub fn all_series(&self) -> &[SeriesReference] {
        &self.sorted_series
    }

    /// Force a refresh from the store on next access
    pub fn mark_needs_refresh(&self) {
        self.needs_refresh
            .store(true, std::sync::atomic::Ordering::Release);
    }
}

impl ViewModel for AllViewModel {
    fn refresh_from_store(&mut self) {
        let needs_refresh = self
            .needs_refresh
            .load(std::sync::atomic::Ordering::Acquire);
        log::info!(
            "AllViewModel::refresh_from_store called, needs_refresh={}, library_id={:?}",
            needs_refresh,
            self.library_id
        );

        // TEMPORARY: Always refresh to debug issue
        // if !needs_refresh {
        //     return;
        // }

        // Get data from store
        if let Ok(store) = self.store.read() {
            log::info!("AllViewModel: MediaStore has {} total items", store.len());

            // Get movies
            let mut movies: Vec<MovieReference> = store
                .get_movies(self.library_id)
                .into_iter()
                .cloned()
                .collect();

            // Sort by title
            movies.sort_by(|a, b| a.title.as_str().cmp(b.title.as_str()));

            // Get series
            let mut series: Vec<SeriesReference> = store
                .get_series(self.library_id)
                .into_iter()
                .cloned()
                .collect();

            // Sort by title
            series.sort_by(|a, b| a.title.as_str().cmp(b.title.as_str()));

            log::info!(
                "AllViewModel: Refreshed with {} movies and {} series from store (library_id={:?})",
                movies.len(),
                series.len(),
                self.library_id
            );

            // Update cached data
            self.sorted_movies = movies;
            self.sorted_series = series;

            // Update carousel item counts
            self.movies_carousel
                .set_total_items(self.sorted_movies.len());
            self.tv_carousel.set_total_items(self.sorted_series.len());

            self.needs_refresh
                .store(false, std::sync::atomic::Ordering::Release);
        }
    }

    fn get_visible_items(&self) -> VisibleItems {
        // Get visible range from carousels
        let movie_range = self.movies_carousel.get_visible_range();
        let series_range = self.tv_carousel.get_visible_range();

        // Extract visible items
        let visible_movies: Vec<MovieReference> = movie_range
            .filter_map(|idx| self.sorted_movies.get(idx).cloned())
            .collect();

        let visible_series: Vec<SeriesReference> = series_range
            .filter_map(|idx| self.sorted_series.get(idx).cloned())
            .collect();

        VisibleItems {
            movies: visible_movies,
            series: visible_series,
        }
    }

    fn get_metadata_needs(&self) -> MetadataNeeds {
        let mut items = Vec::new();

        // Get visible ranges
        let movie_range = self.movies_carousel.get_visible_range();
        let series_range = self.tv_carousel.get_visible_range();

        // Check visible movies
        for idx in movie_range {
            if let Some(movie) = self.sorted_movies.get(idx) {
                if crate::api_types::needs_details_fetch(&movie.details) {
                    items.push((MediaId::Movie(movie.id.clone()), FetchPriority::High));
                }
            }
        }

        // Check visible series
        for idx in series_range {
            if let Some(series) = self.sorted_series.get(idx) {
                if crate::api_types::needs_details_fetch(&series.details) {
                    items.push((MediaId::Series(series.id.clone()), FetchPriority::High));
                }
            }
        }

        // Add preload items at medium priority
        // Movies preload (next page)
        let movie_preload_start = self.movies_carousel.visible_end;
        let movie_preload_end = (movie_preload_start + self.movies_carousel.items_per_page)
            .min(self.sorted_movies.len());

        for idx in movie_preload_start..movie_preload_end {
            if let Some(movie) = self.sorted_movies.get(idx) {
                if crate::api_types::needs_details_fetch(&movie.details) {
                    items.push((MediaId::Movie(movie.id.clone()), FetchPriority::Medium));
                }
            }
        }

        // Series preload (next page)
        let series_preload_start = self.tv_carousel.visible_end;
        let series_preload_end =
            (series_preload_start + self.tv_carousel.items_per_page).min(self.sorted_series.len());

        for idx in series_preload_start..series_preload_end {
            if let Some(series) = self.sorted_series.get(idx) {
                if crate::api_types::needs_details_fetch(&series.details) {
                    items.push((MediaId::Series(series.id.clone()), FetchPriority::Medium));
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

impl MediaStoreSubscriber for AllViewModel {
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
