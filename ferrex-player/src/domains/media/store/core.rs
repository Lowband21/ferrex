//! MediaStore - Single source of truth for all media data
//!
//! This module provides a centralized store for all media references with
//! a subscription mechanism for notifying components of changes.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::Instant;
use uuid::Uuid;

use crate::domains::ui::{SortBy, SortOrder};
use crate::infrastructure::api_types::{
    EpisodeReference, MediaDetailsOption, MediaId, MediaReference, MovieReference, SeasonReference,
    SeriesReference, TmdbDetails,
};
use ferrex_core::query::sorting::{
    fields::*,
    strategy::{FieldSort, SortStrategy},
    traits::{SortKey, SortableEntity},
};
use ferrex_core::SeriesID;

/// Types of media for categorization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaType {
    Movie,
    Series,
    Season,
    Episode,
}

impl From<&MediaReference> for MediaType {
    fn from(media: &MediaReference) -> Self {
        // Use media_type() helper method for cleaner code
        match media.media_type() {
            "movie" => MediaType::Movie,
            "series" => MediaType::Series,
            "season" => MediaType::Season,
            "episode" => MediaType::Episode,
            _ => unreachable!("MediaReference always has a valid media type"),
        }
    }
}

/// Change event for subscribers
#[derive(Debug, Clone)]
pub struct MediaChangeEvent {
    pub media_id: MediaId,
    pub media_type: MediaType,
    pub library_id: Uuid,
    pub change_type: ChangeType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    Added,
    Updated,
    Removed,
}

/// Trait for components that want to be notified of media changes
pub trait MediaStoreSubscriber: Send + Sync {
    /// Called when media is added, updated, or removed
    fn on_media_changed(&self, event: MediaChangeEvent);

    /// Called when a batch of changes completes
    fn on_batch_complete(&self);
}

/// Lightweight notifier that tracks when ViewModels need to refresh from MediaStore
#[derive(Debug)]
pub struct MediaStoreNotifier {
    /// Flag indicating ViewModels need refresh
    needs_refresh: Arc<AtomicBool>,
    /// Last time a refresh was triggered (for debouncing)
    last_refresh_time: Arc<Mutex<Instant>>,
    /// Minimum time between refreshes in milliseconds
    debounce_ms: u64,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl MediaStoreNotifier {
    /// Create a new notifier with default 100ms debounce
    pub fn new() -> Self {
        Self::with_debounce(100)
    }

    /// Create a new notifier with custom debounce time
    pub fn with_debounce(debounce_ms: u64) -> Self {
        Self {
            needs_refresh: Arc::new(AtomicBool::new(false)),
            last_refresh_time: Arc::new(Mutex::new(Instant::now())),
            debounce_ms,
        }
    }

    /// Check if ViewModels should refresh (with debouncing)
    pub fn should_refresh(&self) -> bool {
        // Check if refresh is needed
        if !self.needs_refresh.load(Ordering::Acquire) {
            return false;
        }

        // Check debounce timing
        if let Ok(last_time) = self.last_refresh_time.lock() {
            if last_time.elapsed().as_millis() < self.debounce_ms as u128 {
                return false; // Too soon since last refresh
            }
        }

        // Clear the flag and update time
        self.needs_refresh.store(false, Ordering::Release);
        if let Ok(mut last_time) = self.last_refresh_time.lock() {
            *last_time = Instant::now();
        }

        true
    }

    /// Force mark as needing refresh (bypasses debounce check)
    pub fn mark_needs_refresh(&self) {
        self.needs_refresh.store(true, Ordering::Release);
    }
}

impl MediaStoreSubscriber for MediaStoreNotifier {
    fn on_media_changed(&self, _event: MediaChangeEvent) {
        self.needs_refresh.store(true, Ordering::Release);
    }

    fn on_batch_complete(&self) {
        log::debug!("MediaStoreNotifier: Batch complete, marking for refresh");
        self.needs_refresh.store(true, Ordering::Release);
    }
}

/// Single source of truth for all media data
pub struct MediaStore {
    // Core storage
    media: HashMap<MediaId, MediaReference>,

    // Indices for fast lookup
    by_library: HashMap<Uuid, HashSet<MediaId>>,
    by_type: HashMap<MediaType, HashSet<MediaId>>,

    // Sorted indices for UI display - these are the source of truth for order
    sorted_movie_ids: Vec<MediaId>,
    sorted_series_ids: Vec<MediaId>,

    // Current sort criteria
    current_movie_sort: (SortBy, SortOrder),
    current_series_sort: (SortBy, SortOrder),

    // Subscribers for change notifications
    subscribers: Vec<Weak<dyn MediaStoreSubscriber>>,

    // Batch mode flag to delay sorting and notifications
    batch_mode: bool,
    pending_events: Vec<MediaChangeEvent>,

    // Performance optimization flags
    needs_resort: bool,    // Whether we need to resort after batch
    is_initial_load: bool, // Whether this is the initial load from server (pre-sorted)

    // Track series to library mapping for deriving child library_ids
    series_to_library: HashMap<SeriesID, Uuid>,

    // Deferred items that need parent relationships resolved
    deferred_items: Vec<MediaReference>,
}

impl MediaStore {
    /// Create a new empty media store
    pub fn new() -> Self {
        Self {
            media: HashMap::new(),
            by_library: HashMap::new(),
            by_type: HashMap::new(),
            sorted_movie_ids: Vec::new(),
            sorted_series_ids: Vec::new(),
            current_movie_sort: (SortBy::Title, SortOrder::Ascending), // Default to alphabetical
            current_series_sort: (SortBy::Title, SortOrder::Ascending), // Default to alphabetical
            subscribers: Vec::new(),
            batch_mode: false,
            pending_events: Vec::new(),
            needs_resort: false,
            is_initial_load: true, // Assume initial load by default
            series_to_library: HashMap::new(),
            deferred_items: Vec::new(),
        }
    }

