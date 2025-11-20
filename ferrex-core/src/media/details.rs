use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;
use crate::LibraryType;


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TmdbDetails {
    Movie(EnhancedMovieDetails),
    Series(EnhancedSeriesDetails),
    Season(SeasonDetails),
    Episode(EpisodeDetails),
}

/// Enhanced metadata that includes images, credits, and additional information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnhancedMovieDetails {
    // Basic details
    pub id: u64,
    pub title: String,
    pub overview: Option<String>,
    pub release_date: Option<String>,
    pub runtime: Option<u32>,
    pub vote_average: Option<f32>,
    pub vote_count: Option<u32>,
    pub popularity: Option<f32>,
    pub genres: Vec<String>,
    pub production_companies: Vec<String>,

    // Media assets
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub logo_path: Option<String>,
    pub images: MediaImages,

    // Credits
    pub cast: Vec<CastMember>,
    pub crew: Vec<CrewMember>,

    // Additional
    pub videos: Vec<Video>,
    pub keywords: Vec<String>,
    pub external_ids: ExternalIds,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnhancedSeriesDetails {
    // Basic details
    pub id: u64,
    pub name: String,
    pub overview: Option<String>,
    pub first_air_date: Option<String>,
    pub last_air_date: Option<String>,
    pub number_of_seasons: Option<u32>,
    pub number_of_episodes: Option<u32>,
    pub vote_average: Option<f32>,
    pub vote_count: Option<u32>,
    pub popularity: Option<f32>,
    pub genres: Vec<String>,
    pub networks: Vec<String>,

    // Media assets
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub logo_path: Option<String>,
    pub images: MediaImages,

    // Credits
    pub cast: Vec<CastMember>,
    pub crew: Vec<CrewMember>,

    // Additional
    pub videos: Vec<Video>,
    pub keywords: Vec<String>,
    pub external_ids: ExternalIds,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SeasonDetails {
    pub id: u64,
    pub season_number: u8,
    pub name: String,
    pub overview: Option<String>,
    pub air_date: Option<String>,
    pub episode_count: u32,
    pub poster_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EpisodeDetails {
    pub id: u64,
    pub episode_number: u8,
    pub season_number: u8,
    pub name: String,
    pub overview: Option<String>,
    pub air_date: Option<String>,
    pub runtime: Option<u32>,
    pub still_path: Option<String>,
    pub vote_average: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageMetadata {
    pub file_path: String,
    pub width: u64,
    pub height: u64,
    pub aspect_ratio: f64,
    pub iso_639_1: Option<String>,
    pub vote_average: f64,
    pub vote_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageWithMetadata {
    pub endpoint: String,
    pub metadata: ImageMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct MediaImages {
    pub posters: Vec<ImageWithMetadata>,
    pub backdrops: Vec<ImageWithMetadata>,
    pub logos: Vec<ImageWithMetadata>,
    pub stills: Vec<ImageWithMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CastMember {
    pub id: u64,
    pub name: String,
    pub character: String,
    pub profile_path: Option<String>,
    pub order: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CrewMember {
    pub id: u64,
    pub name: String,
    pub job: String,
    pub department: String,
    pub profile_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Video {
    pub key: String,
    pub name: String,
    pub site: String,
    pub video_type: String,
    pub official: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ExternalIds {
    pub imdb_id: Option<String>,
    pub tvdb_id: Option<u32>,
    pub facebook_id: Option<String>,
    pub instagram_id: Option<String>,
    pub twitter_id: Option<String>,
}

// Media enum removed - duplicate of definition in tmdb_api_provider.rs

// Library reference type - no media references
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryReference {
    pub id: Uuid,
    pub name: String,
    pub library_type: crate::LibraryType,
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MediaDetailsOption {
    Endpoint(String),
    Details(TmdbDetails),
}

impl MediaDetailsOption {
    /// Extract release year from movie details if available
    pub fn get_release_year(&self) -> Option<u16> {
        match self {
            MediaDetailsOption::Endpoint(_) => None,
            MediaDetailsOption::Details(details) => match details {
                TmdbDetails::Movie(movie) => movie
                    .release_date
                    .as_ref()
                    .and_then(|date| date.split("-").next())
                    .and_then(|year| year.parse().ok()),
                _ => None,
            },
        }
    }
}

