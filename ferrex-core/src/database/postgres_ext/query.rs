use crate::{
    api_types::MediaId,
    database::{traits::MediaDatabaseTrait, postgres::PostgresDatabase},
    media::*,
    query::*,
    Result,
};

impl PostgresDatabase {
    /// Execute a media query - delegates to optimized implementation
    pub async fn query_media(&self, query: &MediaQuery) -> Result<Vec<MediaReferenceWithStatus>> {
        // Use the optimized query implementation that leverages indexes
        self.query_media_optimized(query).await
    }
    
    /// Legacy implementation - kept for reference
    #[allow(dead_code)]
    async fn query_media_legacy(&self, query: &MediaQuery) -> Result<Vec<MediaReferenceWithStatus>> {
        let mut results = Vec::new();
        
        // Get watch state if user context provided
        let watch_state = if let Some(user_id) = query.user_context {
            Some(self.get_user_watch_state(user_id).await?)
        } else {
            None
        };
        
        // If filtering by watch status, start with those items
        if let Some(watch_filter) = &query.filters.watch_status {
            if let Some(ref state) = watch_state {
                match watch_filter {
                    crate::watch_status::WatchStatusFilter::InProgress => {
                        // Get in-progress items
                        for item in &state.in_progress {
                            let media_ref = match &item.media_id {
                                MediaId::Movie(movie_id) => {
                                    match self.get_movie_reference(movie_id).await {
                                        Ok(movie) => MediaReference::Movie(movie),
                                        Err(_) => continue,
                                    }
                                }
                                MediaId::Episode(episode_id) => {
                                    match self.get_episode_reference(episode_id).await {
                                        Ok(episode) => MediaReference::Episode(episode),
                                        Err(_) => continue,
                                    }
                                }
                                _ => continue,
                            };
                            
                            results.push(MediaReferenceWithStatus {
                                media: media_ref,
                                watch_status: Some(item.clone()),
                                is_completed: false,
                            });
                        }
                    }
                    crate::watch_status::WatchStatusFilter::Completed => {
                        // Get completed items
                        for media_id in &state.completed {
                            let media_ref = match media_id {
                                MediaId::Movie(movie_id) => {
                                    match self.get_movie_reference(movie_id).await {
                                        Ok(movie) => MediaReference::Movie(movie),
                                        Err(_) => continue,
                                    }
                                }
                                MediaId::Episode(episode_id) => {
                                    match self.get_episode_reference(episode_id).await {
                                        Ok(episode) => MediaReference::Episode(episode),
                                        Err(_) => continue,
                                    }
                                }
                                _ => continue,
                            };
                            
                            results.push(MediaReferenceWithStatus {
                                media: media_ref,
                                watch_status: None,
                                is_completed: true,
                            });
                        }
                    }
                    _ => {} // Other watch status filters not implemented yet
                }
            }
        } else {
            // No watch status filter - query media from libraries
            
            // Determine which libraries to query
            let library_ids = if query.filters.library_ids.is_empty() {
                // Get all library IDs if none specified
                self.list_library_references().await?
                    .into_iter()
                    .map(|lib| lib.id)
                    .collect()
            } else {
                query.filters.library_ids.clone()
            };
            
            // Query each library based on its type
            for library_id in library_ids {
                // Get library info to determine type
                let library_ref = match self.get_library_reference(library_id).await {
                    Ok(lib) => lib,
                    Err(_) => continue,
                };
                
                match library_ref.library_type {
                    crate::LibraryType::Movies => {
                        // Get all movies from this library
                        let movies = self.get_library_movies(library_id).await?;
                        for movie in movies {
                            let movie_id = movie.id.clone();
                            let watch_status = watch_state.as_ref().and_then(|state| {
                                state.in_progress.iter()
                                    .find(|item| matches!(&item.media_id, MediaId::Movie(id) if id == &movie_id))
                                    .cloned()
                            });
                            let is_completed = watch_state.as_ref()
                                .map(|state| state.completed.contains(&MediaId::Movie(movie_id.clone())))
                                .unwrap_or(false);
                            
                            results.push(MediaReferenceWithStatus {
                                media: MediaReference::Movie(movie),
                                watch_status,
                                is_completed,
                            });
                        }
                    }
                    crate::LibraryType::TvShows => {
                        // Get all series from this library
                        let series_list = self.get_library_series(library_id).await?;
                        for series in series_list {
                            let series_id = series.id.clone();
                            
                            // Add series
                            results.push(MediaReferenceWithStatus {
                                media: MediaReference::Series(series.clone()),
                                watch_status: None,
                                is_completed: false,
                            });
                            
                            // Get seasons for this series
                            if let Ok(seasons) = self.get_series_seasons(&series_id).await {
                                for season in seasons {
                                    let season_id = season.id.clone();
                                    
                                    // Add season
                                    results.push(MediaReferenceWithStatus {
                                        media: MediaReference::Season(season.clone()),
                                        watch_status: None,
                                        is_completed: false,
                                    });
                                    
                                    // Get episodes for this season
                                    if let Ok(episodes) = self.get_season_episodes(&season_id).await {
                                        for episode in episodes {
                                            let episode_id = episode.id.clone();
                                            let watch_status = watch_state.as_ref().and_then(|state| {
                                                state.in_progress.iter()
                                                    .find(|item| matches!(&item.media_id, MediaId::Episode(id) if id == &episode_id))
                                                    .cloned()
                                            });
                                            let is_completed = watch_state.as_ref()
                                                .map(|state| state.completed.contains(&MediaId::Episode(episode_id.clone())))
                                                .unwrap_or(false);
                                            
                                            results.push(MediaReferenceWithStatus {
                                                media: MediaReference::Episode(episode),
                                                watch_status,
                                                is_completed,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        // Apply sorting
        match query.sort.primary {
            SortField::LastWatched => {
                results.sort_by(|a, b| {
                    let a_time = a.watch_status.as_ref().map(|w| w.last_watched).unwrap_or(0);
                    let b_time = b.watch_status.as_ref().map(|w| w.last_watched).unwrap_or(0);
                    match query.sort.order {
                        SortOrder::Ascending => a_time.cmp(&b_time),
                        SortOrder::Descending => b_time.cmp(&a_time),
                    }
                });
            }
            SortField::Title => {
                results.sort_by(|a, b| {
                    // Sort by title where available, or by number for seasons/episodes
                    let a_sort_key = match &a.media {
                        MediaReference::Movie(m) => (m.title.as_str(), 0, 0),
                        MediaReference::Series(s) => (s.title.as_str(), 0, 0),
                        MediaReference::Season(s) => ("", s.season_number.value() as i32, 0),
                        MediaReference::Episode(e) => ("", e.season_number.value() as i32, e.episode_number.value() as i32),
                    };
                    let b_sort_key = match &b.media {
                        MediaReference::Movie(m) => (m.title.as_str(), 0, 0),
                        MediaReference::Series(s) => (s.title.as_str(), 0, 0),
                        MediaReference::Season(s) => ("", s.season_number.value() as i32, 0),
                        MediaReference::Episode(e) => ("", e.season_number.value() as i32, e.episode_number.value() as i32),
                    };
                    
                    let cmp = if !a_sort_key.0.is_empty() && !b_sort_key.0.is_empty() {
                        // Both have titles, compare titles
                        a_sort_key.0.cmp(b_sort_key.0)
                    } else if a_sort_key.0.is_empty() && b_sort_key.0.is_empty() {
                        // Neither have titles, compare by season/episode numbers
                        a_sort_key.1.cmp(&b_sort_key.1)
                            .then_with(|| a_sort_key.2.cmp(&b_sort_key.2))
                    } else {
                        // One has title, one doesn't - titles come first
                        if a_sort_key.0.is_empty() {
                            std::cmp::Ordering::Greater
                        } else {
                            std::cmp::Ordering::Less
                        }
                    };
                    
                    match query.sort.order {
                        SortOrder::Ascending => cmp,
                        SortOrder::Descending => cmp.reverse(),
                    }
                });
            }
            SortField::DateAdded => {
                results.sort_by(|a, b| {
                    let a_date = match &a.media {
                        MediaReference::Movie(m) => m.file.created_at,
                        MediaReference::Series(_) => chrono::Utc::now(), // Series don't have files
                        MediaReference::Season(_) => chrono::Utc::now(), // Seasons don't have files
                        MediaReference::Episode(e) => e.file.created_at,
                    };
                    let b_date = match &b.media {
                        MediaReference::Movie(m) => m.file.created_at,
                        MediaReference::Series(_) => chrono::Utc::now(),
                        MediaReference::Season(_) => chrono::Utc::now(),
                        MediaReference::Episode(e) => e.file.created_at,
                    };
                    match query.sort.order {
                        SortOrder::Ascending => a_date.cmp(&b_date),
                        SortOrder::Descending => b_date.cmp(&a_date),
                    }
                });
            }
            _ => {} // Other sort fields can be implemented later
        }
        
        // Apply pagination
        let start = query.pagination.offset;
        let end = (start + query.pagination.limit).min(results.len());
        
        Ok(results[start..end].to_vec())
    }
}