    /// Subscribe to media changes
    pub fn subscribe(&mut self, subscriber: Weak<dyn MediaStoreSubscriber>) {
        self.subscribers.push(subscriber);
    }

    /// Start a batch update (delays notifications until end_batch)
    pub fn begin_batch(&mut self) {
        log::info!(
            "MediaStore: Beginning batch mode - current store size: {}",
            self.media.len()
        );
        if self.batch_mode {
            log::warn!("MediaStore: begin_batch called while already in batch mode!");
        }

        // DEBUG: Log what's currently in the store
        let season_count = self
            .by_type
            .get(&MediaType::Season)
            .map(|s| s.len())
            .unwrap_or(0);
        if season_count > 0 {
            log::info!(
                "MediaStore: {} seasons already in store before batch",
                season_count
            );
        }

        self.batch_mode = true;
        self.pending_events.clear();
        self.deferred_items.clear();
    }

    /// Mark the next batch as initial load (skip sorting, preserve server order)
    pub fn set_initial_load(&mut self, is_initial: bool) {
        self.is_initial_load = is_initial;
    }

    /// Preserve the current insertion order in sorted indices
    /// Used when we receive pre-sorted data from the server
    fn preserve_insertion_order(&mut self) {
        // During initial load, items were already added to sorted_movie_ids and sorted_series_ids
        // in the correct order as they were inserted. We just need to ensure the sets match.

        // For movies, preserve the insertion order
        if !self.sorted_movie_ids.is_empty() {
            log::debug!(
                "Preserving insertion order for {} movies",
                self.sorted_movie_ids.len()
            );
        }

        // For series, preserve the insertion order
        if !self.sorted_series_ids.is_empty() {
            log::debug!(
                "Preserving insertion order for {} series",
                self.sorted_series_ids.len()
            );
        }
    }

    /// End a batch update and send all notifications
    pub fn end_batch(&mut self) {
        if !self.batch_mode {
            return;
        }

        log::debug!("MediaStore: Ending batch mode - processing deferred items");
        self.batch_mode = false;

        // Process any deferred items that were waiting for parent relationships
        if !self.deferred_items.is_empty() {
            log::debug!(
                "Processing {} deferred items after batch",
                self.deferred_items.len()
            );
            let deferred = std::mem::take(&mut self.deferred_items);
            for item in deferred {
                // Re-attempt to insert with parent mappings now available
                self.upsert(item);
            }
        }

        // Always sort after batch to ensure consistent ordering
        // Even if server sends pre-sorted data, we want our own consistent sort
        // This ensures deduplication and proper alphabetical ordering
        self.update_sorted_movie_ids();
        self.update_sorted_series_ids();

        // Reset the needs_resort flag
        self.needs_resort = false;

        // Log a summary of what was added during the batch
        let movie_count = self
            .by_type
            .get(&MediaType::Movie)
            .map(|s| s.len())
            .unwrap_or(0);
        let series_count = self
            .by_type
            .get(&MediaType::Series)
            .map(|s| s.len())
            .unwrap_or(0);
        let season_count = self
            .by_type
            .get(&MediaType::Season)
            .map(|s| s.len())
            .unwrap_or(0);
        let episode_count = self
            .by_type
            .get(&MediaType::Episode)
            .map(|s| s.len())
            .unwrap_or(0);

        log::info!(
            "MediaStore batch complete: {} movies, {} series, {} seasons, {} episodes (total: {})",
            movie_count,
            series_count,
            season_count,
            episode_count,
            self.media.len()
        );

        // Reset flags
        self.needs_resort = false;

        // Send all pending events
        let events = std::mem::take(&mut self.pending_events);
        for event in events {
            self.notify_subscribers(event);
        }

        // Notify batch complete
        self.notify_batch_complete();
    }

