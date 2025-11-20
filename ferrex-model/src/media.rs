use super::{
    details::{MediaDetailsOption, TmdbDetails},
    files::MediaFile,
    ids::{EpisodeID, LibraryID, MovieID, SeasonID, SeriesID},
    numbers::{EpisodeNumber, SeasonNumber},
    titles::{MovieTitle, SeriesTitle},
    urls::{EpisodeURL, MovieURL, SeasonURL, SeriesURL},
};
use std::fmt;

use crate::chrono::{DateTime, Utc};
#[cfg(feature = "rkyv")]
use crate::media_id::ArchivedMediaID;
/// Lightweight movie reference for lists/collections
#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
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
#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct MovieReference {
    pub id: MovieID,
    pub library_id: LibraryID,
    pub tmdb_id: u64,
    pub title: MovieTitle,
    pub details: MediaDetailsOption,
    pub endpoint: MovieURL,
    pub file: MediaFile,
    #[cfg_attr(
        feature = "serde",
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub theme_color: Option<String>, // Hex color e.g. "#2C3E50"
}

/// Lightweight series reference for lists/collections
#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct SeriesReference {
    pub id: SeriesID,
    pub library_id: LibraryID,
    pub tmdb_id: u64,
    pub title: SeriesTitle,
    pub details: MediaDetailsOption,
    pub endpoint: SeriesURL,
    /// When the series was discovered (row creation time)
    #[cfg_attr(feature = "serde", serde(default = "Utc::now"))]
    #[cfg_attr(feature = "rkyv", rkyv(with = crate::rkyv_wrappers::DateTimeWrapper))]
    pub discovered_at: DateTime<Utc>,
    /// When the series folder was created (for date added sorting)
    #[cfg_attr(feature = "serde", serde(default = "Utc::now"))]
    #[cfg_attr(feature = "rkyv", rkyv(with = crate::rkyv_wrappers::DateTimeWrapper))]
    pub created_at: DateTime<Utc>,
    #[cfg_attr(
        feature = "serde",
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub theme_color: Option<String>, // Hex color e.g. "#2C3E50"
}

/// Lightweight season reference
#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct SeasonReference {
    pub id: SeasonID,
    pub library_id: LibraryID,
    pub season_number: SeasonNumber,
    pub series_id: SeriesID, // Link to parent series
    pub tmdb_series_id: u64,
    pub details: MediaDetailsOption,
    pub endpoint: SeasonURL,
    /// When the season was discovered (row creation time)
    #[cfg_attr(feature = "serde", serde(default = "Utc::now"))]
    #[cfg_attr(feature = "rkyv", rkyv(with = crate::rkyv_wrappers::DateTimeWrapper))]
    pub discovered_at: DateTime<Utc>,
    /// When the season folder was created (for date added sorting)
    #[cfg_attr(feature = "serde", serde(default = "Utc::now"))]
    #[cfg_attr(feature = "rkyv", rkyv(with = crate::rkyv_wrappers::DateTimeWrapper))]
    pub created_at: DateTime<Utc>,
    #[cfg_attr(
        feature = "serde",
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub theme_color: Option<String>, // Hex color e.g. "#2C3E50"
}

/// Lightweight episode reference
#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
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
    #[cfg_attr(feature = "serde", serde(default = "Utc::now"))]
    #[cfg_attr(feature = "rkyv", rkyv(with = crate::rkyv_wrappers::DateTimeWrapper))]
    pub discovered_at: DateTime<Utc>,
    /// When the episode was created (for alternate date-based sorting)
    #[cfg_attr(feature = "serde", serde(default = "Utc::now"))]
    #[cfg_attr(feature = "rkyv", rkyv(with = crate::rkyv_wrappers::DateTimeWrapper))]
    pub created_at: DateTime<Utc>,
}

impl fmt::Debug for Media {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Media::Movie(movie) => {
                f.debug_tuple("Media::Movie").field(movie).finish()
            }
            Media::Series(series) => {
                f.debug_tuple("Media::Series").field(series).finish()
            }
            Media::Season(season) => {
                f.debug_tuple("Media::Season").field(season).finish()
            }
            Media::Episode(episode) => {
                f.debug_tuple("Media::Episode").field(episode).finish()
            }
        }
    }
}

impl fmt::Debug for MovieReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MovieReference")
            .field("id", &self.id)
            .field("library_id", &self.library_id)
            .field("tmdb_id", &self.tmdb_id)
            .field("title", &self.title)
            .field("endpoint", &self.endpoint)
            .field("theme_color", &self.theme_color)
            .field("details", &self.details)
            .field("file", &self.file)
            .finish()
    }
}

