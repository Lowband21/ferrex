use std::fmt;
use std::path::PathBuf;
use uuid::Uuid;

use super::{ids::LibraryID, image::MediaImages, library::LibraryType};

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct ReleaseDateEntry {
    pub certification: Option<String>,
    pub release_date: Option<String>,
    pub release_type: Option<i32>,
    pub note: Option<String>,
    pub iso_639_1: Option<String>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub descriptors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct ReleaseDatesByCountry {
    pub iso_3166_1: String,
    #[cfg_attr(feature = "serde", serde(default))]
    pub release_dates: Vec<ReleaseDateEntry>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct ContentRating {
    pub iso_3166_1: String,
    pub rating: Option<String>,
    pub rating_system: Option<String>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub descriptors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct AlternativeTitle {
    pub title: String,
    pub iso_3166_1: Option<String>,
    pub title_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
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

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct CollectionInfo {
    pub id: u64,
    pub name: String,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq, Hash)))]
pub struct Keyword {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct EpisodeGroupMembership {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub group_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct GenreInfo {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct ProductionCompany {
    pub id: u64,
    pub name: String,
    pub origin_country: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct ProductionCountry {
    pub iso_3166_1: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct SpokenLanguage {
    pub iso_639_1: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct NetworkInfo {
    pub id: u64,
    pub name: String,
    pub origin_country: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct RelatedMediaRef {
    pub tmdb_id: u64,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq, Hash)))]
pub struct PersonExternalIds {
    pub imdb_id: Option<String>,
    pub facebook_id: Option<String>,
    pub instagram_id: Option<String>,
    pub twitter_id: Option<String>,
    pub wikidata_id: Option<String>,
    pub tiktok_id: Option<String>,
    pub youtube_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub enum TmdbDetails {
    Movie(EnhancedMovieDetails),
    Series(EnhancedSeriesDetails),
    Season(SeasonDetails),
    Episode(EpisodeDetails),
}

/// Enhanced metadata that includes images, credits, and additional information
#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
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
    #[cfg_attr(feature = "serde", serde(default))]
    pub content_ratings: Vec<ContentRating>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub release_dates: Vec<ReleaseDatesByCountry>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub genres: Vec<GenreInfo>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub spoken_languages: Vec<SpokenLanguage>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub production_companies: Vec<ProductionCompany>,
    #[cfg_attr(feature = "serde", serde(default))]
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
    #[cfg_attr(feature = "serde", serde(default))]
    pub videos: Vec<Video>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub keywords: Vec<Keyword>,
    pub external_ids: ExternalIds,
    #[cfg_attr(feature = "serde", serde(default))]
    pub alternative_titles: Vec<AlternativeTitle>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub translations: Vec<Translation>,
    pub collection: Option<CollectionInfo>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub recommendations: Vec<RelatedMediaRef>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub similar: Vec<RelatedMediaRef>,
}

#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
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
    #[cfg_attr(feature = "serde", serde(default))]
    pub content_ratings: Vec<ContentRating>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub release_dates: Vec<ReleaseDatesByCountry>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub genres: Vec<GenreInfo>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub networks: Vec<NetworkInfo>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub origin_countries: Vec<String>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub spoken_languages: Vec<SpokenLanguage>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub production_companies: Vec<ProductionCompany>,
    #[cfg_attr(feature = "serde", serde(default))]
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
    #[cfg_attr(feature = "serde", serde(default))]
    pub videos: Vec<Video>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub keywords: Vec<Keyword>,
    pub external_ids: ExternalIds,
    #[cfg_attr(feature = "serde", serde(default))]
    pub alternative_titles: Vec<AlternativeTitle>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub translations: Vec<Translation>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub episode_groups: Vec<EpisodeGroupMembership>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub recommendations: Vec<RelatedMediaRef>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub similar: Vec<RelatedMediaRef>,
}

#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct SeasonDetails {
    pub id: u64,
    pub season_number: u8,
    pub name: String,
    pub overview: Option<String>,
    pub air_date: Option<String>,
    pub episode_count: u32,
    pub poster_path: Option<String>,
    pub runtime: Option<u32>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub external_ids: ExternalIds,
    #[cfg_attr(feature = "serde", serde(default))]
    pub images: MediaImages,
    #[cfg_attr(feature = "serde", serde(default))]
    pub videos: Vec<Video>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub keywords: Vec<Keyword>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub translations: Vec<Translation>,
}

#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
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
    #[cfg_attr(feature = "serde", serde(default))]
    pub production_code: Option<String>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub external_ids: ExternalIds,
    #[cfg_attr(feature = "serde", serde(default))]
    pub images: MediaImages,
    #[cfg_attr(feature = "serde", serde(default))]
    pub videos: Vec<Video>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub keywords: Vec<Keyword>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub translations: Vec<Translation>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub guest_stars: Vec<CastMember>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub crew: Vec<CrewMember>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub content_ratings: Vec<ContentRating>,
}

impl fmt::Debug for EnhancedMovieDetails {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnhancedMovieDetails")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("release_date", &self.release_date)
            .field("runtime", &self.runtime)
            .field("vote_average", &self.vote_average)
            .field("vote_count", &self.vote_count)
            .field("popularity", &self.popularity)
            .field("content_rating", &self.content_rating)
            .field("collection", &self.collection.as_ref().map(|c| &c.name))
            .field("genre_count", &self.genres.len())
            .field("spoken_language_count", &self.spoken_languages.len())
            .field("production_company_count", &self.production_companies.len())
            .field("production_country_count", &self.production_countries.len())
            .field("cast_count", &self.cast.len())
            .field("crew_count", &self.crew.len())
            .field("video_count", &self.videos.len())
            .field("keyword_count", &self.keywords.len())
            .field("alternative_title_count", &self.alternative_titles.len())
            .field("translation_count", &self.translations.len())
            .field("recommendation_count", &self.recommendations.len())
            .field("similar_count", &self.similar.len())
            .field("images", &self.images)
            .finish()
    }
}

impl fmt::Debug for EnhancedSeriesDetails {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnhancedSeriesDetails")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("first_air_date", &self.first_air_date)
            .field("last_air_date", &self.last_air_date)
            .field("season_count", &self.number_of_seasons)
            .field("episode_count", &self.number_of_episodes)
            .field("vote_average", &self.vote_average)
            .field("vote_count", &self.vote_count)
            .field("popularity", &self.popularity)
            .field("content_rating", &self.content_rating)
            .field("genre_count", &self.genres.len())
            .field("network_count", &self.networks.len())
            .field("origin_country_count", &self.origin_countries.len())
            .field("spoken_language_count", &self.spoken_languages.len())
            .field("production_company_count", &self.production_companies.len())
            .field("production_country_count", &self.production_countries.len())
            .field("cast_count", &self.cast.len())
            .field("crew_count", &self.crew.len())
            .field("video_count", &self.videos.len())
            .field("keyword_count", &self.keywords.len())
            .field("alternative_title_count", &self.alternative_titles.len())
            .field("translation_count", &self.translations.len())
            .field("episode_group_count", &self.episode_groups.len())
            .field("recommendation_count", &self.recommendations.len())
            .field("similar_count", &self.similar.len())
            .field("images", &self.images)
            .finish()
    }
}

