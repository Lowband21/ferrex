use crate::types::details::MediaDetailsOption;
use crate::types::files::MediaFile;
use crate::types::ids::{LibraryId, MovieID, SeriesID};
use crate::types::media::{MovieReference, SeriesReference};
use crate::types::titles::{MovieTitle, SeriesTitle};
use crate::types::urls::{MovieURL, SeriesURL};
use std::collections::HashSet;
use std::fmt;

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
    client::{Client, reqwest::ReqwestExecutor},
    movie::{
        alternative_titles::{
            MovieAlternativeTitles, MovieAlternativeTitlesResult,
        },
        credits::{MovieCredits, MovieCreditsResult},
        details::MovieDetails,
        external_ids::{MovieExternalIds, MovieExternalIdsResult},
        images::{MovieImages, MovieImagesResult},
        keywords::{MovieKeywords, MovieKeywordsResult},
        popular::MoviePopular,
        recommendations::MovieRecommendations,
        release_dates::{MovieReleaseDates, MovieReleaseDatesResult},
        search::MovieSearch,
        similar::GetSimilarMovies,
        translations::{MovieTranslations, MovieTranslationsResult},
        videos::{MovieVideos, MovieVideosResult},
    },
    prelude::Command,
    tvshow::{
        aggregate_credits::{
            TVShowAggregateCredits, TVShowAggregateCreditsResult,
        },
        content_rating::{
            ContentRatingResult as TvContentRatingResult, TVShowContentRating,
        },
        details::TVShowDetails,
        episode::details::TVShowEpisodeDetails,
        images::{TVShowImages, TVShowImagesResult},
        popular::TVShowPopular,
        search::TVShowSearch,
        season::details::TVShowSeasonDetails,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl fmt::Debug for TmdbApiProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TmdbApiProvider")
            .field("client", &"tmdb_api::Client<ReqwestExecutor>")
            .finish()
    }
}

impl Default for TmdbApiProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TmdbApiProvider {
    pub fn new() -> Self {
        let api_key =
            std::env::var("TMDB_API_KEY").unwrap_or_else(|_| String::new());
        let client = Client::new(api_key);
        Self { client }
    }