impl fmt::Debug for SeriesReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SeriesReference")
            .field("id", &self.id)
            .field("library_id", &self.library_id)
            .field("tmdb_id", &self.tmdb_id)
            .field("title", &self.title)
            .field("discovered_at", &self.discovered_at)
            .field("created_at", &self.created_at)
            .field("endpoint", &self.endpoint)
            .field("theme_color", &self.theme_color)
            .field("details", &self.details)
            .finish()
    }
}

impl fmt::Debug for SeasonReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SeasonReference")
            .field("id", &self.id)
            .field("library_id", &self.library_id)
            .field("season_number", &self.season_number)
            .field("series_id", &self.series_id)
            .field("tmdb_series_id", &self.tmdb_series_id)
            .field("discovered_at", &self.discovered_at)
            .field("created_at", &self.created_at)
            .field("endpoint", &self.endpoint)
            .field("details", &self.details)
            .finish()
    }
}

impl fmt::Debug for EpisodeReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EpisodeReference")
            .field("id", &self.id)
            .field("library_id", &self.library_id)
            .field("season_number", &self.season_number)
            .field("episode_number", &self.episode_number)
            .field("season_id", &self.season_id)
            .field("series_id", &self.series_id)
            .field("tmdb_series_id", &self.tmdb_series_id)
            .field("discovered_at", &self.discovered_at)
            .field("created_at", &self.created_at)
            .field("endpoint", &self.endpoint)
            .field("details", &self.details)
            .field("file", &self.file)
            .finish()
    }
}

impl MovieReference {
    /// Returns the rating when TMDB details are cached locally.
    pub fn rating(&self) -> Option<f32> {
        match &self.details {
            MediaDetailsOption::Details(details) => match details.as_ref() {
                TmdbDetails::Movie(movie) => movie.vote_average,
                TmdbDetails::Series(series) => series.vote_average,
                TmdbDetails::Episode(episode) => episode.vote_average,
                TmdbDetails::Season(_) => None,
            },
            _ => None,
        }
    }

    /// Returns the list of genre names when TMDB details are cached locally.
    pub fn genres(&self) -> Option<Vec<&str>> {
        match &self.details {
            MediaDetailsOption::Details(details) => match details.as_ref() {
                TmdbDetails::Movie(movie) => Some(
                    movie
                        .genres
                        .iter()
                        .map(|genre| genre.name.as_str())
                        .collect(),
                ),
                TmdbDetails::Series(series) => Some(
                    series
                        .genres
                        .iter()
                        .map(|genre| genre.name.as_str())
                        .collect(),
                ),
                _ => None,
            },
            _ => None,
        }
    }
}

impl SeriesReference {
    /// Returns the first available year across the known TMDB detail variants.
    pub fn year(&self) -> Option<u16> {
        match &self.details {
            MediaDetailsOption::Details(details) => match details.as_ref() {
                TmdbDetails::Movie(movie) => movie
                    .release_date
                    .as_ref()
                    .and_then(|date| date.split('-').next())
                    .and_then(|year| year.parse().ok()),
                TmdbDetails::Series(series) => series
                    .first_air_date
                    .as_ref()
                    .and_then(|date| date.split('-').next())
                    .and_then(|year| year.parse().ok()),
                TmdbDetails::Season(season) => season
                    .air_date
                    .as_ref()
                    .and_then(|date| date.split('-').next())
                    .and_then(|year| year.parse().ok()),
                TmdbDetails::Episode(episode) => episode
                    .air_date
                    .as_ref()
                    .and_then(|date| date.split('-').next())
                    .and_then(|year| year.parse().ok()),
            },
            _ => None,
        }
    }

    /// Returns the rating when TMDB details are cached locally.
    pub fn rating(&self) -> Option<f32> {
        match &self.details {
            MediaDetailsOption::Details(details) => match details.as_ref() {
                TmdbDetails::Movie(movie) => movie.vote_average,
                TmdbDetails::Series(series) => series.vote_average,
                TmdbDetails::Episode(episode) => episode.vote_average,
                TmdbDetails::Season(_) => None,
            },
            _ => None,
        }
    }

    /// Returns the list of genre names when TMDB details are cached locally.
    pub fn genres(&self) -> Option<Vec<&str>> {
        match &self.details {
            MediaDetailsOption::Details(details) => match details.as_ref() {
                TmdbDetails::Movie(movie) => Some(
                    movie
                        .genres
                        .iter()
                        .map(|genre| genre.name.as_str())
                        .collect(),
                ),
                TmdbDetails::Series(series) => Some(
                    series
                        .genres
                        .iter()
                        .map(|genre| genre.name.as_str())
                        .collect(),
                ),
                _ => None,
            },
            _ => None,
        }
    }
}

