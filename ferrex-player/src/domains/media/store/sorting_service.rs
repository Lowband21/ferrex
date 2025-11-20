//! Background sorting service for MediaStore
//! 
//! This module handles all sorting operations in a performant manner,
//! including background thread execution and intelligent caching.

use super::core::MediaStore;
use crate::domains::ui::types::{SortBy, SortOrder};
use ferrex_core::{MovieReference, SeriesReference};
use ferrex_core::query::sorting::fields::*;
use ferrex_core::query::sorting::strategy::{FieldSort, SortStrategy};
use std::sync::Arc;

/// Service for handling MediaStore sorting operations
pub struct SortingService {
    /// Handle to the media store
    media_store: Arc<std::sync::RwLock<MediaStore>>,
}

impl SortingService {
    /// Create a new sorting service
    pub fn new(media_store: Arc<std::sync::RwLock<MediaStore>>) -> Self {
        Self { media_store }
    }

    /// Sort movies asynchronously on a background thread
    pub async fn sort_movies_async(
        &self,
        sort_by: SortBy,
        sort_order: SortOrder,
    ) -> Result<(), String> {
        let store = Arc::clone(&self.media_store);
        
        tokio::task::spawn_blocking(move || {
            let mut store = store.write().unwrap();
            store.set_movie_sort(sort_by, sort_order);
        })
        .await
        .map_err(|e| format!("Failed to sort movies: {}", e))?;
        
        Ok(())
    }

    /// Sort series asynchronously on a background thread
    pub async fn sort_series_async(
        &self,
        sort_by: SortBy,
        sort_order: SortOrder,
    ) -> Result<(), String> {
        let store = Arc::clone(&self.media_store);
        
        tokio::task::spawn_blocking(move || {
            let mut store = store.write().unwrap();
            store.set_series_sort(sort_by, sort_order);
        })
        .await
        .map_err(|e| format!("Failed to sort series: {}", e))?;
        
        Ok(())
    }

    /// Sort both movies and series in parallel
    pub async fn sort_all_async(
        &self,
        sort_by: SortBy,
        sort_order: SortOrder,
    ) -> Result<(), String> {
        let movies_future = self.sort_movies_async(sort_by, sort_order);
        let series_future = self.sort_series_async(sort_by, sort_order);
        
        // Execute both sorts in parallel
        let (movies_result, series_result) = tokio::join!(movies_future, series_future);
        
        movies_result?;
        series_result?;
        
        Ok(())
    }
}

/// Optimized sorting strategies for different media types
pub mod strategies {
    use super::*;
    use ferrex_core::MediaRef;
    
    /// Create an optimized sorting strategy for movies
    pub fn create_movie_sort_strategy(
        sort_by: SortBy,
        sort_order: SortOrder,
    ) -> Box<dyn Fn(&mut Vec<MovieReference>) + Send + Sync> {
        let reverse = matches!(sort_order, SortOrder::Descending);
        
        Box::new(move |movies: &mut Vec<MovieReference>| {
            match sort_by {
                SortBy::Title => {
                    let strategy = FieldSort::new(TitleField, reverse);
                    strategy.sort(movies);
                }
                SortBy::DateAdded => {
                    let strategy = FieldSort::new(DateAddedField, reverse);
                    strategy.sort(movies);
                }
                SortBy::Year => {
                    let strategy = FieldSort::new(ReleaseDateField, reverse);
                    strategy.sort(movies);
                }
                SortBy::Rating => {
                    let strategy = FieldSort::new(RatingField, reverse);
                    strategy.sort(movies);
                }
                SortBy::Runtime => {
                    let strategy = FieldSort::new(RuntimeField, reverse);
                    strategy.sort(movies);
                }
                SortBy::Popularity => {
                    let strategy = FieldSort::new(PopularityField, reverse);
                    strategy.sort(movies);
                }
                SortBy::FileSize => {
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
                    // TODO: Implement with watch status
                    let strategy = FieldSort::new(DateAddedField, reverse);
                    strategy.sort(movies);
                }
                SortBy::Genre => {
                    movies.sort_by(|a, b| {
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
            }
        })
    }
    
    /// Create an optimized sorting strategy for series
    pub fn create_series_sort_strategy(
        sort_by: SortBy,
        sort_order: SortOrder,
    ) -> Box<dyn Fn(&mut Vec<SeriesReference>) + Send + Sync> {
        let reverse = matches!(sort_order, SortOrder::Descending);
        
        Box::new(move |series: &mut Vec<SeriesReference>| {
            match sort_by {
                SortBy::Title => {
                    let strategy = FieldSort::new(TitleField, reverse);
                    strategy.sort(series);
                }
                SortBy::DateAdded => {
                    let strategy = FieldSort::new(DateAddedField, reverse);
                    strategy.sort(series);
                }
                SortBy::Year => {
                    let strategy = FieldSort::new(ReleaseDateField, reverse);
                    strategy.sort(series);
                }
                SortBy::Rating => {
                    let strategy = FieldSort::new(RatingField, reverse);
                    strategy.sort(series);
                }
                SortBy::Popularity => {
                    let strategy = FieldSort::new(PopularityField, reverse);
                    strategy.sort(series);
                }
                // Series-incompatible sorts fall back to title
                SortBy::Runtime | SortBy::FileSize | SortBy::Resolution => {
                    let strategy = FieldSort::new(TitleField, reverse);
                    strategy.sort(series);
                }
                SortBy::LastWatched => {
                    // TODO: Implement with watch status
                    let strategy = FieldSort::new(DateAddedField, reverse);
                    strategy.sort(series);
                }
                SortBy::Genre => {
                    series.sort_by(|a, b| {
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
            }
        })
    }
}