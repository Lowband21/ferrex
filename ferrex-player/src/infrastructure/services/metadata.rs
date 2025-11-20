//! Metadata service trait and implementations
//!
//! Provides abstraction over metadata fetching operations,
//! replacing direct TMDB API access per RUS-136.

use crate::infrastructure::repository::RepositoryResult;
use async_trait::async_trait;
use ferrex_core::{EnhancedMovieDetails, EnhancedSeriesDetails, EpisodeDetails, SeasonDetails};
use ferrex_core::{EpisodeReference, MovieReference, SeasonReference, SeriesReference};

/// Search result from metadata provider
#[derive(Debug, Clone)]
pub struct MetadataSearchResult {
    pub id: u64,
    pub title: String,
    pub overview: Option<String>,
    pub release_date: Option<String>,
    pub poster_path: Option<String>,
    pub media_type: String, // "movie" or "tv"
}

/// Metadata service trait for fetching media metadata
#[async_trait]
pub trait MetadataService: Send + Sync {
    /// Search for movies by title
    async fn search_movies(&self, query: &str) -> RepositoryResult<Vec<MetadataSearchResult>>;

    /// Search for TV series by title
    async fn search_series(&self, query: &str) -> RepositoryResult<Vec<MetadataSearchResult>>;

    /// Get detailed movie metadata
    async fn get_movie_details(&self, tmdb_id: u64) -> RepositoryResult<EnhancedMovieDetails>;

    /// Get detailed series metadata
    async fn get_series_details(&self, tmdb_id: u64) -> RepositoryResult<EnhancedSeriesDetails>;

    /// Get season details
    async fn get_season_details(
        &self,
        series_id: u64,
        season_number: u8,
    ) -> RepositoryResult<SeasonDetails>;

    /// Get episode details
    async fn get_episode_details(
        &self,
        series_id: u64,
        season_number: u8,
        episode_number: u8,
    ) -> RepositoryResult<EpisodeDetails>;

    /// Batch fetch metadata for multiple movies
    async fn batch_fetch_movies(
        &self,
        tmdb_ids: Vec<u64>,
    ) -> RepositoryResult<Vec<EnhancedMovieDetails>>;

    /// Batch fetch metadata for multiple series
    async fn batch_fetch_series(
        &self,
        tmdb_ids: Vec<u64>,
    ) -> RepositoryResult<Vec<EnhancedSeriesDetails>>;

    /// Update movie metadata
    async fn update_movie_metadata(&self, movie: &mut MovieReference) -> RepositoryResult<()>;

    /// Update series metadata
    async fn update_series_metadata(&self, series: &mut SeriesReference) -> RepositoryResult<()>;

    /// Get image URL from path
    fn get_image_url(&self, path: &str, size: ImageSize) -> String;
}

/// Image size options for TMDB
#[derive(Debug, Clone, Copy)]
pub enum ImageSize {
    Thumbnail, // w92
    Small,     // w185
    Medium,    // w342
    Large,     // w500
    Original,  // original
}

/// Mock implementation for testing
#[cfg(test)]
pub mod mock {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    pub struct MockMetadataService {
        pub search_movies_called: Arc<RwLock<Vec<String>>>,
        pub get_movie_details_called: Arc<RwLock<Vec<u64>>>,
    }