impl SeasonReference {
    /// Returns the first available year across the known TMDB detail variants.
    pub fn year(&self) -> Option<u16> {
        match &self.details {
            MediaDetailsOption::Details(details) => match details.as_ref() {
                TmdbDetails::Movie(movie) => movie
                    .release_date
                    .as_ref()
                    .and_then(|date| date.split('-').next())
                    .and_then(|year| year.parse().ok()),
                TmdbDetails::Series(series) => series
                    .first_air_date
                    .as_ref()
                    .and_then(|date| date.split('-').next())
                    .and_then(|year| year.parse().ok()),
                TmdbDetails::Season(season) => season
                    .air_date
                    .as_ref()
                    .and_then(|date| date.split('-').next())
                    .and_then(|year| year.parse().ok()),
                TmdbDetails::Episode(episode) => episode
                    .air_date
                    .as_ref()
                    .and_then(|date| date.split('-').next())
                    .and_then(|year| year.parse().ok()),
            },
            _ => None,
        }
    }

    /// Returns the rating when TMDB details are cached locally.
    pub fn rating(&self) -> Option<f32> {
        match &self.details {
            MediaDetailsOption::Details(details) => match details.as_ref() {
                TmdbDetails::Movie(movie) => movie.vote_average,
                TmdbDetails::Series(series) => series.vote_average,
                TmdbDetails::Episode(episode) => episode.vote_average,
                TmdbDetails::Season(_) => None,
            },
            _ => None,
        }
    }

    /// Returns the list of genre names when TMDB details are cached locally.
    pub fn genres(&self) -> Option<Vec<&str>> {
        match &self.details {
            MediaDetailsOption::Details(details) => match details.as_ref() {
                TmdbDetails::Movie(movie) => Some(
                    movie
                        .genres
                        .iter()
                        .map(|genre| genre.name.as_str())
                        .collect(),
                ),
                TmdbDetails::Series(series) => Some(
                    series
                        .genres
                        .iter()
                        .map(|genre| genre.name.as_str())
                        .collect(),
                ),
                _ => None,
            },
            _ => None,
        }
    }
}

impl EpisodeReference {
    /// Returns the first available year across the known TMDB detail variants.
    pub fn year(&self) -> Option<u16> {
        match &self.details {
            MediaDetailsOption::Details(details) => match details.as_ref() {
                TmdbDetails::Movie(movie) => movie
                    .release_date
                    .as_ref()
                    .and_then(|date| date.split('-').next())
                    .and_then(|year| year.parse().ok()),
                TmdbDetails::Series(series) => series
                    .first_air_date
                    .as_ref()
                    .and_then(|date| date.split('-').next())
                    .and_then(|year| year.parse().ok()),
                TmdbDetails::Season(season) => season
                    .air_date
                    .as_ref()
                    .and_then(|date| date.split('-').next())
                    .and_then(|year| year.parse().ok()),
                TmdbDetails::Episode(episode) => episode
                    .air_date
                    .as_ref()
                    .and_then(|date| date.split('-').next())
                    .and_then(|year| year.parse().ok()),
            },
            _ => None,
        }
    }

    /// Returns the rating when TMDB details are cached locally.
    pub fn rating(&self) -> Option<f32> {
        match &self.details {
            MediaDetailsOption::Details(details) => match details.as_ref() {
                TmdbDetails::Movie(movie) => movie.vote_average,
                TmdbDetails::Series(series) => series.vote_average,
                TmdbDetails::Episode(episode) => episode.vote_average,
                TmdbDetails::Season(_) => None,
            },
            _ => None,
        }
    }

    /// Returns the list of genre names when TMDB details are cached locally.
    pub fn genres(&self) -> Option<Vec<&str>> {
        match &self.details {
            MediaDetailsOption::Details(details) => match details.as_ref() {
                TmdbDetails::Movie(movie) => Some(
                    movie
                        .genres
                        .iter()
                        .map(|genre| genre.name.as_str())
                        .collect(),
                ),
                TmdbDetails::Series(series) => Some(
                    series
                        .genres
                        .iter()
                        .map(|genre| genre.name.as_str())
                        .collect(),
                ),
                _ => None,
            },
            _ => None,
        }
    }
}

#[cfg(feature = "rkyv")]
impl ArchivedMedia {
    pub fn archived_media_id(&self) -> ArchivedMediaID {
        match self {
            Self::Movie(movie) => ArchivedMediaID::Movie(movie.id),
            Self::Series(series) => ArchivedMediaID::Series(series.id),
            Self::Season(season) => ArchivedMediaID::Season(season.id),
            Self::Episode(episode) => ArchivedMediaID::Episode(episode.id),
        }
    }
}
