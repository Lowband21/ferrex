//! MediaStore - Single source of truth for all media data
//!
//! This module provides a centralized store for all media references with
//! a subscription mechanism for notifying components of changes.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock, Weak};
use uuid::Uuid;

use crate::api_types::{
    EpisodeReference, MediaId, MediaReference, MovieReference, SeasonReference, SeriesReference,
};

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
        match media {
            MediaReference::Movie(_) => MediaType::Movie,
            MediaReference::Series(_) => MediaType::Series,
            MediaReference::Season(_) => MediaType::Season,
            MediaReference::Episode(_) => MediaType::Episode,
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

/// Single source of truth for all media data
pub struct MediaStore {
    /// All media data indexed by ID
    media: HashMap<MediaId, MediaReference>,

    /// Index by library for fast filtering
    by_library: HashMap<Uuid, HashSet<MediaId>>,

    /// Index by type for fast filtering
    by_type: HashMap<MediaType, HashSet<MediaId>>,

    /// Subscribers to notify of changes
    #[allow(dead_code)]
    subscribers: Vec<Weak<dyn MediaStoreSubscriber>>,

    /// Batch update mode to avoid excessive notifications
    batch_mode: bool,
    pending_events: Vec<MediaChangeEvent>,
}

impl MediaStore {
    /// Create a new empty media store
    pub fn new() -> Self {
        Self {
            media: HashMap::new(),
            by_library: HashMap::new(),
            by_type: HashMap::new(),
            subscribers: Vec::new(),
            batch_mode: false,
            pending_events: Vec::new(),
        }
    }

    /// Subscribe to media changes
    pub fn subscribe(&mut self, subscriber: Weak<dyn MediaStoreSubscriber>) {
        self.subscribers.push(subscriber);
    }

    /// Start a batch update (delays notifications until end_batch)
    pub fn begin_batch(&mut self) {
        self.batch_mode = true;
        self.pending_events.clear();
    }

    /// End a batch update and send all notifications
    pub fn end_batch(&mut self) {
        if !self.batch_mode {
            return;
        }

        self.batch_mode = false;

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
        let library_id = get_library_id(&media);
        let media_type = MediaType::from(&media);

        log::trace!(
            "MediaStore: Upserting {:?} (type: {:?}) with library_id: {}",
            media_id,
            media_type,
            library_id
        );

        // Check if this is an update or insert
        let change_type = if self.media.contains_key(&media_id) {
            ChangeType::Updated
        } else {
            ChangeType::Added
        };

        // Update main storage
        self.media.insert(media_id.clone(), media);

        // Update indices
        self.by_library
            .entry(library_id)
            .or_insert_with(HashSet::new)
            .insert(media_id.clone());

        self.by_type
            .entry(media_type)
            .or_insert_with(HashSet::new)
            .insert(media_id.clone());

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

    /// Remove a media item
    pub fn remove(&mut self, media_id: &MediaId) -> Option<MediaReference> {
        if let Some(media) = self.media.remove(media_id) {
            let library_id = get_library_id(&media);
            let media_type = MediaType::from(&media);

            // Update indices
            if let Some(library_items) = self.by_library.get_mut(&library_id) {
                library_items.remove(media_id);
            }

            if let Some(type_items) = self.by_type.get_mut(&media_type) {
                type_items.remove(media_id);
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

    /// Get all movies, optionally filtered by library
    pub fn get_movies(&self, library_id: Option<Uuid>) -> Vec<&MovieReference> {
        let movie_ids = self.by_type.get(&MediaType::Movie);

        match (movie_ids, library_id) {
            (Some(ids), Some(lib_id)) => {
                // Filter by both type and library
                let library_ids = self.by_library.get(&lib_id);
                log::debug!("MediaStore: Looking for movies in library {}. Found library entry: {}, with {} items", 
                    lib_id, library_ids.is_some(), library_ids.map_or(0, |ids| ids.len()));

                ids.iter()
                    .filter(|id| library_ids.map_or(false, |lib_ids| lib_ids.contains(id)))
                    .filter_map(|id| self.media.get(id))
                    .filter_map(|media| match media {
                        MediaReference::Movie(movie) => Some(movie),
                        _ => None,
                    })
                    .collect()
            }
            (Some(ids), None) => {
                // All movies
                ids.iter()
                    .filter_map(|id| self.media.get(id))
                    .filter_map(|media| match media {
                        MediaReference::Movie(movie) => Some(movie),
                        _ => None,
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// Get all series, optionally filtered by library
    pub fn get_series(&self, library_id: Option<Uuid>) -> Vec<&SeriesReference> {
        let series_ids = self.by_type.get(&MediaType::Series);

        match (series_ids, library_id) {
            (Some(ids), Some(lib_id)) => {
                // Filter by both type and library
                let library_ids = self.by_library.get(&lib_id);
                ids.iter()
                    .filter(|id| library_ids.map_or(false, |lib_ids| lib_ids.contains(id)))
                    .filter_map(|id| self.media.get(id))
                    .filter_map(|media| match media {
                        MediaReference::Series(series) => Some(series),
                        _ => None,
                    })
                    .collect()
            }
            (Some(ids), None) => {
                // All series
                ids.iter()
                    .filter_map(|id| self.media.get(id))
                    .filter_map(|media| match media {
                        MediaReference::Series(series) => Some(series),
                        _ => None,
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// Get seasons for a series
    pub fn get_seasons(&self, series_id: &str) -> Vec<&SeasonReference> {
        self.by_type
            .get(&MediaType::Season)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.media.get(id))
                    .filter_map(|media| match media {
                        MediaReference::Season(season)
                            if season.series_id.as_str() == series_id =>
                        {
                            Some(season)
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get owned seasons for a series (for UI components that need owned data)
    pub fn get_seasons_owned(&self, series_id: &str) -> Vec<SeasonReference> {
        self.by_type
            .get(&MediaType::Season)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.media.get(id))
                    .filter_map(|media| match media {
                        MediaReference::Season(season)
                            if season.series_id.as_str() == series_id =>
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
                    .filter_map(|media| match media {
                        MediaReference::Movie(movie) => Some(movie),
                        _ => None,
                    })
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
                    .filter_map(|media| match media {
                        MediaReference::Series(series) => Some(series),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get episodes for a season
    pub fn get_episodes(&self, season_id: &str) -> Vec<&EpisodeReference> {
        self.by_type
            .get(&MediaType::Episode)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.media.get(id))
                    .filter_map(|media| match media {
                        MediaReference::Episode(episode)
                            if episode.season_id.as_str() == season_id =>
                        {
                            Some(episode)
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get the library ID for a media item
    pub fn get_library_id(&self, media_id: &MediaId) -> Option<Uuid> {
        self.media.get(media_id).and_then(|media_ref| {
            match media_ref {
                MediaReference::Movie(movie) => Some(movie.file.library_id),
                MediaReference::Series(series) => Some(series.library_id),
                MediaReference::Season(season) => {
                    // Get library_id from parent series
                    use ferrex_core::SeriesID;
                    if let Ok(series_id) = SeriesID::new(season.series_id.as_str().to_string()) {
                        self.get(&MediaId::Series(series_id))
                            .and_then(|media| match media {
                                MediaReference::Series(s) => Some(s.library_id),
                                _ => None,
                            })
                    } else {
                        None
                    }
                }
                MediaReference::Episode(episode) => {
                    // Get library_id from parent series
                    use ferrex_core::SeriesID;
                    if let Ok(series_id) = SeriesID::new(episode.series_id.as_str().to_string()) {
                        self.get(&MediaId::Series(series_id))
                            .and_then(|media| match media {
                                MediaReference::Series(s) => Some(s.library_id),
                                _ => None,
                            })
                    } else {
                        None
                    }
                }
            }
        })
    }

    /// Find all media items with a specific file ID
    /// This is useful when handling file deletion events from the server
    /// Returns a vector of MediaIds that have the matching file ID
    pub fn find_by_file_id(&self, file_id: &str) -> Vec<MediaId> {
        let mut matching_ids = Vec::new();

        // Only check Movies and Episodes since they have files
        // Check movies first
        if let Some(movie_ids) = self.by_type.get(&MediaType::Movie) {
            for media_id in movie_ids {
                if let Some(MediaReference::Movie(movie)) = self.media.get(media_id) {
                    if movie.file.id.to_string() == file_id {
                        matching_ids.push(media_id.clone());
                    }
                }
            }
        }

        // Check episodes
        if let Some(episode_ids) = self.by_type.get(&MediaType::Episode) {
            for media_id in episode_ids {
                if let Some(MediaReference::Episode(episode)) = self.media.get(media_id) {
                    if episode.file.id.to_string() == file_id {
                        matching_ids.push(media_id.clone());
                    }
                }
            }
        }

        matching_ids
    }

    /// Clear all data from a specific library
    pub fn clear_library(&mut self, library_id: Uuid) {
        if let Some(media_ids) = self.by_library.remove(&library_id) {
            for media_id in media_ids {
                self.media.remove(&media_id);

                // Remove from type index
                for type_set in self.by_type.values_mut() {
                    type_set.remove(&media_id);
                }
            }
        }
    }

    /// Clear all data
    pub fn clear(&mut self) {
        self.media.clear();
        self.by_library.clear();
        self.by_type.clear();
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
    match media {
        MediaReference::Movie(m) => MediaId::Movie(m.id.clone()),
        MediaReference::Series(s) => MediaId::Series(s.id.clone()),
        MediaReference::Season(s) => MediaId::Season(s.id.clone()),
        MediaReference::Episode(e) => MediaId::Episode(e.id.clone()),
    }
}

/// Get the library ID for any media reference type
fn get_library_id(media: &MediaReference) -> Uuid {
    match media {
        MediaReference::Movie(m) => m.file.library_id,
        MediaReference::Series(s) => s.library_id,
        // Season and Episode references need special handling
        // They don't have direct library_id, need to get from parent
        MediaReference::Season(_) => Uuid::default(), // TODO: Get from parent series
        MediaReference::Episode(e) => e.file.library_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // TODO: Add comprehensive tests for MediaStore
}