    /// Insert or update a media item
    pub fn upsert(&mut self, media: MediaReference) -> MediaId {
        let media_id = get_media_id(&media);

        // Update series_to_library mapping for series (still needed for episodes)
        if let MediaReference::Series(series) = &media {
            let library_id = series.library_id;
            self.series_to_library.insert(series.id.clone(), library_id);
        }

        // Episodes still need parent mapping check, but seasons don't anymore
        if self.batch_mode {
            if let MediaReference::Episode(episode) = &media {
                // Check if parent series mapping exists for episode library_id derivation
                if !self.series_to_library.contains_key(&episode.series_id) {
                    log::debug!(
                        "Deferring episode {} (S{}E{}) - parent series {} not yet processed",
                        episode.id.as_str(),
                        episode.season_number.value(),
                        episode.episode_number.value(),
                        episode.series_id.as_str()
                    );
                    self.deferred_items.push(media);
                    return media_id;
                }
            }
            // Seasons no longer need deferral as they have library_id directly
        }

        let library_id = get_library_id(&media, &self.series_to_library);
        let media_type = MediaType::from(&media);

        // Debug: Log what we're about to insert (only in trace mode to avoid spam)
        if let Some(series) = media.as_series() {
            log::trace!(
                "MediaStore::upsert - Series '{}' with library_id: {} (from series.library_id)",
                series.title.as_str(),
                library_id
            );
        }

        // DEBUG: Log seasons being upserted (only in trace mode to avoid performance impact)
        if let MediaReference::Season(season) = &media {
            log::trace!(
                "MediaStore::upsert - SEASON {} (ID: {}) for series {}",
                season.season_number.value(),
                season.id.as_str(),
                season.series_id.as_str()
            );

            // Extra validation
            if season.series_id.as_str().is_empty() {
                log::error!("Season {} has EMPTY series_id!", season.id.as_str());
            }
        }

        // Debug: Check what we're storing
        let has_details = match media.media_type() {
            "movie" | "series" => {
                !crate::infrastructure::api_types::needs_details_fetch(media.as_ref().details())
            }
            _ => false,
        };

        log::trace!(
            "MediaStore: Upserting {:?} (type: {:?}) with library_id: {}, has_details: {}",
            media_id,
            media_type,
            library_id,
            has_details
        );

        // Check if this is an update or insert
        let change_type = if let Some(existing) = self.media.get(&media_id) {
            // CRITICAL: Don't overwrite Details with Endpoints!
            // If we have Details and incoming is Endpoint, keep the Details
            let should_update = match (&existing, &media) {
                (MediaReference::Movie(existing_movie), MediaReference::Movie(new_movie)) => {
                    let existing_has_details =
                        !crate::infrastructure::api_types::needs_details_fetch(
                            &existing_movie.details,
                        );
                    let new_has_details =
                        !crate::infrastructure::api_types::needs_details_fetch(&new_movie.details);

                    if existing_has_details && !new_has_details {
                        log::warn!(
                            "MediaStore: Refusing to overwrite Details with Endpoint for movie {}",
                            existing_movie.title.as_str()
                        );
                        false // Don't update - keep the existing Details
                    } else {
                        true // Update - new has Details or both have same type
                    }
                }
                (MediaReference::Series(existing_series), MediaReference::Series(new_series)) => {
                    let existing_has_details =
                        !crate::infrastructure::api_types::needs_details_fetch(
                            &existing_series.details,
                        );
                    let new_has_details =
                        !crate::infrastructure::api_types::needs_details_fetch(&new_series.details);

                    if existing_has_details && !new_has_details {
                        log::warn!(
                            "MediaStore: Refusing to overwrite Details with Endpoint for series {}",
                            existing_series.title.as_str()
                        );
                        false // Don't update - keep the existing Details
                    } else {
                        true // Update - new has Details or both have same type
                    }
                }
                _ => true, // For other types, always update
            };

            if should_update {
                // Update main storage
                self.media.insert(media_id.clone(), media);
                ChangeType::Updated
            } else {
                // Skip update, return early without triggering events
                return media_id;
            }
        } else {
            // New item, insert it
            self.media.insert(media_id.clone(), media);
            ChangeType::Added
        };

        // Update indices
        // Check if library_id is valid (not default UUID)
        if library_id == Uuid::default() {
            log::warn!(
                "MediaStore: Skipping library index for {:?} - has default/invalid library_id",
                media_id
            );
            // Don't index items with default UUID in by_library
        } else {
            // series_to_library mapping already updated at the start of upsert() for series
            // Only log in trace mode to reduce noise during batch operations
            if log::log_enabled!(log::Level::Trace) {
                if matches!(media_type, MediaType::Series) {
                    log::trace!(
                        "MediaStore: Indexing SERIES {:?} to library {}",
                        media_id,
                        library_id
                    );
                }
                // Log seasons with enhanced debugging (also in trace mode)
                if matches!(media_type, MediaType::Season) {
                    // Get the season reference to log more details
                    if let Some(MediaReference::Season(season_ref)) = self.media.get(&media_id) {
                        log::trace!(
                            "MediaStore: Indexing SEASON {} (Series: {}) to library {} (derived from parent)",
                            season_ref.id.as_str(),
                            season_ref.series_id.as_str(),
                            library_id
                        );
                    } else {
                        log::trace!(
                            "MediaStore: Indexing SEASON {:?} to library {} (derived)",
                            media_id,
                            library_id
                        );
                    }
                }
            }
            self.by_library
                .entry(library_id)
                .or_insert_with(HashSet::new)
                .insert(media_id.clone());
        }

        // Add to by_type index
        let inserted = self
            .by_type
            .entry(media_type)
            .or_insert_with(HashSet::new)
            .insert(media_id.clone());

        // Only log season insertions in trace mode to avoid performance impact
        if media_type == MediaType::Season && log::log_enabled!(log::Level::Trace) {
            let season_count = self
                .by_type
                .get(&MediaType::Season)
                .map(|s| s.len())
                .unwrap_or(0);
            log::trace!(
                "MediaStore: Season added to by_type index. Total seasons: {}, was_new: {}",
                season_count,
                inserted
            );
        }

        // Debug logging commented out - was causing performance issues with large libraries
        // Log summary only when not in batch mode to avoid spam
        if !self.batch_mode && log::log_enabled!(log::Level::Debug) {
            if let Some(library_items) = self.by_library.get(&library_id) {
                log::debug!(
                    "MediaStore: Library {} now has {} items indexed",
                    library_id,
                    library_items.len()
                );
            }
        }

        // Update sorted indices for the appropriate type
        // CRITICAL: Always ensure movies/series are in sorted lists, whether insert or update
        match media_type {
            MediaType::Movie => {
                if !self.batch_mode {
                    // Non-batch mode: immediate re-sort
                    // Remove if it exists (for updates)
                    self.sorted_movie_ids.retain(|id| id != &media_id);
                    // Add and re-sort
                    self.sorted_movie_ids.push(media_id.clone());
                    self.update_sorted_movie_ids();
                } else {
                    // Batch mode: just add to list, we'll dedup when sorting
                    // This ensures ALL movies end up in the sorted list
                    self.needs_resort = true;
                    self.sorted_movie_ids.push(media_id.clone());
                }
            }
            MediaType::Series => {
                if !self.batch_mode {
                    // Non-batch mode: immediate re-sort
                    // Remove if it exists (for updates)
                    self.sorted_series_ids.retain(|id| id != &media_id);
                    // Add and re-sort
                    self.sorted_series_ids.push(media_id.clone());
                    self.update_sorted_series_ids();
                } else {
                    // Batch mode: just add to list, we'll dedup when sorting
                    // This ensures ALL series end up in the sorted list
                    self.needs_resort = true;
                    self.sorted_series_ids.push(media_id.clone());
                }
            }
            _ => {}
        }

        // Create event
        let event = MediaChangeEvent {
            media_id: media_id.clone(),
            media_type,
            library_id,
            change_type,
        };

        if self.batch_mode {
            self.pending_events.push(event);
        } else {
            self.notify_subscribers(event);
        }

        media_id
    }

