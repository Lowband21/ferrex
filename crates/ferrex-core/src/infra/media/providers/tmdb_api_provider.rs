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

use super::tmdb_discover::{
    DiscoverMovieItem, DiscoverMovieQuery, DiscoverPage, DiscoverTvItem,
    DiscoverTvQuery,
};
use ferrex_model::ImageSize;
use serde::Deserialize;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tmdb_api::{
    client::{Client, reqwest::Client as ReqwestClient},
    common::{
        EntityResults, LanguagePageParams, LanguageParams, PaginatedResult,
        release_date::LocatedReleaseDates, video::Video,
    },
    genre::Response as GenreResponse,
    movie::{
        Movie as TmdbMovieDetails, MovieShort,
        alternative_titles::{
            Params as CountryParams, Response as MovieAltTitleResponse,
        },
        credits::GetMovieCreditsResponse,
        external_ids::MovieExternalIds,
        images::GetMovieImagesResponse,
        keywords::Response as KeywordsResponse,
        popular::Params,
        search::Params as MovieSearchParams,
        translations::Response as TranslationResponse,
    },
    tvshow::{
        TVShowShort,
        aggregate_credits::TVShowAggregateCredits,
        content_rating::Response as SeriesContentRatingResponse,
        images::{GetTVshowImagesResponse, Params as SeriesImageParams},
        search::Params as SeriesSearchParams,
    },
};
use tracing::error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Media {
    Movie,
    Series,
}

const TMDB_IMAGE_BASE: &str = "https://image.tmdb.org/t/p";
const TMDB_V3_BASE: &str = "https://api.themoviedb.org/3";

pub struct TmdbApiProvider {
    client: Client<ReqwestClient>,
    http: reqwest::Client,
    api_key: String,
    env_language: Option<String>,
    env_region: Option<String>,
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
        let env_language = std::env::var("TMDB_LANG").ok();
        let env_region = std::env::var("TMDB_REGION").ok();

        let client = Client::<ReqwestClient>::new(api_key.clone());

