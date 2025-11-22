use std::collections::HashMap;

#[cfg(feature = "rkyv")]
use rkyv::{
    Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::details::LibraryReference;
use crate::types::ids::LibraryId;
use crate::types::library::LibraryType;
use crate::types::media::{
    EpisodeReference, Media, MovieReference, SeasonReference, SeriesReference,
};
use crate::types::media_id::MediaID;

/// Lightweight payload of library media used by UI clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "rkyv", derive(Archive, RkyvSerialize, RkyvDeserialize))]
pub struct LibraryMediaResponse {
    pub library: LibraryReference,
    pub media: Vec<Media>, // Lightweight - MediaDetailsOption will be Endpoint variant
}

/// In-memory cache shared between services for library media lookups
#[derive(Debug, Clone)]
pub enum LibraryMediaCache {
    Movies {
        references: Vec<MovieReference>,
    },
    TvShows {
        series_references: HashMap<Uuid, SeriesReference>,
        series_references_sorted: Vec<SeriesReference>,
        series_indices_sorted: Vec<String>,
        season_references: HashMap<Uuid, Vec<SeasonReference>>,
        episode_references: HashMap<Uuid, Vec<EpisodeReference>>,
    },
}

impl LibraryMediaCache {
    pub fn is_empty(&self) -> bool {
        match self {
            LibraryMediaCache::Movies { references } => references.is_empty(),
            LibraryMediaCache::TvShows {
                series_references, ..
            } => series_references.is_empty(),
        }
    }

    pub fn new(library_type: LibraryType) -> Self {
        match library_type {
            LibraryType::Movies => LibraryMediaCache::Movies {
                references: Vec::new(),
            },
            LibraryType::Series => LibraryMediaCache::TvShows {
                series_references: HashMap::new(),
                series_references_sorted: Vec::new(),
                series_indices_sorted: Vec::new(),
                season_references: HashMap::new(),
                episode_references: HashMap::new(),
            },
        }
    }
}

/// Fetch a particular media item from a library
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchMediaRequest {
    pub library_id: LibraryId,
    pub media_id: MediaID,
}

/// Fetch multiple media items from a library
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchMediaRequest {
    pub library_id: LibraryId,
    pub media_ids: Vec<MediaID>,
}

/// Response for batch media fetch operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchMediaResponse {
    pub items: Vec<Media>,
    pub errors: Vec<(MediaID, String)>,
}

/// Request payload for manual metadata matching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualMatchRequest {
    pub library_id: LibraryId,
    pub media_id: MediaID,
    pub tmdb_id: u64,
}

/// Library creation request payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateLibraryRequest {
    pub name: String,
    pub library_type: LibraryType,
    pub paths: Vec<String>,
    #[serde(default = "default_scan_interval")]
    pub scan_interval_minutes: u32,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_start_scan")]
    pub start_scan: bool,
}

/// Library update request payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateLibraryRequest {
    pub name: Option<String>,
    pub paths: Option<Vec<String>>,
    pub scan_interval_minutes: Option<u32>,
    pub enabled: Option<bool>,
}

fn default_scan_interval() -> u32 {
    60
}

fn default_enabled() -> bool {
    true
}

fn default_start_scan() -> bool {
    true
}
