use crate::{
    LibraryID, MediaDetailsOption, MovieID, MovieReference, MovieTitle, MovieURL, SeriesID,
    SeriesReference, SeriesTitle, SeriesURL,
};

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("API error: {0}")]
    ApiError(String),

    #[error("Not found")]
    NotFound,

    #[error("Rate limited")]
    RateLimited,

    #[error("Invalid API key")]
    InvalidApiKey,

    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("Parse error: {0}")]
    ParseError(String),
}

// async_trait removed - unused
use tmdb_api::{
    client::{reqwest::ReqwestExecutor, Client},
    movie::{
        credits::{MovieCredits, MovieCreditsResult}, details::MovieDetails, images::{MovieImages, MovieImagesResult}, popular::MoviePopular, search::MovieSearch
    },
    prelude::Command,
    tvshow::{
        aggregate_credits::{TVShowAggregateCredits, TVShowAggregateCreditsResult}, details::TVShowDetails, episode::details::TVShowEpisodeDetails, images::{TVShowImages, TVShowImagesResult}, popular::TVShowPopular, search::TVShowSearch, season::details::TVShowSeasonDetails
    },
};

pub enum Media {
    Movie,
    Series,
}

const TMDB_IMAGE_BASE: &str = "https://image.tmdb.org/t/p";

#[derive(Debug, Clone, Copy)]
pub enum PosterSize {
    W92,
    W154,
    W185,
    W342,
    W500,
    W780,
    Original,
}

impl PosterSize {
    pub fn as_str(&self) -> &'static str {
        match self {
            PosterSize::W92 => "w92",
            PosterSize::W154 => "w154",
            PosterSize::W185 => "w185",
            PosterSize::W342 => "w342",
            PosterSize::W500 => "w500",
            PosterSize::W780 => "w780",
            PosterSize::Original => "original",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum BackdropSize {
    W300,
    W780,
    W1280,
    Original,
}

impl BackdropSize {
    pub fn as_str(&self) -> &'static str {
        match self {
            BackdropSize::W300 => "w300",
            BackdropSize::W780 => "w780",
            BackdropSize::W1280 => "w1280",
            BackdropSize::Original => "original",
        }
    }
}

pub struct TmdbApiProvider {
    client: Client<ReqwestExecutor>,
}

impl TmdbApiProvider {
pub fn new() -> Self {
        let api_key = std::env::var("TMDB_API_KEY").unwrap_or_else(|_| String::new());
        let client = Client::new(api_key);
        Self { client }
    }

    /// Fetch a page of popular movies
    pub async fn list_popular_movies(
        &self,
        page: Option<u32>,
        language: Option<String>,
        region: Option<String>,
    ) -> Result<tmdb_api::common::PaginatedResult<tmdb_api::movie::MovieShort>, ProviderError> {

        let movie_popular = MoviePopular::default().with_language(language).with_region(region).with_page(page);
        let popular_movies_cmd = movie_popular.execute(&self.client).await;

        popular_movies_cmd.map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Fetch a page of popular TV shows
    pub async fn list_popular_tvshows(
        &self,
        page: Option<u32>,
        language: Option<String>,
    ) -> Result<tmdb_api::common::PaginatedResult<tmdb_api::tvshow::TVShowShort>, ProviderError> {
        let tv_show_popular = TVShowPopular::default().with_language(language).with_page(page);
        let popular_tvshows_cmd = tv_show_popular.execute(&self.client).await;

        popular_tvshows_cmd.map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Search for movies and return lightweight references
    pub async fn search_movies(
        &self,
        query: &str,
        year: Option<u16>,
    ) -> Result<Vec<MovieReference>, ProviderError> {
        let movie_search = MovieSearch::new(query.to_string());
        let search_cmd = MovieSearch::with_year(movie_search, year);
        let results = search_cmd
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))?;

        Ok(results
            .results
            .into_iter()
            .map(|r| MovieReference {
                id: MovieID::new_uuid(),
                library_id: LibraryID(uuid::Uuid::nil()), // Search results aren't tied to a library yet
                tmdb_id: r.inner.id as u64,
                title: MovieTitle::new(r.inner.title).unwrap(),
                details: MediaDetailsOption::Endpoint(format!("/movie/{}", r.inner.id)),
                endpoint: MovieURL::from(format!("/stream/movie/{}", r.inner.id)),
                file: crate::MediaFile::default(), // Placeholder - will be filled during scan
                theme_color: None,
            })
            .collect())
    }