    /// Bulk upsert multiple media items efficiently
    /// This is much more efficient than calling upsert() in a loop
    pub fn bulk_upsert(&mut self, items: Vec<MediaReference>) -> Vec<MediaId> {
        let was_batch_mode = self.batch_mode;

        // Temporarily enable batch mode if not already enabled
        if !was_batch_mode {
            self.begin_batch();
        }

        let mut media_ids = Vec::with_capacity(items.len());

        // First pass: insert all series to establish parent mappings
        for item in items.iter() {
            if matches!(item, MediaReference::Series(_)) {
                media_ids.push(self.upsert(item.clone()));
            }
        }

        // Second pass: insert everything else
        for item in items {
            if !matches!(item, MediaReference::Series(_)) {
                media_ids.push(self.upsert(item));
            }
        }

        // Restore original batch mode state
        if !was_batch_mode {
            self.end_batch();
        }

        media_ids
    }

    /// Remove a media item
    pub fn remove(&mut self, media_id: &MediaId) -> Option<MediaReference> {
        if let Some(media) = self.media.remove(media_id) {
            let library_id = get_library_id(&media, &self.series_to_library);
            let media_type = MediaType::from(&media);

            // Update indices
            if let Some(library_items) = self.by_library.get_mut(&library_id) {
                library_items.remove(media_id);
            }

            if let Some(type_items) = self.by_type.get_mut(&media_type) {
                type_items.remove(media_id);
            }

            // Remove from sorted indices
            match media_type {
                MediaType::Movie => {
                    self.sorted_movie_ids.retain(|id| id != media_id);
                }
                MediaType::Series => {
                    self.sorted_series_ids.retain(|id| id != media_id);
                    // Also clean up series_to_library mapping
                    if let MediaReference::Series(series_ref) = &media {
                        self.series_to_library.remove(&series_ref.id);
                        log::debug!(
                            "MediaStore: Removed series {} from library mapping",
                            series_ref.id.as_str()
                        );
                    }
                }
                _ => {}
            }

            // Create event
            let event = MediaChangeEvent {
                media_id: media_id.clone(),
                media_type,
                library_id,
                change_type: ChangeType::Removed,
            };

            if self.batch_mode {
                self.pending_events.push(event);
            } else {
                self.notify_subscribers(event);
            }

            Some(media)
        } else {
            None
        }
    }

    /// Get a media item by ID
    pub fn get(&self, media_id: &MediaId) -> Option<&MediaReference> {
        self.media.get(media_id)
    }

    /// Update the sorted movie indices based on current sort criteria
    pub fn update_sorted_movie_ids(&mut self) {
        #[cfg(any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ))]
        profiling::scope!("MediaStore::update_sorted_movie_ids");

        use ferrex_core::query::sorting::strategy::FieldSort;

        // Get all movie IDs without cloning the HashSet
        if let Some(movie_ids) = self.by_type.get(&MediaType::Movie) {
            // Collect MovieReference instances that we'll sort
            let mut movies: Vec<MovieReference> = movie_ids
                .iter()
                .filter_map(|id| self.media.get(id))
                .filter_map(|media| match media {
                    MediaReference::Movie(movie) => Some(movie.clone()),
                    _ => None,
                })
                .collect();

            // Create the appropriate sorting strategy based on current sort criteria
            let reverse = matches!(self.current_movie_sort.1, SortOrder::Descending);

            match self.current_movie_sort.0 {
                SortBy::Title => {
                    let strategy = FieldSort::new(TitleField, reverse);
                    strategy.sort(&mut movies);
                }
                SortBy::DateAdded => {
                    let strategy = FieldSort::new(DateAddedField, reverse);
                    strategy.sort(&mut movies);
                }
                SortBy::Year => {
                    let strategy = FieldSort::new(ReleaseDateField, reverse);
                    strategy.sort(&mut movies);
                }
                SortBy::Rating => {
                    let strategy = FieldSort::new(RatingField, reverse);
                    strategy.sort(&mut movies);
                }
                SortBy::Runtime => {
                    let strategy = FieldSort::new(RuntimeField, reverse);
                    strategy.sort(&mut movies);
                }
                SortBy::Popularity => {
                    let strategy = FieldSort::new(PopularityField, reverse);
                    strategy.sort(&mut movies);
                }
                SortBy::FileSize => {
                    // FileSize is not in the trait system yet, fall back to manual sort
                    movies.sort_by(|a, b| {
                        let cmp = a.file.size.cmp(&b.file.size);
                        if reverse {
                            cmp.reverse()
                        } else {
                            cmp
                        }
                    });
                }
                SortBy::Resolution => {
                    // Resolution is not in the trait system yet, fall back to manual sort
                    movies.sort_by(|a, b| {
                        let res_a = a
                            .file
                            .media_file_metadata
                            .as_ref()
                            .and_then(|m| m.height)
                            .unwrap_or(0);
                        let res_b = b
                            .file
                            .media_file_metadata
                            .as_ref()
                            .and_then(|m| m.height)
                            .unwrap_or(0);
                        let cmp = res_a.cmp(&res_b);
                        if reverse {
                            cmp.reverse()
                        } else {
                            cmp
                        }
                    });
                }
                SortBy::LastWatched => {
                    // TODO: LastWatchedField requires user context
                    // For now, fall back to date added
                    let strategy = FieldSort::new(DateAddedField, reverse);
                    strategy.sort(&mut movies);
                }
                SortBy::Genre => {
                    // Genre sorting - use first genre alphabetically
                    movies.sort_by(|a, b| {
                        use ferrex_core::MediaRef;
                        let genres_a = a.genres();
                        let genres_b = b.genres();
                        let genre_a = genres_a.first().copied().unwrap_or("");
                        let genre_b = genres_b.first().copied().unwrap_or("");
                        let cmp = genre_a.cmp(genre_b);
                        if reverse {
                            cmp.reverse()
                        } else {
                            cmp
                        }
                    });
                }
            };

            // Extract the sorted IDs and dedup
            let mut movie_ids: Vec<MediaId> = movies
                .into_iter()
                .map(|movie| MediaId::Movie(movie.id))
                .collect();
            movie_ids.dedup();
            self.sorted_movie_ids = movie_ids;
        } else {
            self.sorted_movie_ids.clear();
        }
    }

