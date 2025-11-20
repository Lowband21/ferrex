use crate::MediaImages;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use super::LibraryID;

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub enum TmdbDetails {
    Movie(EnhancedMovieDetails),
    Series(EnhancedSeriesDetails),
    Season(SeasonDetails),
    Episode(EpisodeDetails),
}

/// Enhanced metadata that includes images, credits, and additional information
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
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

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
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

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq, Hash))]
pub struct SeasonDetails {
    pub id: u64,
    pub season_number: u8,
    pub name: String,
    pub overview: Option<String>,
    pub air_date: Option<String>,
    pub episode_count: u32,
    pub poster_path: Option<String>,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
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

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq, Hash))]
pub struct CastMember {
    pub id: u64,
    pub name: String,
    pub character: String,
    pub profile_path: Option<String>,
    pub order: u32,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq, Hash))]
pub struct CrewMember {
    pub id: u64,
    pub name: String,
    pub job: String,
    pub department: String,
    pub profile_path: Option<String>,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq, Hash))]
pub struct Video {
    pub key: String,
    pub name: String,
    pub site: String,
    pub video_type: String,
    pub official: bool,
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    Default,
    PartialEq,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq, Hash))]
pub struct ExternalIds {
    pub imdb_id: Option<String>,
    pub tvdb_id: Option<u32>,
    pub facebook_id: Option<String>,
    pub instagram_id: Option<String>,
    pub twitter_id: Option<String>,
}

// Library reference type - no media references
#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
#[rkyv(derive(Debug, PartialEq, Eq, Hash))]
pub struct LibraryReference {
    pub id: LibraryID,
    pub name: String,
    pub library_type: crate::LibraryType,
    #[rkyv(with = crate::rkyv_wrappers::VecPathBuf)]
    pub paths: Vec<PathBuf>,
}

pub trait MediaDetailsOptionLike {
    fn get_release_year(&self) -> Option<u16>;
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub enum MediaDetailsOption {
    Endpoint(String),
    Details(TmdbDetails),
}

impl MediaDetailsOptionLike for MediaDetailsOption {
    fn get_release_year(&self) -> Option<u16> {
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

impl MediaDetailsOptionLike for ArchivedMediaDetailsOption {
    fn get_release_year(&self) -> Option<u16> {
        match self {
            ArchivedMediaDetailsOption::Endpoint(_) => None,
            ArchivedMediaDetailsOption::Details(details) => match details {
                ArchivedTmdbDetails::Movie(movie) => movie
                    .release_date
                    .as_ref()
                    .and_then(|date| date.split("-").next())
                    .and_then(|year| year.parse().ok()),
                _ => None,
            },
        }
    }
}
