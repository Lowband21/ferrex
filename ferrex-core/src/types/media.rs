use crate::{
    EpisodeID, EpisodeNumber, EpisodeURL, LibraryID, MediaDetailsOption, MediaFile, MovieID,
    MovieTitle, MovieURL, SeasonID, SeasonNumber, SeasonURL, SeriesID, SeriesTitle, SeriesURL,
};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

/// Lightweight movie reference for lists/collections
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub enum Media {
    /// Movie media reference
    Movie(MovieReference),
    /// Series media reference
    Series(SeriesReference),
    /// Season media reference
    Season(SeasonReference),
    /// Episode media reference
    Episode(EpisodeReference),
}

/// Lightweight movie reference for lists/collections
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct MovieReference {
    pub id: MovieID,
    pub library_id: LibraryID,
    pub tmdb_id: u64,
    pub title: MovieTitle,
    pub details: MediaDetailsOption,
    pub endpoint: MovieURL,
    pub file: MediaFile,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_color: Option<String>, // Hex color e.g. "#2C3E50"
}

/// Lightweight series reference for lists/collections
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct SeriesReference {
    pub id: SeriesID,
    pub library_id: LibraryID,
    pub tmdb_id: u64,
    pub title: SeriesTitle,
    pub details: MediaDetailsOption,
    pub endpoint: SeriesURL,
    /// When the series was discovered (row creation time)
    #[serde(default = "chrono::Utc::now")]
    #[rkyv(with = crate::rkyv_wrappers::DateTimeWrapper)]
    pub discovered_at: chrono::DateTime<chrono::Utc>,
    /// When the series folder was created (for date added sorting)
    #[serde(default = "chrono::Utc::now")]
    #[rkyv(with = crate::rkyv_wrappers::DateTimeWrapper)]
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_color: Option<String>, // Hex color e.g. "#2C3E50"
}

/// Lightweight season reference
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct SeasonReference {
    pub id: SeasonID,
    pub library_id: LibraryID,
    pub season_number: SeasonNumber,
    pub series_id: SeriesID, // Link to parent series
    pub tmdb_series_id: u64,
    pub details: MediaDetailsOption,
    pub endpoint: SeasonURL,
    /// When the season was discovered (row creation time)
    #[serde(default = "chrono::Utc::now")]
    #[rkyv(with = crate::rkyv_wrappers::DateTimeWrapper)]
    pub discovered_at: chrono::DateTime<chrono::Utc>,
    /// When the season folder was created (for date added sorting)
    #[serde(default = "chrono::Utc::now")]
    #[rkyv(with = crate::rkyv_wrappers::DateTimeWrapper)]
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_color: Option<String>, // Hex color e.g. "#2C3E50"
}

/// Lightweight episode reference
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct EpisodeReference {
    pub id: EpisodeID,
    pub library_id: LibraryID,
    pub episode_number: EpisodeNumber,
    pub season_number: SeasonNumber,
    pub season_id: SeasonID, // Link to parent season
    pub series_id: SeriesID, // Link to parent series
    pub tmdb_series_id: u64,
    pub details: MediaDetailsOption,
    pub endpoint: EpisodeURL,
    pub file: MediaFile,
    /// When the episode was discovered (row creation time)
    #[serde(default = "chrono::Utc::now")]
    #[rkyv(with = crate::rkyv_wrappers::DateTimeWrapper)]
    pub discovered_at: chrono::DateTime<chrono::Utc>,
    /// When the episode was created (for alternate date-based sorting)
    #[serde(default = "chrono::Utc::now")]
    #[rkyv(with = crate::rkyv_wrappers::DateTimeWrapper)]
    pub created_at: chrono::DateTime<chrono::Utc>,
}