    /// Update the sorted series indices based on current sort criteria
    pub fn update_sorted_series_ids(&mut self) {
        #[cfg(any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ))]
        profiling::scope!("MediaStore::update_sorted_series_ids");

        use ferrex_core::query::sorting::strategy::FieldSort;

        // Get all series IDs without cloning the HashSet
        if let Some(series_ids) = self.by_type.get(&MediaType::Series) {
            // Collect SeriesReference instances that we'll sort
            let mut series: Vec<SeriesReference> = series_ids
                .iter()
                .filter_map(|id| self.media.get(id))
                .filter_map(|media| match media {
                    MediaReference::Series(s) => Some(s.clone()),
                    _ => None,
                })
                .collect();

            // Create the appropriate sorting strategy based on current sort criteria
            let reverse = matches!(self.current_series_sort.1, SortOrder::Descending);

            match self.current_series_sort.0 {
                SortBy::Title => {
                    let strategy = FieldSort::new(TitleField, reverse);
                    strategy.sort(&mut series);
                }
                SortBy::DateAdded => {
                    let strategy = FieldSort::new(DateAddedField, reverse);
                    strategy.sort(&mut series);
                }
                SortBy::Year => {
                    let strategy = FieldSort::new(ReleaseDateField, reverse);
                    strategy.sort(&mut series);
                }
                SortBy::Rating => {
                    let strategy = FieldSort::new(RatingField, reverse);
                    strategy.sort(&mut series);
                }
                SortBy::Runtime => {
                    // Series don't have runtime as a whole, skip or use first episode
                    // For now, fall back to title sort
                    let strategy = FieldSort::new(TitleField, reverse);
                    strategy.sort(&mut series);
                }
                SortBy::Popularity => {
                    let strategy = FieldSort::new(PopularityField, reverse);
                    strategy.sort(&mut series);
                }
                SortBy::FileSize => {
                    // Series don't have file size, fall back to title sort
                    let strategy = FieldSort::new(TitleField, reverse);
                    strategy.sort(&mut series);
                }
                SortBy::Resolution => {
                    // Series don't have resolution, fall back to title sort
                    let strategy = FieldSort::new(TitleField, reverse);
                    strategy.sort(&mut series);
                }
                SortBy::LastWatched => {
                    // TODO: LastWatchedField requires user context
                    // For now, fall back to date added
                    let strategy = FieldSort::new(DateAddedField, reverse);
                    strategy.sort(&mut series);
                }
                SortBy::Genre => {
                    // Genre sorting - use first genre alphabetically
                    series.sort_by(|a, b| {
                        use ferrex_core::MediaRef;
                        let genres_a = a.genres();
                        let genres_b = b.genres();
                        let genre_a = genres_a.first().copied().unwrap_or("");
                        let genre_b = genres_b.first().copied().unwrap_or("");
                        let cmp = genre_a.cmp(genre_b);
                        if reverse {
                            cmp.reverse()
                        } else {
                            cmp
                        }
                    });
                }
            };

            // Extract the sorted IDs and dedup
            let mut series_ids: Vec<MediaId> =
                series.into_iter().map(|s| MediaId::Series(s.id)).collect();
            series_ids.dedup();
            self.sorted_series_ids = series_ids;
        } else {
            self.sorted_series_ids.clear();
        }
    }

    /// Set the sort criteria for movies and update indices
    pub fn set_movie_sort(&mut self, sort_by: SortBy, sort_order: SortOrder) {
        self.current_movie_sort = (sort_by, sort_order);
        self.update_sorted_movie_ids();

        // Notify subscribers that data has changed
        self.notify_batch_complete();
    }

    /// Set the sort criteria for series and update indices
    pub fn set_series_sort(&mut self, sort_by: SortBy, sort_order: SortOrder) {
        self.current_series_sort = (sort_by, sort_order);
        self.update_sorted_series_ids();

        // Notify subscribers that data has changed
        self.notify_batch_complete();
    }

    /// Get all movies, optionally filtered by library
    pub fn get_movies(&self, library_id: Option<Uuid>) -> Vec<&MovieReference> {
        #[cfg(any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ))]
        profiling::scope!("MediaStore::get_movies");

        // Use sorted indices for predictable order
        match library_id {
            Some(lib_id) => {
                // Filter sorted movies by library
                let library_ids = self.by_library.get(&lib_id);

                self.sorted_movie_ids
                    .iter()
                    .filter(|id| library_ids.map_or(false, |lib_ids| lib_ids.contains(id)))
                    .filter_map(|id| self.media.get(id))
                    .filter_map(|media| media.as_movie())
                    .collect()
            }
            None => {
                // Return all movies in sorted order
                self.sorted_movie_ids
                    .iter()
                    .filter_map(|id| self.media.get(id))
                    .filter_map(|media| media.as_movie())
                    .collect()
            }
        }
    }

