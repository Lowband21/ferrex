//! Library domain types

use ferrex_core::player_prelude::Library;
use ferrex_core::player_prelude::LibraryId;
use ferrex_core::player_prelude::{MovieBatchId, SeriesID};
use rkyv::util::AlignedVec;

/// Library form data for creating/editing libraries
#[derive(Debug, Clone)]
pub struct LibraryFormData {
    pub id: LibraryId,
    pub name: String,
    pub library_type: String,
    pub paths: String, // comma-separated paths as entered by user
    pub scan_interval_minutes: String,
    pub enabled: bool,
    pub editing: bool, // true if editing existing library, false if creating new
    pub start_scan: bool,
}

/// Supplemental bootstrap payloads (movie batches) fetched/loaded alongside the libraries snapshot.
#[derive(Debug, Clone)]
pub struct MovieBatchInstallCart {
    pub library_id: LibraryId,
    pub batch_id: MovieBatchId,
    pub version: u64,
    pub cart: AlignedVec,
}

/// Supplemental bootstrap payloads (series bundles) fetched/loaded alongside the libraries snapshot.
#[derive(Debug, Clone)]
pub struct SeriesBundleInstallCart {
    pub library_id: LibraryId,
    pub series_id: SeriesID,
    pub version: u64,
    pub cart: AlignedVec,
}

/// Libraries bootstrap payload delivered by the existing `LibrariesLoaded` message.
///
/// This keeps the "load libraries" flow gated and consistent, while still
/// allowing additional startup payloads (like movie batch bundles) to be
/// fetched within the same async task.
#[derive(Debug, Clone)]
pub struct LibrariesBootstrapPayload {
    pub libraries: Vec<Library>,
    pub movie_batches: Vec<MovieBatchInstallCart>,
    pub series_bundles: Vec<SeriesBundleInstallCart>,
}
