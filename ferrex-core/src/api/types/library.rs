use std::collections::HashMap;

#[cfg(feature = "rkyv")]
use rkyv::{
    Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::details::LibraryReference;
use crate::types::ids::LibraryId;
use crate::types::ids::{MovieBatchId, SeriesID};
use crate::types::library::LibraryType;
use crate::types::media::{
    EpisodeReference, Media, MovieReference, SeasonReference, Series,
};
use crate::types::media_id::MediaID;

/// Lightweight payload of library media used by UI clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "rkyv", derive(Archive, RkyvSerialize, RkyvDeserialize))]
pub struct LibraryMediaResponse {
    pub library: LibraryReference,
    pub media: Vec<Media>, // Lightweight - MediaDetailsOption will be Endpoint variant
}

/// Archived movie reference batch payload fetched by (library_id, batch_id).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "rkyv", derive(Archive, RkyvSerialize, RkyvDeserialize))]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq)))]
pub struct MovieReferenceBatchResponse {
    pub library_id: LibraryId,
    pub batch_id: MovieBatchId,
    pub movies: Vec<MovieReference>,
}

/// Bundled movie batches payload for bootstrapping movie libraries.
///
/// This is used by the player at startup to fetch all finalized movie batches
/// for a library in a single request, avoiding N HTTP round-trips.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "rkyv", derive(Archive, RkyvSerialize, RkyvDeserialize))]
pub struct MovieReferenceBatchBlob {
    pub batch_id: MovieBatchId,
    pub bytes: Vec<u8>,
}

/// Response containing all finalized movie reference batches for a library.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "rkyv", derive(Archive, RkyvSerialize, RkyvDeserialize))]
pub struct MovieReferenceBatchBundleResponse {
    pub library_id: LibraryId,
    pub batches: Vec<MovieReferenceBatchBlob>,
}

/// Archived per-series payload containing the series and all dependent
/// season/episode references.
///
/// This is intended to be stored and accessed as rkyv bytes by the player so it
/// can look up series, seasons, and episodes without fully deserializing them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "rkyv", derive(Archive, RkyvSerialize, RkyvDeserialize))]
pub struct SeriesBundleResponse {
    pub library_id: LibraryId,
    pub series_id: SeriesID,
    pub series: Series,
    pub seasons: Vec<SeasonReference>,
    pub episodes: Vec<EpisodeReference>,
}

/// Bundled per-series archives payload for bootstrapping series libraries.
///
/// Each `bytes` blob is an rkyv-serialized `SeriesBundleResponse`, allowing the
/// player to keep per-series payloads isolated (and replaceable) without
/// re-downloading the entire library snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
#[cfg_attr(feature = "rkyv", derive(Archive, RkyvSerialize, RkyvDeserialize))]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, Hash, PartialEq, Eq)))]
pub struct SeriesBundleBlob {
    pub series_id: SeriesID,
    pub bytes: Vec<u8>,
}

/// Response containing all series bundles for a library.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "rkyv", derive(Archive, RkyvSerialize, RkyvDeserialize))]
pub struct SeriesBundleBundleResponse {
    pub library_id: LibraryId,
    pub bundles: Vec<(SeriesBundleBlob, u64)>,
}

/// In-memory cache shared between services for library media lookups
#[derive(Debug, Clone)]
pub enum LibraryMediaCache {
    Movies {
        references: Vec<MovieReference>,
    },
    TvShows {
        series_references: HashMap<Uuid, Series>,
        series_references_sorted: Vec<Series>,
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
    #[serde(default = "default_movie_ref_batch_size")]
    pub movie_ref_batch_size: u32,
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
    pub movie_ref_batch_size: Option<u32>,
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

fn default_movie_ref_batch_size() -> u32 {
    250
}