        Self {
            client,
            http: reqwest::Client::new(),
            api_key,
            env_language,
            env_region,
        }
    }

    async fn get_tmdb_json<Q, T>(
        &self,
        url: &str,
        query: &Q,
    ) -> Result<T, ProviderError>
    where
        Q: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        let response = self.http.get(url).query(query).send().await?;

        let status = response.status();
        if status.is_success() {
            return response.json::<T>().await.map_err(ProviderError::from);
        }

        #[derive(Debug, Deserialize)]
        struct TmdbErrorBody {
            #[serde(default)]
            status_message: Option<String>,
        }

        let message = response
            .json::<TmdbErrorBody>()
            .await
            .ok()
            .and_then(|body| body.status_message)
            .unwrap_or_else(|| {
                format!("TMDB request failed with status {}", status)
            });

        match status.as_u16() {
            401 => Err(ProviderError::InvalidApiKey),
            404 => Err(ProviderError::NotFound),
            429 => Err(ProviderError::RateLimited),
            _ => Err(ProviderError::ApiError(message)),
        }
    }

    pub async fn discover_movies_by_year(
        &self,
        year: i32,
        page: u32,
        language: Option<&str>,
        region: Option<&str>,
    ) -> Result<DiscoverPage<DiscoverMovieItem>, ProviderError> {
        let query = DiscoverMovieQuery {
            api_key: &self.api_key,
            sort_by: "popularity.desc",
            include_adult: false,
            include_video: false,
            page: page.max(1),
            primary_release_year: year,
            language: language.or(self.env_language.as_deref()),
            region: region.or(self.env_region.as_deref()),
        };

        self.get_tmdb_json(&format!("{TMDB_V3_BASE}/discover/movie"), &query)
            .await
    }

    pub async fn discover_tv_by_year(
        &self,
        year: i32,
        page: u32,
        language: Option<&str>,
    ) -> Result<DiscoverPage<DiscoverTvItem>, ProviderError> {
        let query = DiscoverTvQuery {
            api_key: &self.api_key,
            sort_by: "popularity.desc",
            include_adult: false,
            page: page.max(1),
            first_air_date_year: year,
            language: language.or(self.env_language.as_deref()),
        };

        self.get_tmdb_json(&format!("{TMDB_V3_BASE}/discover/tv"), &query)
            .await
    }

    /// Fetch a page of popular movies
    pub async fn list_popular_movies(
        &self,
        page: Option<u32>,
        language: Option<&str>,
        region: Option<&str>,
    ) -> Result<PaginatedResult<MovieShort>, ProviderError> {
        let params = Params {
            language: language.or(self.env_language.as_deref()).map(Into::into),
            page,
            region: region.or(self.env_region.as_deref()).map(Into::into),
        };

        let popular_movies_cmd = self.client.list_popular_movies(&params).await;

        popular_movies_cmd.map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Fetch a  of popular TV shows
    pub async fn list_popular_tvshows(
        &self,
        page: Option<u32>,
        language: Option<&str>,
    ) -> Result<PaginatedResult<TVShowShort>, ProviderError> {
        let params = LanguagePageParams {
            language: language.or(self.env_language.as_deref()).map(Into::into),
            page,
        };

        let popular_tvshows_cmd =
            self.client.list_popular_tvshows(&params).await;

        popular_tvshows_cmd.map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Search for movies and return lightweight references
    pub async fn search_movies(
        &self,
        query: &str,
        year: Option<u16>,
        language: Option<&str>,
        region: Option<&str>,
    ) -> Result<PaginatedResult<MovieShort>, ProviderError> {
        let params = MovieSearchParams {
            year,
            language: language.or(self.env_language.as_deref()).map(Into::into),
            region: region.or(self.env_region.as_deref()).map(Into::into),
            ..Default::default()
        };

        let results = self.client.search_movies(query, &params).await;

        results.map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Search for TV series and return lightweight references
    pub async fn search_series(
        &self,
        query: &str,
        year: Option<u16>,
        language: Option<&str>,
        region: Option<&str>,
    ) -> Result<PaginatedResult<TVShowShort>, ProviderError> {
        let params = SeriesSearchParams {
            first_air_date_year: year,
            language: language.or(self.env_language.as_deref()).map(Into::into),
            region: region.or(self.env_region.as_deref()).map(Into::into),
            ..Default::default()
        };

        let results = self.client.search_tvshows(query, &params).await;

        results.map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get full movie details - returns TMDB type directly
    pub async fn get_movie(
        &self,
        id: u64,
        language: Option<&str>,
    ) -> Result<TmdbMovieDetails, ProviderError> {
        let params = LanguageParams {
            language: language.or(self.env_language.as_deref()).map(Into::into),
        };

        let details = self.client.get_movie_details(id, &params).await;

        details.map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get regional release dates for a movie (contains certifications)
    pub async fn get_movie_release_dates(
        &self,
        id: u64,
    ) -> Result<EntityResults<Vec<LocatedReleaseDates>>, ProviderError> {
        self.client
            .get_movie_release_dates(id)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get movie keywords
    pub async fn get_movie_keywords(
        &self,
        id: u64,
    ) -> Result<KeywordsResponse, ProviderError> {
        self.client
            .get_movie_keywords(id)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get movie videos (trailers, clips, etc.)
    pub async fn get_movie_videos(
        &self,
        id: u64,
        language: Option<&str>,
    ) -> Result<EntityResults<Vec<Video>>, ProviderError> {
        let params = LanguageParams {
            language: language.or(self.env_language.as_deref()).map(Into::into),
        };

        self.client
            .get_movie_videos(id, &params)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get movie translations
    pub async fn get_movie_translations(
        &self,
        id: u64,
    ) -> Result<TranslationResponse, ProviderError> {
        self.client
            .get_movie_translations(id)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get movie alternative titles
    pub async fn get_movie_alternative_titles(
        &self,
        id: u64,
        country: Option<&str>,
    ) -> Result<MovieAltTitleResponse, ProviderError> {
        let params = CountryParams {
            country: country.or(self.env_region.as_deref()).map(Into::into),
        };

        self.client
            .get_movie_alternative_titles(id, &params)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get movie recommendations
    pub async fn get_movie_recommendations(
        &self,
        id: u64,
        language: Option<&str>,
        page: Option<u32>,
    ) -> Result<PaginatedResult<MovieShort>, ProviderError> {
        let params = LanguagePageParams {
            language: language.or(self.env_language.as_deref()).map(Into::into),
            page,
        };

        self.client
            .get_movie_recommendations(id, &params)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get similar movies
    pub async fn get_movie_similar(
        &self,
        id: u64,
        language: Option<&str>,
        page: Option<u32>,
    ) -> Result<PaginatedResult<MovieShort>, ProviderError> {
        let params = LanguagePageParams {
            language: language.map(|l| l.into()),
            page,
        };

        self.client
            .get_similar_movies(id, &params)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get movie external IDs
    pub async fn get_movie_external_ids(
        &self,
        id: u64,
    ) -> Result<MovieExternalIds, ProviderError> {
        self.client
            .get_movie_external_ids(id)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get movie images
    pub async fn get_movie_images(
        &self,
        id: u64,
        language: Option<&str>,
    ) -> Result<GetMovieImagesResponse, ProviderError> {
        let params = LanguageParams {
            language: language.or(self.env_language.as_deref()).map(Into::into),
        };

        self.client
            .get_movie_images(id, &params)
            .await
            .map_err(|e| {
                error!("Failed to map movie images for id {}", id);
                ProviderError::ApiError(e.to_string())
            })
    }

    /// Get movie credits (cast and crew)
    pub async fn get_movie_credits(
        &self,
        id: u64,
        language: Option<&str>,
    ) -> Result<GetMovieCreditsResponse, ProviderError> {
        let params = LanguageParams {
            language: language.or(self.env_language.as_deref()).map(Into::into),
        };

        self.client
            .get_movie_credits(id, &params)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get full TV series details - returns TMDB type directly
    pub async fn get_series(
        &self,
        id: u64,
        language: Option<&str>,
    ) -> Result<tmdb_api::tvshow::TVShow, ProviderError> {
        let params = LanguageParams {
            language: language.or(self.env_language.as_deref()).map(Into::into),
        };

        self.client
            .get_tvshow_details(id, &params)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get TV content ratings (per region)
    pub async fn get_tv_content_ratings(
        &self,
        id: u64,
    ) -> Result<SeriesContentRatingResponse, ProviderError> {
        self.client
            .get_tvshow_content_ratings(id)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get TV series images
    pub async fn get_series_images(
        &self,
        id: u64,
        language: Option<&str>,
    ) -> Result<GetTVshowImagesResponse, ProviderError> {
        let params = SeriesImageParams {
            language: language.or(self.env_language.as_deref()).map(Into::into),
            include_image_language: Some(
                format!("{},null", language.unwrap_or("en")).into(),
            ),
        };

        self.client
            .get_tvshow_images(id, &params)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get TV series credits (cast and crew)
    pub async fn get_series_credits(
        &self,
        id: u64,
        language: Option<&str>,
    ) -> Result<TVShowAggregateCredits, ProviderError> {
        let params = LanguageParams {
            language: language.or(self.env_language.as_deref()).map(Into::into),
        };

        self.client
            .get_tvshow_aggregate_credits(id, &params)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get season details - returns TMDB type directly
    pub async fn get_season(
        &self,
        series_id: u64,
        season_number: u16,
        language: Option<&str>,
    ) -> Result<tmdb_api::tvshow::Season, ProviderError> {
        let params = LanguageParams {
            language: language.or(self.env_language.as_deref()).map(Into::into),
        };

        self.client
            .get_tvshow_season_details(series_id, season_number.into(), &params)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get episode details - returns TMDB type directly
    pub async fn get_episode(
        &self,
        series_id: u64,
        season_number: u16,
        episode_number: u16,
        language: Option<&str>,
    ) -> Result<tmdb_api::tvshow::Episode, ProviderError> {
        let params = LanguageParams {
            language: language.or(self.env_language.as_deref()).map(Into::into),
        };

        self.client
            .get_tvshow_episode_details(
                series_id,
                season_number.into(),
                episode_number.into(),
                &params,
            )
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get all movie genres
    pub async fn get_movie_genres(
        &self,
        language: Option<&str>,
    ) -> Result<GenreResponse, ProviderError> {
        let params = LanguageParams {
            language: language.or(self.env_language.as_deref()).map(Into::into),
        };

        self.client
            .list_movie_genres(&params)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Get all TV genres
    pub async fn get_tv_genres(
        &self,
        language: Option<&str>,
    ) -> Result<GenreResponse, ProviderError> {
        let params = LanguageParams {
            language: language.or(self.env_language.as_deref()).map(Into::into),
        };

        self.client
            .list_tvshow_genres(&params)
            .await
            .map_err(|e| ProviderError::ApiError(e.to_string()))
    }

    /// Build a poster URL from a poster path
    pub fn get_poster_url(&self, path: &str, size: ImageSize) -> String {
        format!("{}/{}{}", TMDB_IMAGE_BASE, size.to_tmdb_param(), path)
    }
}
