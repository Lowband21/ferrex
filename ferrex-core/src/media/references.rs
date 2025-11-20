use crate::api_types::MediaId;
use super::{
    MediaDetailsOption, TmdbDetails, MediaFile, 
    MovieID, SeriesID, SeasonID, EpisodeID,
    MovieTitle, SeriesTitle, EpisodeTitle,
    MovieURL, SeriesURL, SeasonURL, EpisodeURL,
    SeasonNumber, EpisodeNumber,
    MediaRef, Playable, Browsable,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;


/// Lightweight movie reference for lists/collections
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MediaReference {
    Movie(MovieReference),
    Series(SeriesReference),
    Season(SeasonReference),
    Episode(EpisodeReference),
}

// ===== MediaReference Helper Methods =====

impl MediaReference {
    /// Get as a trait object for ergonomic method access
    pub fn as_ref(&self) -> &dyn MediaRef {
        match self {
            Self::Movie(m) => m,
            Self::Series(s) => s,
            Self::Season(s) => s,
            Self::Episode(e) => e,
        }
    }
    
    /// Get as a mutable trait object
    pub fn as_mut(&mut self) -> &mut dyn MediaRef {
        match self {
            Self::Movie(m) => m,
            Self::Series(s) => s,
            Self::Season(s) => s,
            Self::Episode(e) => e,
        }
    }
    
    /// Try to get as a specific movie reference
    pub fn as_movie(&self) -> Option<&MovieReference> {
        match self {
            Self::Movie(m) => Some(m),
            _ => None,
        }
    }
    
    /// Try to get as a specific series reference
    pub fn as_series(&self) -> Option<&SeriesReference> {
        match self {
            Self::Series(s) => Some(s),
            _ => None,
        }
    }
    
    /// Try to get as a specific season reference
    pub fn as_season(&self) -> Option<&SeasonReference> {
        match self {
            Self::Season(s) => Some(s),
            _ => None,
        }
    }
    
    /// Try to get as a specific episode reference
    pub fn as_episode(&self) -> Option<&EpisodeReference> {
        match self {
            Self::Episode(e) => Some(e),
            _ => None,
        }
    }
    
    /// Get as a playable item if this media can be played
    pub fn as_playable(&self) -> Option<&dyn Playable> {
        match self {
            Self::Movie(m) => Some(m),
            Self::Episode(e) => Some(e),
            _ => None, // Series and seasons can't be played directly
        }
    }
    
    /// Get as a browsable item if this media contains other media
    pub fn as_browsable(&self) -> Option<&dyn Browsable> {
        match self {
            Self::Series(s) => Some(s),
            Self::Season(s) => Some(s),
            _ => None, // Movies and episodes don't contain other media
        }
    }
    
    /// Helper to get media type
    pub fn media_type(&self) -> &'static str {
        match self {
            Self::Movie(_) => "movie",
            Self::Series(_) => "series",
            Self::Season(_) => "season",
            Self::Episode(_) => "episode",
        }
    }
}

/// Lightweight movie reference for lists/collections
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MovieReference {
    pub id: MovieID,
    pub tmdb_id: u64,
    pub title: MovieTitle,
    pub details: MediaDetailsOption,
    pub endpoint: MovieURL,
    pub file: MediaFile,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_color: Option<String>, // Hex color e.g. "#2C3E50"
}

/// Lightweight series reference for lists/collections
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SeriesReference {
    pub id: SeriesID,
    pub library_id: Uuid,
    pub tmdb_id: u64,
    pub title: SeriesTitle,
    pub details: MediaDetailsOption,
    pub endpoint: SeriesURL,
    /// When the series folder was created (for date added sorting)
    #[serde(default = "chrono::Utc::now")]
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_color: Option<String>, // Hex color e.g. "#2C3E50"
}

/// Lightweight season reference
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SeasonReference {
    pub id: SeasonID,
    pub season_number: SeasonNumber,
    pub series_id: SeriesID, // Link to parent series
    pub library_id: Uuid,     // Direct reference to library (no runtime derivation needed)
    pub tmdb_series_id: u64,
    pub details: MediaDetailsOption,
    pub endpoint: SeasonURL,
    /// When the season folder was created (for date added sorting)
    #[serde(default = "chrono::Utc::now")]
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_color: Option<String>, // Hex color e.g. "#2C3E50"
}

/// Lightweight episode reference
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EpisodeReference {
    pub id: EpisodeID,
    pub episode_number: EpisodeNumber,
    pub season_number: SeasonNumber,
    pub season_id: SeasonID, // Link to parent season
    pub series_id: SeriesID, // Link to parent series
    pub tmdb_series_id: u64,
    pub details: MediaDetailsOption,
    pub endpoint: EpisodeURL,
    pub file: MediaFile,
}

