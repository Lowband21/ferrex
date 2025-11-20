use crate::MediaImages;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use super::LibraryID;

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Default,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct ReleaseDateEntry {
    pub certification: Option<String>,
    pub release_date: Option<String>,
    pub release_type: Option<i32>,
    pub note: Option<String>,
    pub iso_639_1: Option<String>,
    #[serde(default)]
    pub descriptors: Vec<String>,
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Default,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct ReleaseDatesByCountry {
    pub iso_3166_1: String,
    #[serde(default)]
    pub release_dates: Vec<ReleaseDateEntry>,
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Default,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct ContentRating {
    pub iso_3166_1: String,
    pub rating: Option<String>,
    pub rating_system: Option<String>,
    #[serde(default)]
    pub descriptors: Vec<String>,
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Default,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct AlternativeTitle {
    pub title: String,
    pub iso_3166_1: Option<String>,
    pub title_type: Option<String>,
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Default,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct Translation {
    pub iso_3166_1: String,
    pub iso_639_1: String,
    pub name: Option<String>,
    pub english_name: Option<String>,
    pub title: Option<String>,
    pub overview: Option<String>,
    pub homepage: Option<String>,
    pub tagline: Option<String>,
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
    Default,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct CollectionInfo {
    pub id: u64,
    pub name: String,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
    Default,
)]
#[rkyv(derive(Debug, PartialEq, Eq, Hash))]
pub struct Keyword {
    pub id: u64,
    pub name: String,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct EpisodeGroupMembership {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub group_type: Option<String>,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct GenreInfo {
    pub id: u64,
    pub name: String,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct ProductionCompany {
    pub id: u64,
    pub name: String,
    pub origin_country: Option<String>,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct ProductionCountry {
    pub iso_3166_1: String,
    pub name: String,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct SpokenLanguage {
    pub iso_639_1: Option<String>,
    pub name: String,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct NetworkInfo {
    pub id: u64,
    pub name: String,
    pub origin_country: Option<String>,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct RelatedMediaRef {
    pub tmdb_id: u64,
    pub title: Option<String>,
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Hash,
    Default,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq, Hash))]
pub struct PersonExternalIds {
    pub imdb_id: Option<String>,
    pub facebook_id: Option<String>,
    pub instagram_id: Option<String>,
    pub twitter_id: Option<String>,
    pub wikidata_id: Option<String>,
    pub tiktok_id: Option<String>,
    pub youtube_id: Option<String>,
}

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
    pub original_title: Option<String>,
    pub overview: Option<String>,
    pub release_date: Option<String>,
    pub runtime: Option<u32>,
    pub vote_average: Option<f32>,
    pub vote_count: Option<u32>,
    pub popularity: Option<f32>,
    pub content_rating: Option<String>,
    #[serde(default)]
    pub content_ratings: Vec<ContentRating>,
    #[serde(default)]
    pub release_dates: Vec<ReleaseDatesByCountry>,
    #[serde(default)]
    pub genres: Vec<GenreInfo>,
    #[serde(default)]
    pub spoken_languages: Vec<SpokenLanguage>,
    #[serde(default)]
    pub production_companies: Vec<ProductionCompany>,
    #[serde(default)]
    pub production_countries: Vec<ProductionCountry>,
    pub homepage: Option<String>,
    pub status: Option<String>,
    pub tagline: Option<String>,
    pub budget: Option<u64>,
    pub revenue: Option<u64>,

    // Media assets
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub logo_path: Option<String>,
    pub images: MediaImages,

    // Credits
    pub cast: Vec<CastMember>,
    pub crew: Vec<CrewMember>,

    // Additional
    #[serde(default)]
    pub videos: Vec<Video>,
    #[serde(default)]
    pub keywords: Vec<Keyword>,
    pub external_ids: ExternalIds,
    #[serde(default)]
    pub alternative_titles: Vec<AlternativeTitle>,
    #[serde(default)]
    pub translations: Vec<Translation>,
    pub collection: Option<CollectionInfo>,
    #[serde(default)]
    pub recommendations: Vec<RelatedMediaRef>,
    #[serde(default)]
    pub similar: Vec<RelatedMediaRef>,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct EnhancedSeriesDetails {
    // Basic details
    pub id: u64,
    pub name: String,
    pub original_name: Option<String>,
    pub overview: Option<String>,
    pub first_air_date: Option<String>,
    pub last_air_date: Option<String>,
    pub number_of_seasons: Option<u32>,
    pub number_of_episodes: Option<u32>,
    pub vote_average: Option<f32>,
    pub vote_count: Option<u32>,
    pub popularity: Option<f32>,
    pub content_rating: Option<String>,
    #[serde(default)]
    pub content_ratings: Vec<ContentRating>,
    #[serde(default)]
    pub release_dates: Vec<ReleaseDatesByCountry>,
    #[serde(default)]
    pub genres: Vec<GenreInfo>,
    #[serde(default)]
    pub networks: Vec<NetworkInfo>,
    #[serde(default)]
    pub origin_countries: Vec<String>,
    #[serde(default)]
    pub spoken_languages: Vec<SpokenLanguage>,
    #[serde(default)]
    pub production_companies: Vec<ProductionCompany>,
    #[serde(default)]
    pub production_countries: Vec<ProductionCountry>,
    pub homepage: Option<String>,
    pub status: Option<String>,
    pub tagline: Option<String>,
    pub in_production: Option<bool>,

    // Media assets
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub logo_path: Option<String>,
    pub images: MediaImages,

    // Credits
    pub cast: Vec<CastMember>,
    pub crew: Vec<CrewMember>,

    // Additional
    #[serde(default)]
    pub videos: Vec<Video>,
    #[serde(default)]
    pub keywords: Vec<Keyword>,
    pub external_ids: ExternalIds,
    #[serde(default)]
    pub alternative_titles: Vec<AlternativeTitle>,
    #[serde(default)]
    pub translations: Vec<Translation>,
    #[serde(default)]
    pub episode_groups: Vec<EpisodeGroupMembership>,
    #[serde(default)]
    pub recommendations: Vec<RelatedMediaRef>,
    #[serde(default)]
    pub similar: Vec<RelatedMediaRef>,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct SeasonDetails {
    pub id: u64,
    pub season_number: u8,
    pub name: String,
    pub overview: Option<String>,
    pub air_date: Option<String>,
    pub episode_count: u32,
    pub poster_path: Option<String>,
    pub runtime: Option<u32>,
    #[serde(default)]
    pub external_ids: ExternalIds,
    #[serde(default)]
    pub images: MediaImages,
    #[serde(default)]
    pub videos: Vec<Video>,
    #[serde(default)]
    pub keywords: Vec<Keyword>,
    #[serde(default)]
    pub translations: Vec<Translation>,
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
    pub vote_count: Option<u32>,
    #[serde(default)]
    pub production_code: Option<String>,
    #[serde(default)]
    pub external_ids: ExternalIds,
    #[serde(default)]
    pub images: MediaImages,
    #[serde(default)]
    pub videos: Vec<Video>,
    #[serde(default)]
    pub keywords: Vec<Keyword>,
    #[serde(default)]
    pub translations: Vec<Translation>,
    #[serde(default)]
    pub guest_stars: Vec<CastMember>,
    #[serde(default)]
    pub crew: Vec<CrewMember>,
    #[serde(default)]
    pub content_ratings: Vec<ContentRating>,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct CastMember {
    pub id: u64,
    pub credit_id: Option<String>,
    pub cast_id: Option<u64>,
    pub name: String,
    pub original_name: Option<String>,
    pub character: String,
    pub profile_path: Option<String>,
    pub order: u32,
    pub gender: Option<u8>,
    pub known_for_department: Option<String>,
    pub adult: Option<bool>,
    pub popularity: Option<f32>,
    #[serde(default)]
    pub also_known_as: Vec<String>,
    #[serde(default)]
    pub external_ids: PersonExternalIds,
    #[serde(default)]
    pub image_slot: u32,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct CrewMember {
    pub id: u64,
    pub credit_id: Option<String>,
    pub name: String,
    pub job: String,
    pub department: String,
    pub profile_path: Option<String>,
    pub gender: Option<u8>,
    pub known_for_department: Option<String>,
    pub adult: Option<bool>,
    pub popularity: Option<f32>,
    pub original_name: Option<String>,
    #[serde(default)]
    pub also_known_as: Vec<String>,
    #[serde(default)]
    pub external_ids: PersonExternalIds,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq, Hash))]
pub struct Video {
    pub key: String,
    pub name: Option<String>,
    pub site: String,
    pub video_type: Option<String>,
    pub official: Option<bool>,
    pub iso_639_1: Option<String>,
    pub iso_3166_1: Option<String>,
    pub published_at: Option<String>,
    pub size: Option<u32>,
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
    pub wikidata_id: Option<String>,
    pub tiktok_id: Option<String>,
    pub youtube_id: Option<String>,
    pub freebase_id: Option<String>,
    pub freebase_mid: Option<String>,
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
