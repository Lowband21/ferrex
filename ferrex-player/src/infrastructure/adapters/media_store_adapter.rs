//! MediaStore adapter that implements MediaRepository trait
//!
//! Wraps the existing MediaStore to provide a trait-based interface

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock as TokioRwLock;
use std::sync::RwLock as StdRwLock;
use uuid::Uuid;

use ferrex_core::media::{MovieReference, SeriesReference, EpisodeReference, SeasonReference, MediaReference};
use ferrex_core::media::{MovieID, SeriesID, SeasonID, EpisodeID};
use ferrex_core::api_types::MediaId;
use crate::domains::media::store::{MediaStore, MediaType};
use crate::domains::ui::types::{SortBy, SortOrder};
use crate::infrastructure::repositories::{
    RepositoryResult, RepositoryError,
    media::{MediaRepository, MediaQuery, MediaFilterOptions, MediaMetadataUpdate}
};

/// Adapter that implements MediaRepository using the existing MediaStore
pub struct MediaStoreAdapter {
    store: Arc<StdRwLock<MediaStore>>,
}

impl MediaStoreAdapter {
    pub fn new(store: Arc<StdRwLock<MediaStore>>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl MediaRepository for MediaStoreAdapter {
    async fn get(&self, id: &MediaId) -> RepositoryResult<Option<MediaReference>> {
        let store = self.store.read()
            .map_err(|e| RepositoryError::LockError(e.to_string()))?;
        Ok(store.get(id).cloned())
    }
    
    async fn get_many(&self, ids: &[MediaId]) -> RepositoryResult<Vec<MediaReference>> {
        let store = self.store.read()
            .map_err(|e| RepositoryError::LockError(e.to_string()))?;
        
        let mut results = Vec::new();
        for id in ids {
            if let Some(media) = store.get(id) {
                results.push(media.clone());
            }
        }
        Ok(results)
    }
    
    async fn query_movies(&self, query: &MediaQuery) -> RepositoryResult<Vec<MovieReference>> {
        let store = self.store.read()
            .map_err(|e| RepositoryError::LockError(e.to_string()))?;
        
        // Get movies, optionally filtered by library
        let movies = if let Some(lib_id) = query.filter.library_id {
            store.get_movies(Some(lib_id))
        } else {
            store.get_movies(None)
        };
        
        // Apply additional filters
        let filtered: Vec<MovieReference> = movies
            .into_iter()
            .filter(|movie| {
                use ferrex_core::MediaDetailsOption as MDO;
                use ferrex_core::TmdbDetails as TD;
                // Year filter
                if let Some(year_min) = query.filter.year_min {
                    if let MDO::Details(TD::Movie(mov)) = &movie.details {
                        if let Some(release_date) = &mov.release_date {
                            if let Ok(year) = release_date[..4].parse::<i32>() {
                                if year < year_min {
                                    return false;
                                }
                            }
                        }
                    }
                }
                
                if let Some(year_max) = query.filter.year_max {
                    if let MDO::Details(TD::Movie(mov)) = &movie.details {
                        if let Some(release_date) = &mov.release_date {
                            if let Ok(year) = release_date[..4].parse::<i32>() {
                                if year > year_max {
                                    return false;
                                }
                            }
                        }
                    }
                }
                
                // Rating filter
                if let Some(rating_min) = query.filter.rating_min {
                    if let MDO::Details(TD::Movie(mov)) = &movie.details {
                        if let Some(rating) = mov.vote_average {
                            if rating < rating_min {
                                return false;
                            }
                        }
                    }
                }
                
                true
            })
            .cloned()
            .collect();
        
        // Apply limit and offset
        let result = if let Some(offset) = query.offset {
            filtered.into_iter().skip(offset).collect::<Vec<_>>()
        } else {
            filtered
        };
        
        let result = if let Some(limit) = query.limit {
            result.into_iter().take(limit).collect()
        } else {
            result
        };
        
        Ok(result)
    }
    
    async fn query_series(&self, query: &MediaQuery) -> RepositoryResult<Vec<SeriesReference>> {
        let store = self.store.read()
            .map_err(|e| RepositoryError::LockError(e.to_string()))?;
        
        // Get series, optionally filtered by library
        let series = if let Some(lib_id) = query.filter.library_id {
            store.get_series(Some(lib_id))
        } else {
            store.get_series(None)
        };
        
        // Apply additional filters (similar to movies)
        let filtered: Vec<SeriesReference> = series
            .into_iter()
            .filter(|series| {
                use ferrex_core::MediaDetailsOption as MDO;
                use ferrex_core::TmdbDetails as TD;
                // Year filter
                if let Some(year_min) = query.filter.year_min {
                    if let MDO::Details(TD::Series(ser)) = &series.details {
                        if let Some(first_air) = &ser.first_air_date {
                            if let Ok(year) = first_air[..4].parse::<i32>() {
                                if year < year_min {
                                    return false;
                                }
                            }
                        }
                    }
                }
                
                if let Some(year_max) = query.filter.year_max {
                    if let MDO::Details(TD::Series(ser)) = &series.details {
                        if let Some(first_air) = &ser.first_air_date {
                            if let Ok(year) = first_air[..4].parse::<i32>() {
                                if year > year_max {
                                    return false;
                                }
                            }
                        }
                    }
                }
                
                // Rating filter
                if let Some(rating_min) = query.filter.rating_min {
                    if let MDO::Details(TD::Series(ser)) = &series.details {
                        if let Some(rating) = ser.vote_average {
                            if rating < rating_min {
                                return false;
                            }
                        }
                    }
                }
                
                true
            })
            .cloned()
            .collect();
        
        // Apply limit and offset
        let result = if let Some(offset) = query.offset {
            filtered.into_iter().skip(offset).collect::<Vec<_>>()
        } else {
            filtered
        };
        
        let result = if let Some(limit) = query.limit {
            result.into_iter().take(limit).collect()
        } else {
            result
        };
        
        Ok(result)
    }
    
    async fn get_all_movies(&self) -> RepositoryResult<Vec<MovieReference>> {
        let store = self.store.read()
            .map_err(|e| RepositoryError::LockError(e.to_string()))?;
        Ok(store.get_all_movies().into_iter().cloned().collect())
    }
    
    async fn get_all_series(&self) -> RepositoryResult<Vec<SeriesReference>> {
        let store = self.store.read()
            .map_err(|e| RepositoryError::LockError(e.to_string()))?;
        Ok(store.get_all_series().into_iter().cloned().collect())
    }
    
    async fn get_movies_by_library(&self, library_id: Uuid) -> RepositoryResult<Vec<MovieReference>> {
        let store = self.store.read()
            .map_err(|e| RepositoryError::LockError(e.to_string()))?;
        Ok(store.get_movies(Some(library_id)).into_iter().cloned().collect())
    }
    
    async fn get_series_by_library(&self, library_id: Uuid) -> RepositoryResult<Vec<SeriesReference>> {
        let store = self.store.read()
            .map_err(|e| RepositoryError::LockError(e.to_string()))?;
        Ok(store.get_series(Some(library_id)).into_iter().cloned().collect())
    }
    
    async fn get_seasons(&self, series_id: &SeriesID) -> RepositoryResult<Vec<SeasonReference>> {
        let store = self.store.read()
            .map_err(|e| RepositoryError::LockError(e.to_string()))?;
        Ok(store.get_seasons(series_id.as_str()).into_iter().cloned().collect())
    }
    
    async fn get_episodes(&self, season_id: &SeasonID) -> RepositoryResult<Vec<EpisodeReference>> {
        let store = self.store.read()
            .map_err(|e| RepositoryError::LockError(e.to_string()))?;
        Ok(store.get_episodes(season_id.as_str()).into_iter().cloned().collect())
    }
    
    async fn get_episode(&self, episode_id: &EpisodeID) -> RepositoryResult<Option<EpisodeReference>> {
        let store = self.store.read()
            .map_err(|e| RepositoryError::LockError(e.to_string()))?;
        Ok(store.get(&MediaId::Episode(episode_id.clone()))
            .and_then(|m| match m {
                MediaReference::Episode(e) => Some(e.clone()),
                _ => None,
            }))
    }
    
    async fn search(&self, query: &str, media_type: Option<MediaType>) -> RepositoryResult<Vec<MediaReference>> {
        let store = self.store.read()
            .map_err(|e| RepositoryError::LockError(e.to_string()))?;
        
        // Simple title-based search
        let query_lower = query.to_lowercase();
        // Search movies and series by title. Episodes/seasons omitted due to lack of title field.
        let mut results: Vec<MediaReference> = Vec::new();
        let movies = store.get_all_movies();
        for m in movies {
            if let Some(ref mt) = media_type {
                if *mt != MediaType::Movie { continue; }
            }
            if m.title.as_str().to_lowercase().contains(&query_lower) {
                results.push(MediaReference::Movie(m.clone()));
            }
        }
        let series = store.get_all_series();
        for s in series {
            if let Some(ref mt) = media_type {
                if *mt != MediaType::Series { continue; }
            }
            if s.title.as_str().to_lowercase().contains(&query_lower) {
                results.push(MediaReference::Series(s.clone()));
            }
        }
        
        Ok(results)
    }
    
    async fn count(&self, filter: &MediaFilterOptions) -> RepositoryResult<usize> {
        let store = self.store.read()
            .map_err(|e| RepositoryError::LockError(e.to_string()))?;
        
        if let Some(media_type) = &filter.media_type {
            if let Some(library_id) = filter.library_id {
                // Count by type and library
                let count = match media_type {
                    MediaType::Movie => store.get_movies(Some(library_id)).len(),
                    MediaType::Series => store.get_series(Some(library_id)).len(),
                    _ => 0,
                };
                Ok(count)
            } else {
                // Count by type only
                let count = match media_type {
                    MediaType::Movie => store.get_all_movies().len(),
                    MediaType::Series => store.get_all_series().len(),
                    _ => 0,
                };
                Ok(count)
            }
        } else {
            // Count all
            // Total count of top-level browsable/playable: movies + series
            Ok(store.get_all_movies().len() + store.get_all_series().len())
        }
    }
    
    async fn is_empty(&self) -> RepositoryResult<bool> {
        let store = self.store.read()
            .map_err(|e| RepositoryError::LockError(e.to_string()))?;
        Ok(store.is_empty())
    }
    
    async fn upsert(&self, media: MediaReference) -> RepositoryResult<()> {
        let mut store = self.store.write()
            .map_err(|e| RepositoryError::LockError(e.to_string()))?;
        store.upsert(media);
        Ok(())
    }
    
    async fn upsert_many(&self, media: Vec<MediaReference>) -> RepositoryResult<()> {
        let mut store = self.store.write()
            .map_err(|e| RepositoryError::LockError(e.to_string()))?;
        for item in media {
            store.upsert(item);
        }
        Ok(())
    }
    
    async fn delete(&self, id: &MediaId) -> RepositoryResult<bool> {
        let mut store = self.store.write()
            .map_err(|e| RepositoryError::LockError(e.to_string()))?;
        Ok(store.remove(id).is_some())
    }
    
    async fn delete_by_library(&self, library_id: Uuid) -> RepositoryResult<usize> {
        let mut store = self.store.write()
            .map_err(|e| RepositoryError::LockError(e.to_string()))?;
        
        // Get all media for this library (collect IDs first to avoid borrow conflicts)
        let movie_ids: Vec<MediaId> = store
            .get_movies(Some(library_id))
            .into_iter()
            .map(|m| MediaId::Movie(m.id.clone()))
            .collect();
        let series_ids: Vec<ferrex_core::SeriesID> = store
            .get_series(Some(library_id))
            .into_iter()
            .map(|s| s.id.clone())
            .collect();
        
        let mut count = 0;
        
        // Delete movies
        for mid in movie_ids {
            if store.remove(&mid).is_some() {
                count += 1;
            }
        }
        
        // Delete series (and their seasons/episodes)
        for sid in series_ids {
            // Remove series itself
            if store.remove(&MediaId::Series(sid.clone())).is_some() {
                count += 1;
            }
            // Also remove associated seasons and episodes
            let season_ids: Vec<ferrex_core::SeasonID> = store
                .get_seasons(sid.as_str())
                .into_iter()
                .map(|s| s.id.clone())
                .collect();
            for season_id in season_ids {
                if store.remove(&MediaId::Season(season_id.clone())).is_some() {
                    count += 1;
                }
                let episode_ids: Vec<ferrex_core::EpisodeID> = store
                    .get_episodes(season_id.as_str())
                    .into_iter()
                    .map(|e| e.id.clone())
                    .collect();
                for episode_id in episode_ids {
                    if store.remove(&MediaId::Episode(episode_id)).is_some() {
                        count += 1;
                    }
                }
            }
        }
        
        Ok(count)
    }
    
    async fn clear(&self) -> RepositoryResult<()> {
        let mut store = self.store.write()
            .map_err(|e| RepositoryError::LockError(e.to_string()))?;
        store.clear();
        Ok(())
    }
    
    async fn update_metadata(&self, _id: &MediaId, _metadata: MediaMetadataUpdate) -> RepositoryResult<()> {
        // This would update the metadata in the store
        // For now, not implemented as MediaStore doesn't have a direct update method
        Ok(())
    }
    
    async fn update_watched_status(&self, _id: &MediaId, _watched: bool, _progress: Option<f32>) -> RepositoryResult<()> {
        // This would update watched status
        // Would need to integrate with watch status tracking
        Ok(())
    }
}