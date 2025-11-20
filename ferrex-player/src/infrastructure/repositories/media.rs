//! Media repository trait and related types
//!
//! Defines the interface for all media data access operations,
//! replacing direct access to MediaStore.

use async_trait::async_trait;
use uuid::Uuid;
use ferrex_core::media::{MovieReference, SeriesReference, EpisodeReference, SeasonReference, MediaReference};
use ferrex_core::media::{MovieID, SeriesID, SeasonID, EpisodeID};
use ferrex_core::api_types::MediaId;
use crate::domains::media::store::MediaType;
use crate::domains::ui::types::{SortBy, SortOrder};
use super::RepositoryResult;

/// Query options for filtering media
#[derive(Debug, Clone, Default)]
pub struct MediaFilterOptions {
    pub library_id: Option<Uuid>,
    pub media_type: Option<MediaType>,
    pub genre: Option<String>,
    pub year_min: Option<i32>,
    pub year_max: Option<i32>,
    pub rating_min: Option<f32>,
    pub watched_status: Option<WatchedStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WatchedStatus {
    Watched,
    Unwatched,
    InProgress,
    Any,
}

/// Sort options for media queries - wraps UI types
#[derive(Debug, Clone)]
pub struct MediaSortOptions {
    pub field: SortBy,
    pub order: SortOrder,
}

impl Default for MediaSortOptions {
    fn default() -> Self {
        Self {
            field: SortBy::Title,
            order: SortOrder::Ascending,
        }
    }
}

/// Combined query options
#[derive(Debug, Clone, Default)]
pub struct MediaQuery {
    pub filter: MediaFilterOptions,
    pub sort: MediaSortOptions,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Repository trait for media data access
///
/// This trait defines all operations for accessing and modifying media data.
/// Implementations can use different storage backends (in-memory, database, etc.)
#[async_trait]
pub trait MediaRepository: Send + Sync {
    // ===== Read Operations =====
    
    /// Get a single media item by ID
    async fn get(&self, id: &MediaId) -> RepositoryResult<Option<MediaReference>>;
    
    /// Get multiple media items by IDs
    async fn get_many(&self, ids: &[MediaId]) -> RepositoryResult<Vec<MediaReference>>;
    
    /// Query movies with filtering and sorting
    async fn query_movies(&self, query: &MediaQuery) -> RepositoryResult<Vec<MovieReference>>;
    
    /// Query series with filtering and sorting
    async fn query_series(&self, query: &MediaQuery) -> RepositoryResult<Vec<SeriesReference>>;
    
    /// Get all movies (unfiltered, unsorted)
    async fn get_all_movies(&self) -> RepositoryResult<Vec<MovieReference>>;
    
    /// Get all series (unfiltered, unsorted)
    async fn get_all_series(&self) -> RepositoryResult<Vec<SeriesReference>>;
    
    /// Get movies by library ID
    async fn get_movies_by_library(&self, library_id: Uuid) -> RepositoryResult<Vec<MovieReference>>;
    
    /// Get series by library ID
    async fn get_series_by_library(&self, library_id: Uuid) -> RepositoryResult<Vec<SeriesReference>>;
    
    /// Get seasons for a series
    async fn get_seasons(&self, series_id: &SeriesID) -> RepositoryResult<Vec<SeasonReference>>;
    
    /// Get episodes for a season
    async fn get_episodes(&self, season_id: &SeasonID) -> RepositoryResult<Vec<EpisodeReference>>;
    
    /// Get a specific episode
    async fn get_episode(&self, episode_id: &EpisodeID) -> RepositoryResult<Option<EpisodeReference>>;
    
    /// Search media by title
    async fn search(&self, query: &str, media_type: Option<MediaType>) -> RepositoryResult<Vec<MediaReference>>;
    
    /// Count total items
    async fn count(&self, filter: &MediaFilterOptions) -> RepositoryResult<usize>;
    
    /// Check if repository is empty
    async fn is_empty(&self) -> RepositoryResult<bool>;
    
    // ===== Write Operations =====
    
    /// Insert or update a media item
    async fn upsert(&self, media: MediaReference) -> RepositoryResult<()>;
    
    /// Insert or update multiple media items
    async fn upsert_many(&self, media: Vec<MediaReference>) -> RepositoryResult<()>;
    
    /// Delete a media item
    async fn delete(&self, id: &MediaId) -> RepositoryResult<bool>;
    
    /// Delete all media for a library
    async fn delete_by_library(&self, library_id: Uuid) -> RepositoryResult<usize>;
    
    /// Clear all media
    async fn clear(&self) -> RepositoryResult<()>;
    
    // ===== Metadata Operations =====
    
    /// Update metadata for a media item
    async fn update_metadata(&self, id: &MediaId, metadata: MediaMetadataUpdate) -> RepositoryResult<()>;
    
    /// Mark media as watched/unwatched
    async fn update_watched_status(&self, id: &MediaId, watched: bool, progress: Option<f32>) -> RepositoryResult<()>;
}

/// Metadata update payload
#[derive(Debug, Clone)]
pub struct MediaMetadataUpdate {
    pub title: Option<String>,
    pub overview: Option<String>,
    pub release_date: Option<String>,
    pub rating: Option<f32>,
    pub genres: Option<Vec<String>>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
}

/// Mock implementation for testing
#[cfg(test)]
pub mod mock {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    
    pub struct MockMediaRepository {
        storage: Arc<RwLock<HashMap<MediaId, MediaReference>>>,
        pub get_called: Arc<RwLock<Vec<MediaId>>>,
        pub query_called: Arc<RwLock<Vec<MediaQuery>>>,
    }
    
