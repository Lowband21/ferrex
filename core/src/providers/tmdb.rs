use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use chrono::NaiveDate;
use std::sync::Arc;

use crate::media::{MediaType, ExternalMediaInfo};
use super::traits::{
    MetadataProvider, ProviderError, SearchResult, MediaQuery, 
    DetailedMediaInfo, MediaImages, CastMember, CrewMember
};

const TMDB_API_BASE: &str = "https://api.themoviedb.org/3";
const TMDB_IMAGE_BASE: &str = "https://image.tmdb.org/t/p";

pub struct TmdbProvider {
    api_key: String,
    client: Arc<Client>,
}

impl TmdbProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: Arc::new(Client::new()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct TmdbSearchResponse {
    results: Vec<TmdbSearchResult>,
    total_results: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct TmdbSearchResult {
    id: u32,
    title: Option<String>,
    name: Option<String>, // TV shows use "name" instead of "title"
    release_date: Option<String>,
    first_air_date: Option<String>, // TV shows
    poster_path: Option<String>,
    overview: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TmdbMovieDetails {
    id: u32,
    imdb_id: Option<String>,
    title: String,
    overview: Option<String>,
    release_date: Option<String>,
    runtime: Option<u32>,
    genres: Vec<TmdbGenre>,
    vote_average: Option<f32>,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TmdbTvDetails {
    id: u32,
    name: String,
    overview: Option<String>,
    first_air_date: Option<String>,
    genres: Vec<TmdbGenre>,
    vote_average: Option<f32>,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    external_ids: Option<TmdbExternalIds>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TmdbGenre {
    id: u32,
    name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TmdbExternalIds {
    imdb_id: Option<String>,
    tvdb_id: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TmdbCredits {
    cast: Vec<TmdbCast>,
    crew: Vec<TmdbCrew>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TmdbCast {
    id: u32,
    name: String,
    character: Option<String>,
    profile_path: Option<String>,
    order: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct TmdbCrew {
    id: u32,
    name: String,
    job: String,
    department: String,
    profile_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TmdbImages {
    posters: Vec<TmdbImage>,
    backdrops: Vec<TmdbImage>,
    logos: Option<Vec<TmdbImage>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TmdbImage {
    file_path: String,
    width: u32,
    height: u32,
    vote_average: Option<f32>,
}

#[async_trait]
impl MetadataProvider for TmdbProvider {
    async fn search(&self, query: &MediaQuery) -> Result<Vec<SearchResult>, ProviderError> {
        tracing::info!("TMDB search for: {:?}", query);
        
        let endpoint = match query.media_type {
            MediaType::Movie => "search/movie",
            MediaType::TvEpisode => "search/tv",
            MediaType::Unknown => "search/multi",
        };
        
        let mut params = vec![
            ("api_key", self.api_key.as_str()),
            ("query", query.title.as_str()),
        ];
        
        let year_str;
        if let Some(year) = query.year {
            year_str = year.to_string();
            let year_param = match query.media_type {
                MediaType::Movie => "year",
                MediaType::TvEpisode => "first_air_date_year",
                MediaType::Unknown => "year",
            };
            params.push((year_param, &year_str));
        }
        
        let url = format!("{}/{}", TMDB_API_BASE, endpoint);
        tracing::debug!("TMDB request URL: {}", url);
        
        let response = self.client
            .get(&url)
            .query(&params)
            .send()
            .await?;
        
        if response.status() == 401 {
            return Err(ProviderError::InvalidApiKey);
        }
        
        if response.status() == 429 {
            return Err(ProviderError::RateLimited);
        }
        
        if !response.status().is_success() {
            return Err(ProviderError::ApiError(
                format!("TMDB API returned status: {}", response.status())
            ));
        }
        
        let tmdb_response: TmdbSearchResponse = response.json().await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;
        
        tracing::info!("TMDB search returned {} results", tmdb_response.results.len());
        
        Ok(tmdb_response.results.into_iter().map(|r| {
            let (title, year) = if let Some(title) = r.title {
                // Movie
                let year = r.release_date
                    .as_ref()
                    .and_then(|d| d.split('-').next())
                    .and_then(|y| y.parse().ok());
                (title, year)
            } else if let Some(name) = r.name {
                // TV Show
                let year = r.first_air_date
                    .as_ref()
                    .and_then(|d| d.split('-').next())
                    .and_then(|y| y.parse().ok());
                (name, year)
            } else {
                ("Unknown".to_string(), None)
            };
            
            SearchResult {
                id: r.id.to_string(),
                title,
                year,
                media_type: query.media_type.clone(),
                poster_path: r.poster_path,
                overview: r.overview,
            }
        }).collect())
    }
    
    async fn get_metadata(&self, provider_id: &str, media_type: MediaType) -> Result<DetailedMediaInfo, ProviderError> {
        let id: u32 = provider_id.parse()
            .map_err(|_| ProviderError::ParseError("Invalid TMDB ID".to_string()))?;
        
        match media_type {
            MediaType::Movie => self.get_movie_metadata(id).await,
            MediaType::TvEpisode => self.get_tv_metadata(id).await,
            MediaType::Unknown => Err(ProviderError::ApiError("Unknown media type".to_string())),
        }
    }
    
    fn name(&self) -> &'static str {
        "TMDB"
    }
    
    fn image_base_url(&self) -> &str {
        TMDB_IMAGE_BASE
    }
}

impl TmdbProvider {
    async fn get_movie_metadata(&self, id: u32) -> Result<DetailedMediaInfo, ProviderError> {
        // Fetch movie details
        let details_url = format!("{}/movie/{}", TMDB_API_BASE, id);
        let details_response = self.client
            .get(&details_url)
            .query(&[("api_key", &self.api_key)])
            .send()
            .await?;
        
        if !details_response.status().is_success() {
            return Err(ProviderError::NotFound);
        }
        
        let details: TmdbMovieDetails = details_response.json().await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;
        
        // Fetch credits
        let credits_url = format!("{}/movie/{}/credits", TMDB_API_BASE, id);
        let credits_response = self.client
            .get(&credits_url)
            .query(&[("api_key", &self.api_key)])
            .send()
            .await?;
        
        let credits: TmdbCredits = credits_response.json().await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;
        
        // Fetch images
        let images_url = format!("{}/movie/{}/images", TMDB_API_BASE, id);
        let images_response = self.client
            .get(&images_url)
            .query(&[("api_key", &self.api_key)])
            .send()
            .await?;
        
        let images: TmdbImages = images_response.json().await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;
        
        // Convert to our format
        let external_info = ExternalMediaInfo {
            tmdb_id: Some(id),
            tvdb_id: None,
            imdb_id: details.imdb_id,
            description: details.overview,
            poster_url: details.poster_path,
            backdrop_url: details.backdrop_path,
            genres: details.genres.into_iter().map(|g| g.name).collect(),
            rating: details.vote_average,
            release_date: details.release_date
                .and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok()),
            show_description: None,
            show_poster_url: None,
            season_poster_url: None,
            episode_still_url: None,
        };
        
        let cast = credits.cast.into_iter().map(|c| CastMember {
            id: c.id,
            name: c.name,
            character: c.character,
            profile_path: c.profile_path,
            order: c.order,
        }).collect();
        
        let crew = credits.crew.into_iter().map(|c| CrewMember {
            id: c.id,
            name: c.name,
            job: c.job,
            department: c.department,
            profile_path: c.profile_path,
        }).collect();
        
        let media_images = MediaImages {
            posters: images.posters.into_iter()
                .map(|i| i.file_path)
                .collect(),
            backdrops: images.backdrops.into_iter()
                .map(|i| i.file_path)
                .collect(),
            logos: images.logos.map(|logos| 
                logos.into_iter().map(|i| i.file_path).collect()
            ).unwrap_or_default(),
        };
        
        Ok(DetailedMediaInfo {
            external_info,
            cast,
            crew,
            images: media_images,
        })
    }
    
    async fn get_tv_metadata(&self, _id: u32) -> Result<DetailedMediaInfo, ProviderError> {
        // Similar implementation for TV shows
        // For now, return a basic implementation
        Err(ProviderError::ApiError("TV metadata not implemented yet".to_string()))
    }
}