impl fmt::Debug for SeasonDetails {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let has_external_ids = self.external_ids != ExternalIds::default();
        f.debug_struct("SeasonDetails")
            .field("id", &self.id)
            .field("season_number", &self.season_number)
            .field("episode_count", &self.episode_count)
            .field("runtime", &self.runtime)
            .field("air_date", &self.air_date)
            .field("poster_path", &self.poster_path)
            .field("has_external_ids", &has_external_ids)
            .field("images", &self.images)
            .field("video_count", &self.videos.len())
            .field("keyword_count", &self.keywords.len())
            .field("translation_count", &self.translations.len())
            .finish()
    }
}

impl fmt::Debug for EpisodeDetails {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let has_external_ids = self.external_ids != ExternalIds::default();
        f.debug_struct("EpisodeDetails")
            .field("id", &self.id)
            .field("season_number", &self.season_number)
            .field("episode_number", &self.episode_number)
            .field("air_date", &self.air_date)
            .field("runtime", &self.runtime)
            .field("vote_average", &self.vote_average)
            .field("vote_count", &self.vote_count)
            .field("still_path", &self.still_path)
            .field("has_external_ids", &has_external_ids)
            .field("guest_star_count", &self.guest_stars.len())
            .field("crew_count", &self.crew.len())
            .field("keyword_count", &self.keywords.len())
            .field("translation_count", &self.translations.len())
            .field("content_rating_count", &self.content_ratings.len())
            .field("images", &self.images)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
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
    #[cfg_attr(feature = "serde", serde(default))]
    pub also_known_as: Vec<String>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub external_ids: PersonExternalIds,
    #[cfg_attr(feature = "serde", serde(default))]
    pub image_slot: u32,
    #[cfg_attr(feature = "serde", serde(default))]
    pub profile_media_id: Option<Uuid>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub profile_image_index: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
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
    #[cfg_attr(feature = "serde", serde(default))]
    pub also_known_as: Vec<String>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub external_ids: PersonExternalIds,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq, Hash)))]
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

