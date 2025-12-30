use serde::{Deserialize, Serialize};

use crate::types::ids::{LibraryId, MovieBatchId, SeriesID};

/// A cached per-batch version entry sent by the player.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovieBatchVersionManifestEntry {
    pub batch_id: MovieBatchId,
    pub version: u64,
    /// Optional stable content hash (derived from the rkyv payload) that allows
    /// clients to detect "version-only" bumps without re-downloading bytes.
    ///
    /// When present in sync responses, the player may compare this hash against
    /// its locally cached payload to decide whether a fetch is actually needed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<u64>,
}

/// A cached per-series bundle version entry sent by the player.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesBundleVersionManifestEntry {
    pub series_id: SeriesID,
    pub version: u64,
}

/// Request to compare the player's cached movie batch versions against the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovieBatchSyncRequest {
    #[serde(default)]
    pub batches: Vec<MovieBatchVersionManifestEntry>,
}

/// Response describing which movie batches the player should fetch (new or changed),
/// and which cached batch IDs no longer exist on the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovieBatchSyncResponse {
    pub library_id: LibraryId,
    #[serde(default)]
    pub updates: Vec<MovieBatchVersionManifestEntry>,
    #[serde(default)]
    pub removals: Vec<MovieBatchId>,
}

/// Request to compare the player's cached series bundle versions against the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesBundleSyncRequest {
    #[serde(default)]
    pub bundles: Vec<SeriesBundleVersionManifestEntry>,
}

/// Response describing which series bundles the player should fetch (new or changed),
/// and which cached series IDs no longer exist on the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesBundleSyncResponse {
    pub library_id: LibraryId,
    #[serde(default)]
    pub updates: Vec<SeriesBundleVersionManifestEntry>,
    #[serde(default)]
    pub removals: Vec<SeriesID>,
}

/// Request to fetch a subset of movie batches for a library.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovieBatchFetchRequest {
    #[serde(default)]
    pub batch_ids: Vec<MovieBatchId>,
}

/// Request to fetch a subset of series bundles for a library.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesBundleFetchRequest {
    #[serde(default)]
    pub series_ids: Vec<SeriesID>,
}