    impl MockMetadataService {
        pub fn new() -> Self {
            Self {
                search_movies_called: Arc::new(RwLock::new(Vec::new())),
                get_movie_details_called: Arc::new(RwLock::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl MetadataService for MockMetadataService {
        async fn search_movies(&self, query: &str) -> RepositoryResult<Vec<MetadataSearchResult>> {
            self.search_movies_called
                .write()
                .await
                .push(query.to_string());

            // Return mock results
            Ok(vec![MetadataSearchResult {
                id: 550,
                title: "Fight Club".to_string(),
                overview: Some("A ticking-time-bomb insomniac...".to_string()),
                release_date: Some("1999-10-15".to_string()),
                poster_path: Some("/poster.jpg".to_string()),
                media_type: "movie".to_string(),
            }])
        }

        async fn search_series(&self, _query: &str) -> RepositoryResult<Vec<MetadataSearchResult>> {
            Ok(vec![])
        }

        async fn get_movie_details(&self, tmdb_id: u64) -> RepositoryResult<EnhancedMovieDetails> {
            self.get_movie_details_called.write().await.push(tmdb_id);

            Ok(EnhancedMovieDetails {
                id: tmdb_id,
                title: "Test Movie".to_string(),
                overview: Some("Test overview".to_string()),
                release_date: Some("2024-01-01".to_string()),
                runtime: Some(120),
                vote_average: Some(8.5),
                vote_count: Some(1000),
                popularity: Some(100.0),
                content_rating: None,
                genres: vec!["Action".to_string()],
                production_companies: vec![],
                poster_path: Some("/poster.jpg".to_string()),
                backdrop_path: Some("/backdrop.jpg".to_string()),
                logo_path: None,
                images: Default::default(),
                cast: vec![],
                crew: vec![],
                videos: vec![],
                keywords: vec![],
                external_ids: Default::default(),
            })
        }

        async fn get_series_details(
            &self,
            tmdb_id: u64,
        ) -> RepositoryResult<EnhancedSeriesDetails> {
            Ok(EnhancedSeriesDetails {
                id: tmdb_id,
                name: "Test Series".to_string(),
                overview: Some("Test overview".to_string()),
                first_air_date: Some("2024-01-01".to_string()),
                last_air_date: Some("2024-12-01".to_string()),
                number_of_seasons: Some(2),
                number_of_episodes: Some(20),
                vote_average: Some(8.0),
                vote_count: Some(500),
                popularity: Some(80.0),
                content_rating: None,
                genres: vec!["Drama".to_string()],
                networks: vec![],
                poster_path: Some("/poster.jpg".to_string()),
                backdrop_path: Some("/backdrop.jpg".to_string()),
                logo_path: None,
                images: Default::default(),
                cast: vec![],
                crew: vec![],
                videos: vec![],
                keywords: vec![],
                external_ids: Default::default(),
            })
        }

        async fn get_season_details(
            &self,
            _series_id: u64,
            season_number: u8,
        ) -> RepositoryResult<SeasonDetails> {
            Ok(SeasonDetails {
                id: 1,
                season_number,
                name: format!("Season {}", season_number),
                overview: Some("Season overview".to_string()),
                air_date: Some("2024-01-01".to_string()),
                episode_count: 10,
                poster_path: Some("/season.jpg".to_string()),
            })
        }

        async fn get_episode_details(
            &self,
            _series_id: u64,
            season_number: u8,
            episode_number: u8,
        ) -> RepositoryResult<EpisodeDetails> {
            Ok(EpisodeDetails {
                id: 1,
                episode_number,
                season_number,
                name: format!("Episode {}", episode_number),
                overview: Some("Episode overview".to_string()),
                air_date: Some("2024-01-01".to_string()),
                runtime: Some(45),
                still_path: Some("/still.jpg".to_string()),
                vote_average: Some(8.0),
            })
        }

        async fn batch_fetch_movies(
            &self,
            tmdb_ids: Vec<u64>,
        ) -> RepositoryResult<Vec<EnhancedMovieDetails>> {
            let mut results = Vec::new();
            for id in tmdb_ids {
                results.push(self.get_movie_details(id).await?);
            }
            Ok(results)
        }

        async fn batch_fetch_series(
            &self,
            tmdb_ids: Vec<u64>,
        ) -> RepositoryResult<Vec<EnhancedSeriesDetails>> {
            let mut results = Vec::new();
            for id in tmdb_ids {
                results.push(self.get_series_details(id).await?);
            }
            Ok(results)
        }

        async fn update_movie_metadata(&self, _movie: &mut MovieReference) -> RepositoryResult<()> {
            // Mock implementation - just return success
            Ok(())
        }

        async fn update_series_metadata(
            &self,
            _series: &mut SeriesReference,
        ) -> RepositoryResult<()> {
            // Mock implementation - just return success
            Ok(())
        }

        fn get_image_url(&self, path: &str, _size: ImageSize) -> String {
            format!("https://mock.tmdb.org{}", path)
        }
    }
}