    /// Get all series, optionally filtered by library
    pub fn get_series(&self, library_id: Option<Uuid>) -> Vec<&SeriesReference> {
        #[cfg(any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ))]
        profiling::scope!("MediaStore::get_series");

        // Use sorted indices for predictable order
        match library_id {
            Some(lib_id) => {
                // Filter sorted series by library
                let library_ids = self.by_library.get(&lib_id);

                self.sorted_series_ids
                    .iter()
                    .filter(|id| library_ids.map_or(false, |lib_ids| lib_ids.contains(id)))
                    .filter_map(|id| self.media.get(id))
                    .filter_map(|media| media.as_series())
                    .collect()
            }
            None => {
                // Return all series in sorted order
                self.sorted_series_ids
                    .iter()
                    .filter_map(|id| self.media.get(id))
                    .filter_map(|media| media.as_series())
                    .collect()
            }
        }
    }

    /// Get seasons for a series
    pub fn get_seasons(&self, series_id: &Uuid) -> Vec<&SeasonReference> {
        // Debug logging to diagnose season loading issue
        let season_ids = self.by_type.get(&MediaType::Season);
        if let Some(ids) = season_ids {
            log::info!(
                "MediaStore::get_seasons - Found {} total seasons in by_type index for series {}",
                ids.len(),
                series_id
            );

            // Log first few season IDs for debugging
            for (i, id) in ids.iter().take(3).enumerate() {
                log::debug!("  Season ID {}: {:?}", i, id);
            }

            let all_seasons: Vec<_> = ids
                .iter()
                .filter_map(|id| self.media.get(id))
                .filter_map(|media| media.as_season())
                .collect();

            log::debug!(
                "MediaStore::get_seasons - {} seasons found in media HashMap (out of {} in index)",
                all_seasons.len(),
                ids.len()
            );

            // DEBUG: Log what series_ids the seasons have
            if all_seasons.is_empty() {
                log::warn!("No seasons found in media HashMap despite having IDs in index!");
            } /* else {

                  for season in &all_seasons {
                      log::debug!(
                          "  Season {} (ID: {}) has series_id: {} (looking for: {})",
                          season.season_number.value(),
                          season.id.as_str(),
                          season.series_id.as_str(),
                          series_id
                      );
                  }
              } */

            let matching_seasons: Vec<_> = all_seasons
                .into_iter()
                .filter(|season| season.series_id.as_ref() == series_id)
                .collect();

            /*
            log::debug!(
                "MediaStore::get_seasons - {} seasons match series_id {}",
                matching_seasons.len(),
                series_id
            ); */

            matching_seasons
        } else {
            log::warn!("MediaStore::get_seasons - No seasons in by_type index!");
            Vec::new()
        }
    }