    impl MockMediaRepository {
        pub fn new() -> Self {
            Self {
                storage: Arc::new(RwLock::new(HashMap::new())),
                get_called: Arc::new(RwLock::new(Vec::new())),
                query_called: Arc::new(RwLock::new(Vec::new())),
            }
        }
        
        pub async fn insert_test_data(&self, media: MediaReference) {
            let id = match &media {
                MediaReference::Movie(m) => MediaId::Movie(m.id.clone()),
                MediaReference::Series(s) => MediaId::Series(s.id.clone()),
                MediaReference::Episode(e) => MediaId::Episode(e.id.clone()),
                MediaReference::Season(s) => MediaId::Season(s.id.clone()),
            };
            self.storage.write().await.insert(id, media);
        }
    }
    
    #[async_trait]
    impl MediaRepository for MockMediaRepository {
        async fn get(&self, id: &MediaId) -> RepositoryResult<Option<MediaReference>> {
            self.get_called.write().await.push(id.clone());
            Ok(self.storage.read().await.get(id).cloned())
        }
        
        async fn get_many(&self, ids: &[MediaId]) -> RepositoryResult<Vec<MediaReference>> {
            let storage = self.storage.read().await;
            Ok(ids.iter().filter_map(|id| storage.get(id).cloned()).collect())
        }
        
        async fn query_movies(&self, query: &MediaQuery) -> RepositoryResult<Vec<MovieReference>> {
            self.query_called.write().await.push(query.clone());
            let storage = self.storage.read().await;
            let movies: Vec<MovieReference> = storage
                .values()
                .filter_map(|m| match m {
                    MediaReference::Movie(movie) => Some(movie.clone()),
                    _ => None,
                })
                .collect();
            Ok(movies)
        }
        
        async fn query_series(&self, query: &MediaQuery) -> RepositoryResult<Vec<SeriesReference>> {
            self.query_called.write().await.push(query.clone());
            let storage = self.storage.read().await;
            let series: Vec<SeriesReference> = storage
                .values()
                .filter_map(|m| match m {
                    MediaReference::Series(series) => Some(series.clone()),
                    _ => None,
                })
                .collect();
            Ok(series)
        }
        
        async fn get_all_movies(&self) -> RepositoryResult<Vec<MovieReference>> {
            self.query_movies(&MediaQuery::default()).await
        }
        
        async fn get_all_series(&self) -> RepositoryResult<Vec<SeriesReference>> {
            self.query_series(&MediaQuery::default()).await
        }
        
        async fn get_movies_by_library(&self, _library_id: Uuid) -> RepositoryResult<Vec<MovieReference>> {
            // Mock implementation
            self.get_all_movies().await
        }
        
        async fn get_series_by_library(&self, _library_id: Uuid) -> RepositoryResult<Vec<SeriesReference>> {
            // Mock implementation
            self.get_all_series().await
        }
        
        async fn get_seasons(&self, _series_id: &SeriesID) -> RepositoryResult<Vec<SeasonReference>> {
            // Mock implementation
            Ok(Vec::new())
        }
        
        async fn get_episodes(&self, _season_id: &SeasonID) -> RepositoryResult<Vec<EpisodeReference>> {
            // Mock implementation
            Ok(Vec::new())
        }
        
        async fn get_episode(&self, _episode_id: &EpisodeID) -> RepositoryResult<Option<EpisodeReference>> {
            // Mock implementation
            Ok(None)
        }
        
        async fn search(&self, _query: &str, _media_type: Option<MediaType>) -> RepositoryResult<Vec<MediaReference>> {
            // Mock implementation
            Ok(Vec::new())
        }
        
        async fn count(&self, _filter: &MediaFilterOptions) -> RepositoryResult<usize> {
            Ok(self.storage.read().await.len())
        }
        
        async fn is_empty(&self) -> RepositoryResult<bool> {
            Ok(self.storage.read().await.is_empty())
        }
        
        async fn upsert(&self, media: MediaReference) -> RepositoryResult<()> {
            let id = match &media {
                MediaReference::Movie(m) => MediaId::Movie(m.id.clone()),
                MediaReference::Series(s) => MediaId::Series(s.id.clone()),
                MediaReference::Episode(e) => MediaId::Episode(e.id.clone()),
                MediaReference::Season(s) => MediaId::Season(s.id.clone()),
            };
            self.storage.write().await.insert(id, media);
            Ok(())
        }
        
        async fn upsert_many(&self, media: Vec<MediaReference>) -> RepositoryResult<()> {
            for item in media {
                self.upsert(item).await?;
            }
            Ok(())
        }
        
        async fn delete(&self, id: &MediaId) -> RepositoryResult<bool> {
            Ok(self.storage.write().await.remove(id).is_some())
        }
        
        async fn delete_by_library(&self, _library_id: Uuid) -> RepositoryResult<usize> {
            // Mock implementation
            Ok(0)
        }
        
        async fn clear(&self) -> RepositoryResult<()> {
            self.storage.write().await.clear();
            Ok(())
        }
        
        async fn update_metadata(&self, _id: &MediaId, _metadata: MediaMetadataUpdate) -> RepositoryResult<()> {
            // Mock implementation
            Ok(())
        }
        
        async fn update_watched_status(&self, _id: &MediaId, _watched: bool, _progress: Option<f32>) -> RepositoryResult<()> {
            // Mock implementation
            Ok(())
        }
    }
}