#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq, Hash)))]
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
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq, Hash)))]
pub struct LibraryReference {
    pub id: LibraryID,
    pub name: String,
    pub library_type: LibraryType,
    #[cfg_attr(feature = "rkyv", rkyv(with = crate::rkyv_wrappers::VecPathBuf))]
    pub paths: Vec<PathBuf>,
}

pub trait MediaDetailsOptionLike {
    fn get_release_year(&self) -> Option<u16>;
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub enum MediaDetailsOption {
    Endpoint(String),
    Details(Box<TmdbDetails>),
}

impl MediaDetailsOption {
    /// Returns the underlying TMDB details if they are cached locally.
    pub fn as_tmdb(&self) -> Option<&TmdbDetails> {
        match self {
            MediaDetailsOption::Details(details) => Some(details.as_ref()),
            MediaDetailsOption::Endpoint(_) => None,
        }
    }

    /// Returns the enhanced movie details when present.
    pub fn as_movie(&self) -> Option<&EnhancedMovieDetails> {
        match self.as_tmdb() {
            Some(TmdbDetails::Movie(movie)) => Some(movie),
            _ => None,
        }
    }

    /// Returns the enhanced series details when present.
    pub fn as_series(&self) -> Option<&EnhancedSeriesDetails> {
        match self.as_tmdb() {
            Some(TmdbDetails::Series(series)) => Some(series),
            _ => None,
        }
    }

    /// Returns the season details when present.
    pub fn as_season(&self) -> Option<&SeasonDetails> {
        match self.as_tmdb() {
            Some(TmdbDetails::Season(season)) => Some(season),
            _ => None,
        }
    }

    /// Returns the episode details when present.
    pub fn as_episode(&self) -> Option<&EpisodeDetails> {
        match self.as_tmdb() {
            Some(TmdbDetails::Episode(episode)) => Some(episode),
            _ => None,
        }
    }
}

impl MediaDetailsOptionLike for MediaDetailsOption {
    fn get_release_year(&self) -> Option<u16> {
        match self {
            MediaDetailsOption::Endpoint(_) => None,
            MediaDetailsOption::Details(details) => match details.as_ref() {
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

#[cfg(feature = "rkyv")]
impl MediaDetailsOptionLike for ArchivedMediaDetailsOption {
    fn get_release_year(&self) -> Option<u16> {
        match self {
            ArchivedMediaDetailsOption::Endpoint(_) => None,
            ArchivedMediaDetailsOption::Details(details) => {
                match details.as_ref() {
                    ArchivedTmdbDetails::Movie(movie) => movie
                        .release_date
                        .as_ref()
                        .and_then(|date| date.split("-").next())
                        .and_then(|year| year.parse().ok()),
                    ArchivedTmdbDetails::Series(series) => series
                        .first_air_date
                        .as_ref()
                        .and_then(|date| date.split("-").next())
                        .and_then(|year| year.parse().ok()),
                    _ => None,
                }
            }
        }
    }
}

#[cfg(feature = "rkyv")]
impl ArchivedMediaDetailsOption {
    /// Returns the archived TMDB details if available.
    pub fn as_tmdb(&self) -> Option<&ArchivedTmdbDetails> {
        match self {
            ArchivedMediaDetailsOption::Details(details) => {
                Some(details.as_ref())
            }
            ArchivedMediaDetailsOption::Endpoint(_) => None,
        }
    }

    /// Returns archived movie details when present.
    pub fn as_movie(&self) -> Option<&ArchivedEnhancedMovieDetails> {
        match self.as_tmdb() {
            Some(ArchivedTmdbDetails::Movie(movie)) => Some(movie),
            _ => None,
        }
    }

    /// Returns archived series details when present.
    pub fn as_series(&self) -> Option<&ArchivedEnhancedSeriesDetails> {
        match self.as_tmdb() {
            Some(ArchivedTmdbDetails::Series(series)) => Some(series),
            _ => None,
        }
    }

    /// Returns archived season details when present.
    pub fn as_season(&self) -> Option<&ArchivedSeasonDetails> {
        match self.as_tmdb() {
            Some(ArchivedTmdbDetails::Season(season)) => Some(season),
            _ => None,
        }
    }

    /// Returns archived episode details when present.
    pub fn as_episode(&self) -> Option<&ArchivedEpisodeDetails> {
        match self.as_tmdb() {
            Some(ArchivedTmdbDetails::Episode(episode)) => Some(episode),
            _ => None,
        }
    }
}