    /// Search for TV series and return lightweight references
    pub async fn search_series(&self, query: &str) -> Result<Vec<SeriesReference>, ProviderError> {
        let search_cmd = TVShowSearch::new(query.to_string());
        let results = search_cmd
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))?;

        Ok(results
            .results
            .into_iter()
            .map(|r| SeriesReference {
                id: SeriesID::new_uuid(),
                library_id: LibraryID(uuid::Uuid::nil()), // Search results aren't tied to a library yet
                tmdb_id: r.inner.id as u64,
                title: SeriesTitle::new(r.inner.name).unwrap(),
                details: MediaDetailsOption::Endpoint(format!("/series/{}", r.inner.id)),
                endpoint: SeriesURL::from_string(format!("/series/{}", r.inner.id)),
                created_at: chrono::Utc::now(), // New results use current time
                theme_color: None,
            })
            .collect())
    }

    /// Get full movie details - returns TMDB type directly
    pub async fn get_movie(&self, id: u64) -> Result<tmdb_api::movie::Movie, ProviderError> {
        let details_cmd = MovieDetails::new(id);
        details_cmd
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get movie images
    pub async fn get_movie_images(&self, id: u64) -> Result<MovieImagesResult, ProviderError> {
        let images_cmd = MovieImages::new(id);
        images_cmd
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get movie credits (cast and crew)
    pub async fn get_movie_credits(&self, id: u64) -> Result<MovieCreditsResult, ProviderError> {
        let credits_cmd = MovieCredits::new(id);
        credits_cmd
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get full TV series details - returns TMDB type directly
    pub async fn get_series(&self, id: u64) -> Result<tmdb_api::tvshow::TVShow, ProviderError> {
        let details_cmd = TVShowDetails::new(id);
        details_cmd
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get TV series images
    pub async fn get_series_images(&self, id: u64) -> Result<TVShowImagesResult, ProviderError> {
        let images_cmd = TVShowImages::new(id);
        images_cmd
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get TV series credits (cast and crew)
    pub async fn get_series_credits(
        &self,
        id: u64,
    ) -> Result<TVShowAggregateCreditsResult, ProviderError> {
        let credits_cmd = TVShowAggregateCredits::new(id);
        credits_cmd
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get season details - returns TMDB type directly
    pub async fn get_season(
        &self,
        series_id: u64,
        season_number: u8,
    ) -> Result<tmdb_api::tvshow::Season, ProviderError> {
        let details_cmd = TVShowSeasonDetails::new(series_id, season_number as u64);
        details_cmd
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get episode details - returns TMDB type directly
    pub async fn get_episode(
        &self,
        series_id: u64,
        season_number: u8,
        episode_number: u8,
    ) -> Result<tmdb_api::tvshow::Episode, ProviderError> {
        let details_cmd =
            TVShowEpisodeDetails::new(series_id, season_number as u64, episode_number as u64);
        details_cmd
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get all movie genres
    pub async fn get_movie_genres(&self) -> Result<Vec<tmdb_api::genre::Genre>, ProviderError> {
        let genres_cmd = tmdb_api::genre::list::GenreList::movie();
        genres_cmd
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get all TV genres
    pub async fn get_tv_genres(&self) -> Result<Vec<tmdb_api::genre::Genre>, ProviderError> {
        let genres_cmd = tmdb_api::genre::list::GenreList::tv();
        genres_cmd
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Build a poster URL from a poster path
    pub fn get_poster_url(&self, path: &str, size: PosterSize) -> String {
        format!("{}/{}{}", TMDB_IMAGE_BASE, size.as_str(), path)
    }

    /// Build a backdrop URL from a backdrop path
    pub fn get_backdrop_url(&self, path: &str, size: BackdropSize) -> String {
        format!("{}/{}{}", TMDB_IMAGE_BASE, size.as_str(), path)
    }
}