    /// Fetch a page of popular movies
    pub async fn list_popular_movies(
        &self,
        page: Option<u32>,
        language: Option<String>,
        region: Option<String>,
    ) -> Result<
        tmdb_api::common::PaginatedResult<tmdb_api::movie::MovieShort>,
        ProviderError,
    > {
        let movie_popular = MoviePopular::default()
            .with_language(language)
            .with_region(region)
            .with_page(page);
        let popular_movies_cmd = movie_popular.execute(&self.client).await;

        popular_movies_cmd.map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Fetch a page of popular TV shows
    pub async fn list_popular_tvshows(
        &self,
        page: Option<u32>,
        language: Option<String>,
    ) -> Result<
        tmdb_api::common::PaginatedResult<tmdb_api::tvshow::TVShowShort>,
        ProviderError,
    > {
        let tv_show_popular = TVShowPopular::default()
            .with_language(language)
            .with_page(page);
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
                library_id: LibraryId(uuid::Uuid::nil()), // Search results aren't tied to a library yet
                tmdb_id: r.inner.id,
                title: MovieTitle::new(r.inner.title).unwrap(),
                details: MediaDetailsOption::Endpoint(format!(
                    "/movie/{}",
                    r.inner.id
                )),
                endpoint: MovieURL::from(format!(
                    "/stream/movie/{}",
                    r.inner.id
                )),
                file: MediaFile::default(), // Placeholder - will be filled during scan
                theme_color: None,
            })
            .collect())
    }

    /// Search for TV series and return lightweight references
    pub async fn search_series(
        &self,
        query: &str,
        year: Option<u16>,
        region: Option<&str>,
    ) -> Result<Vec<SeriesReference>, ProviderError> {
        let mut search_cmd = TVShowSearch::new(query.to_string());
        if year.is_some() {
            search_cmd = search_cmd.with_first_air_date_year(year);
        }

        let results = search_cmd
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))?;

        let mut prioritized = Vec::new();
        let mut others = Vec::new();
        let mut seen = HashSet::new();
        let region = region.map(|raw| raw.trim().to_ascii_uppercase());

        for item in results.results {
            if !seen.insert(item.inner.id) {
                continue;
            }

            let title = SeriesTitle::new(item.inner.name.clone())
                .map_err(|e| ProviderError::ParseError(e.to_string()))?;

            let reference = SeriesReference {
                id: SeriesID::new_uuid(),
                library_id: LibraryId(uuid::Uuid::nil()),
                tmdb_id: item.inner.id,
                title,
                details: MediaDetailsOption::Endpoint(format!(
                    "/series/{}",
                    item.inner.id
                )),
                endpoint: SeriesURL::from_string(format!(
                    "/series/{}",
                    item.inner.id
                )),
                discovered_at: chrono::Utc::now(),
                created_at: chrono::Utc::now(),
                theme_color: None,
            };

            let mut origin_countries = item
                .inner
                .origin_country
                .iter()
                .map(|country| country.trim().to_ascii_uppercase())
                .filter(|country| !country.is_empty())
                .collect::<Vec<_>>();

            origin_countries.sort();
            origin_countries.dedup();

            let matches_region = region
                .as_ref()
                .map(|target| origin_countries.iter().any(|c| c == target))
                .unwrap_or(false);

            if matches_region {
                prioritized.push(reference);
            } else {
                others.push(reference);
            }
        }

        if prioritized.is_empty() {
            Ok(others)
        } else {
            prioritized.extend(others);
            Ok(prioritized)
        }
    }

    /// Get full movie details - returns TMDB type directly
    pub async fn get_movie(
        &self,
        id: u64,
    ) -> Result<tmdb_api::movie::Movie, ProviderError> {
        let details_cmd = MovieDetails::new(id);
        details_cmd
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get regional release dates for a movie (contains certifications)
    pub async fn get_movie_release_dates(
        &self,
        id: u64,
    ) -> Result<MovieReleaseDatesResult, ProviderError> {
        MovieReleaseDates::new(id)
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get movie keywords
    pub async fn get_movie_keywords(
        &self,
        id: u64,
    ) -> Result<MovieKeywordsResult, ProviderError> {
        MovieKeywords::new(id)
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get movie videos (trailers, clips, etc.)
    pub async fn get_movie_videos(
        &self,
        id: u64,
    ) -> Result<MovieVideosResult, ProviderError> {
        MovieVideos::new(id)
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get movie translations
    pub async fn get_movie_translations(
        &self,
        id: u64,
    ) -> Result<MovieTranslationsResult, ProviderError> {
        MovieTranslations::new(id)
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get movie alternative titles
    pub async fn get_movie_alternative_titles(
        &self,
        id: u64,
    ) -> Result<MovieAlternativeTitlesResult, ProviderError> {
        MovieAlternativeTitles::new(id)
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get movie recommendations (first page)
    pub async fn get_movie_recommendations(
        &self,
        id: u64,
    ) -> Result<
        tmdb_api::common::PaginatedResult<tmdb_api::movie::MovieShort>,
        ProviderError,
    > {
        MovieRecommendations::new(id)
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get similar movies (first page)
    pub async fn get_movie_similar(
        &self,
        id: u64,
    ) -> Result<
        tmdb_api::common::PaginatedResult<tmdb_api::movie::MovieShort>,
        ProviderError,
    > {
        GetSimilarMovies::new(id)
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get movie external IDs
    pub async fn get_movie_external_ids(
        &self,
        id: u64,
    ) -> Result<MovieExternalIdsResult, ProviderError> {
        MovieExternalIds::new(id)
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get movie images
    pub async fn get_movie_images(
        &self,
        id: u64,
    ) -> Result<MovieImagesResult, ProviderError> {
        let images_cmd = MovieImages::new(id);
        images_cmd
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get movie credits (cast and crew)
    pub async fn get_movie_credits(
        &self,
        id: u64,
    ) -> Result<MovieCreditsResult, ProviderError> {
        let credits_cmd = MovieCredits::new(id);
        credits_cmd
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get full TV series details - returns TMDB type directly
    pub async fn get_series(
        &self,
        id: u64,
    ) -> Result<tmdb_api::tvshow::TVShow, ProviderError> {
        let details_cmd = TVShowDetails::new(id);
        details_cmd
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get TV content ratings (per region)
    pub async fn get_tv_content_ratings(
        &self,
        id: u64,
    ) -> Result<TvContentRatingResult, ProviderError> {
        TVShowContentRating::new(id)
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get TV series images
    pub async fn get_series_images(
        &self,
        id: u64,
    ) -> Result<TVShowImagesResult, ProviderError> {
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
        let details_cmd =
            TVShowSeasonDetails::new(series_id, season_number as u64);
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
        let details_cmd = TVShowEpisodeDetails::new(
            series_id,
            season_number as u64,
            episode_number as u64,
        );
        details_cmd
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get all movie genres
    pub async fn get_movie_genres(
        &self,
    ) -> Result<Vec<tmdb_api::genre::Genre>, ProviderError> {
        let genres_cmd = tmdb_api::genre::list::GenreList::movie();
        genres_cmd
            .execute(&self.client)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get all TV genres
    pub async fn get_tv_genres(
        &self,
    ) -> Result<Vec<tmdb_api::genre::Genre>, ProviderError> {
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