    /// Get owned seasons for a series (for UI components that need owned data)
    pub fn get_seasons_owned(&self, series_id: &Uuid) -> Vec<SeasonReference> {
        self.by_type
            .get(&MediaType::Season)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.media.get(id))
                    .filter_map(|media| match media {
                        MediaReference::Season(season)
                            if season.series_id.as_ref() == series_id =>
                        {
                            Some(season.clone())
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all movies regardless of library
    pub fn get_all_movies(&self) -> Vec<&MovieReference> {
        self.by_type
            .get(&MediaType::Movie)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.media.get(id))
                    .filter_map(|media| media.as_movie())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all series regardless of library
    pub fn get_all_series(&self) -> Vec<&SeriesReference> {
        self.by_type
            .get(&MediaType::Series)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.media.get(id))
                    .filter_map(|media| media.as_series())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get episodes for a season
    pub fn get_episodes(&self, season_id: &Uuid) -> Vec<&EpisodeReference> {
        self.by_type
            .get(&MediaType::Episode)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.media.get(id))
                    .filter_map(|media| {
                        media
                            .as_episode()
                            .filter(|episode| episode.season_id.as_ref() == season_id)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get the library ID for a media item
    pub fn get_library_id(&self, media_id: &MediaId) -> Option<Uuid> {
        self.media.get(media_id).and_then(|media_ref| {
            // Try to get library_id from browsable types (series/seasons)
            if let Some(browsable) = media_ref.as_browsable() {
                return Some(browsable.library_id());
            }

            // Try to get library_id from playable types (movies/episodes)
            if let Some(playable) = media_ref.as_playable() {
                return Some(playable.file().library_id);
            }

            // Special handling for seasons - need to look up parent series
            if let Some(season) = media_ref.as_season() {
                use ferrex_core::SeriesID;
                if let Ok(series_id) = SeriesID::new(season.series_id.as_str().to_string()) {
                    return self
                        .get(&MediaId::Series(series_id))
                        .and_then(|media| media.as_series().map(|s| s.library_id));
                }
            }

            // Special handling for episodes - need to look up parent series
            if let Some(episode) = media_ref.as_episode() {
                use ferrex_core::SeriesID;
                if let Ok(series_id) = SeriesID::new(episode.series_id.as_str().to_string()) {
                    return self
                        .get(&MediaId::Series(series_id))
                        .and_then(|media| media.as_series().map(|s| s.library_id));
                }
            }

            None
        })
    }

    /// Get MediaFile by MediaId - O(1) lookup for efficient playback
    /// Returns None if the MediaId doesn't exist or doesn't have a file (Series/Season)
    pub fn get_media_file_by_id(
        &self,
        media_id: &MediaId,
    ) -> Option<ferrex_core::media::MediaFile> {
        // Direct O(1) HashMap lookup
        self.media.get(media_id).and_then(|media_ref| {
            match media_ref {
                MediaReference::Movie(movie) => Some(movie.file.clone()),
                MediaReference::Episode(episode) => Some(episode.file.clone()),
                MediaReference::Series(_) | MediaReference::Season(_) => None, // No file for these
            }
        })
    }

    /// Find all media items with a specific file ID
    /// This is useful when handling file deletion events from the server
    /// Returns a vector of MediaIds that have the matching file ID
    pub fn _find_by_file_id(&self, media_id: &MediaId) -> Vec<MediaId> {
        let mut matching_ids = Vec::new();

        // Only check Movies and Episodes since they have files
        // Check movies first
        if let Some(movie_ids) = self.by_type.get(&MediaType::Movie) {
            for id in movie_ids {
                if id == media_id {
                    matching_ids.push(*media_id);
                }
            }
        }

        // Check episodes
        if let Some(episode_ids) = self.by_type.get(&MediaType::Episode) {
            for id in episode_ids {
                if id == media_id {
                    matching_ids.push(*media_id);
                }
            }
        }

        matching_ids
    }

    /// Clear all data from a specific library
    pub fn clear_library(&mut self, library_id: Uuid) {
        log::warn!(
            "MediaStore::clear_library() called for library {}",
            library_id
        );
        if let Some(media_ids) = self.by_library.remove(&library_id) {
            for media_id in &media_ids {
                self.media.remove(media_id);

                // Remove from type index
                for type_set in self.by_type.values_mut() {
                    type_set.remove(media_id);
                }
            }

            // Remove from sorted indices
            self.sorted_movie_ids.retain(|id| !media_ids.contains(id));
            self.sorted_series_ids.retain(|id| !media_ids.contains(id));
            // No need to re-sort when removing
        }
    }

    /// Clear all data
    pub fn clear(&mut self) {
        log::warn!("MediaStore::clear() called - clearing all data!");
        self.media.clear();
        self.by_library.clear();
        self.by_type.clear();
        self.sorted_movie_ids.clear();
        self.sorted_series_ids.clear();
        self.pending_events.clear();
    }

    /// Get total count of items
    pub fn len(&self) -> usize {
        self.media.len()
    }

    /// Check if store is empty
    pub fn is_empty(&self) -> bool {
        self.media.is_empty()
    }

    // ============ Library-Centric Sorting Methods ============

    /// Sort movies in a library using a movie-specific sorting strategy
    pub fn sort_library_movies<S>(&mut self, library_id: Uuid, strategy: S)
    where
        S: ferrex_core::query::sorting::SortStrategy<MovieReference>,
    {
        // Get all media IDs for this library that are movies
        if let Some(library_media_ids) = self.by_library.get(&library_id) {
            if let Some(movie_ids) = self.by_type.get(&MediaType::Movie) {
                // Get intersection - movies in this library
                let mut movies: Vec<MovieReference> = library_media_ids
                    .intersection(movie_ids)
                    .filter_map(|id| self.media.get(id))
                    .filter_map(|media| match media {
                        MediaReference::Movie(movie) => Some(movie.clone()),
                        _ => None,
                    })
                    .collect();

                // Apply the sorting strategy
                strategy.sort(&mut movies);

                // Update the media store with sorted movies
                for movie in movies {
                    self.media.insert(
                        MediaId::Movie(movie.id.clone()),
                        MediaReference::Movie(movie),
                    );
                }
            }
        }
    }

    /// Sort series in a library using a series-specific sorting strategy
    pub fn sort_library_series<S>(&mut self, library_id: Uuid, strategy: S)
    where
        S: ferrex_core::query::sorting::SortStrategy<SeriesReference>,
    {
        // Get all media IDs for this library that are series
        if let Some(library_media_ids) = self.by_library.get(&library_id) {
            if let Some(series_ids) = self.by_type.get(&MediaType::Series) {
                // Get intersection - series in this library
                let mut series: Vec<SeriesReference> = library_media_ids
                    .intersection(series_ids)
                    .filter_map(|id| self.media.get(id))
                    .filter_map(|media| match media {
                        MediaReference::Series(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect();

                // Apply the sorting strategy
                strategy.sort(&mut series);

                // Update the media store with sorted series
                for s in series {
                    self.media
                        .insert(MediaId::Series(s.id.clone()), MediaReference::Series(s));
                }
            }
        }
    }

    /// Get sorted movies from a library without mutation
    pub fn get_sorted_library_movies<S>(&self, library_id: Uuid, strategy: S) -> Vec<MovieReference>
    where
        S: ferrex_core::query::sorting::SortStrategy<MovieReference>,
    {
        // Get all media IDs for this library that are movies
        if let Some(library_media_ids) = self.by_library.get(&library_id) {
            if let Some(movie_ids) = self.by_type.get(&MediaType::Movie) {
                // Get intersection - movies in this library
                let mut movies: Vec<MovieReference> = library_media_ids
                    .intersection(movie_ids)
                    .filter_map(|id| self.media.get(id))
                    .filter_map(|media| match media {
                        MediaReference::Movie(movie) => Some(movie.clone()),
                        _ => None,
                    })
                    .collect();

                // Apply the sorting strategy
                strategy.sort(&mut movies);

                return movies;
            }
        }
        Vec::new()
    }

    /// Get sorted series from a library without mutation
    pub fn get_sorted_library_series<S>(
        &self,
        library_id: Uuid,
        strategy: S,
    ) -> Vec<SeriesReference>
    where
        S: ferrex_core::query::sorting::SortStrategy<SeriesReference>,
    {
        // Get all media IDs for this library that are series
        if let Some(library_media_ids) = self.by_library.get(&library_id) {
            if let Some(series_ids) = self.by_type.get(&MediaType::Series) {
                // Get intersection - series in this library
                let mut series: Vec<SeriesReference> = library_media_ids
                    .intersection(series_ids)
                    .filter_map(|id| self.media.get(id))
                    .filter_map(|media| match media {
                        MediaReference::Series(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect();

                // Apply the sorting strategy
                strategy.sort(&mut series);

                return series;
            }
        }
        Vec::new()
    }

    /// Get sorted media filtered by type within a library
    /// Useful when you need just movies or just series from a library
    pub fn get_sorted_library_media_by_type<S>(
        &self,
        library_id: Uuid,
        media_type: MediaType,
        strategy: S,
    ) -> Vec<MediaReference>
    where
        S: ferrex_core::query::sorting::SortStrategy<MediaReference>,
    {
        // Get intersection of library media and type media
        if let (Some(library_ids), Some(type_ids)) = (
            self.by_library.get(&library_id),
            self.by_type.get(&media_type),
        ) {
            let mut filtered_media: Vec<MediaReference> = library_ids
                .intersection(type_ids)
                .filter_map(|id| self.media.get(id).cloned())
                .collect();

            // Apply the sorting strategy
            strategy.sort(&mut filtered_media);

            filtered_media
        } else {
            Vec::new()
        }
    }

    /// Get all media sorted by the provided strategy (no library filter)
    pub fn get_all_sorted<S>(&self, strategy: S) -> Vec<MediaReference>
    where
        S: ferrex_core::query::sorting::SortStrategy<MediaReference>,
    {
        let mut all_media: Vec<MediaReference> = self.media.values().cloned().collect();

        strategy.sort(&mut all_media);
        all_media
    }

    /// Check if library has sortable media (i.e., movies or series with metadata)
    pub fn library_has_sortable_media(&self, library_id: Uuid) -> bool {
        if let Some(media_ids) = self.by_library.get(&library_id) {
            media_ids.iter().any(|id| {
                if let Some(media) = self.media.get(id) {
                    match media {
                        MediaReference::Movie(m) => {
                            matches!(&m.details, ferrex_core::MediaDetailsOption::Details(_))
                        }
                        MediaReference::Series(s) => {
                            matches!(&s.details, ferrex_core::MediaDetailsOption::Details(_))
                        }
                        _ => false,
                    }
                } else {
                    false
                }
            })
        } else {
            false
        }
    }

    /// Notify subscribers of a change
    fn notify_subscribers(&mut self, event: MediaChangeEvent) {
        // Clean up dead weak references and notify live ones
        self.subscribers.retain(|weak_sub| {
            if let Some(subscriber) = weak_sub.upgrade() {
                subscriber.on_media_changed(event.clone());
                true
            } else {
                false
            }
        });
    }

    /// Notify subscribers that a batch is complete
    fn notify_batch_complete(&mut self) {
        self.subscribers.retain(|weak_sub| {
            if let Some(subscriber) = weak_sub.upgrade() {
                subscriber.on_batch_complete();
                true
            } else {
                false
            }
        });
    }

    // ========== ACCESSOR METHODS FOR TRAIT IMPLEMENTATIONS ==========

    /// Get access to the media HashMap
    pub(in crate::domains::media::store) fn media(&self) -> &HashMap<MediaId, MediaReference> {
        &self.media
    }

    /// Get mutable access to the media HashMap
    pub(in crate::domains::media::store) fn media_mut(
        &mut self,
    ) -> &mut HashMap<MediaId, MediaReference> {
        &mut self.media
    }

    /// Get access to the by_type index
    pub(in crate::domains::media::store) fn by_type(
        &self,
    ) -> &HashMap<MediaType, HashSet<MediaId>> {
        &self.by_type
    }
}

impl std::fmt::Debug for MediaStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaStore")
            .field("media_count", &self.media.len())
            .field("libraries", &self.by_library.keys().collect::<Vec<_>>())
            .field("types", &self.by_type.keys().collect::<Vec<_>>())
            .field("subscriber_count", &self.subscribers.len())
            .field("batch_mode", &self.batch_mode)
            .field("pending_events", &self.pending_events.len())
            .finish()
    }
}

// Helper functions for MediaReference

/// Get the ID for any media reference type
fn get_media_id(media: &MediaReference) -> MediaId {
    // Use the trait method for cleaner access
    media.as_ref().id()
}

/// Get the library ID for any media reference type
/// Seasons now have library_id directly, no derivation needed
fn get_library_id(media: &MediaReference, series_to_library: &HashMap<SeriesID, Uuid>) -> Uuid {
    let lib_id = match media {
        MediaReference::Season(season) => {
            // Seasons now have library_id directly
            season.library_id
        }
        MediaReference::Episode(episode) => {
            // Episodes still need derivation from parent series or file
            series_to_library
                .get(&episode.series_id)
                .copied()
                .unwrap_or_else(|| {
                    // Fallback to file's library_id if series not found
                    log::debug!(
                        "Episode {} using file library_id as fallback",
                        episode.id.as_str()
                    );
                    episode.file.library_id
                })
        }
        _ => {
            // For Movies and Series, use the existing trait-based approach
            if let Some(browsable) = media.as_browsable() {
                // Series has library_id directly
                browsable.library_id()
            } else if let Some(playable) = media.as_playable() {
                // Movies have library_id through their file
                playable.file().library_id
            } else {
                // This shouldn't happen in practice, but handle gracefully
                log::warn!("Media reference doesn't have library_id, using default. This may cause filtering issues.");
                Uuid::default()
            }
        }
    };

    /*
    // Debug logging
    match media.media_type() {
        "movie" | "series" => log::debug!(
            "get_library_id: {} '{}' -> library {}",
            media.media_type(),
            media.as_ref().title(),
            lib_id
        ),
        "season" => log::debug!(
            "get_library_id: season {} -> library {} (derived from parent)",
            media.as_ref().id(),
            lib_id
        ),
        "episode" => log::debug!(
            "get_library_id: episode {} -> library {} (derived from parent)",
            media.as_ref().id(),
            lib_id
        ),
        _ => {}
    } */

    lib_id
}
