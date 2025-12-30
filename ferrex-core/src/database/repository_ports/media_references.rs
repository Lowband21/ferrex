use async_trait::async_trait;
use ferrex_model::MediaID;

use crate::{
    error::Result,
    types::{
        EpisodeID, EpisodeReference, LibraryId, Media, MovieBatchId, MovieID,
        MovieReference, SeasonID, SeasonReference, Series, SeriesID,
        library::LibraryType,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TvReferenceOrphanCleanup {
    pub deleted_seasons: u64,
    pub deleted_series: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovieBatchVersionRecord {
    pub batch_id: MovieBatchId,
    pub version: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovieBatchManifestRecord {
    pub batch_id: MovieBatchId,
    pub version: u64,
    pub content_hash: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeriesBundleVersionRecord {
    pub series_id: SeriesID,
    pub version: u64,
}

/// Repository port for media references in the catalog context.
///
/// Focuses on lightweight references (Movie/Series/Season/Episode) used by
/// navigation, lists, and query flows.
#[async_trait]
pub trait MediaReferencesRepository: Send + Sync {
    // Store operations
    /// Store/refresh a movie reference and return the canonical reference id.
    ///
    /// Important: this may differ from the passed `movie.id` when the DB
    /// resolves conflicts (e.g. unique `(tmdb_id, library_id)`), so callers
    /// must use the returned UUID as the logical id for downstream work.
    async fn store_movie_reference(
        &self,
        movie: &MovieReference,
    ) -> Result<MediaID>;
    async fn store_series_reference(&self, series: &Series) -> Result<MediaID>;
    async fn store_season_reference(
        &self,
        season: &SeasonReference,
    ) -> Result<MediaID>;
    async fn store_episode_reference(
        &self,
        episode: &EpisodeReference,
    ) -> Result<MediaID>;

    // Individual lookups
    async fn get_media_reference(&self, id: &MediaID) -> Result<Media>;

    async fn get_movie_reference(&self, id: &MovieID)
    -> Result<MovieReference>;
    async fn get_series_reference(&self, id: &SeriesID) -> Result<Series>;
    async fn get_season_reference(
        &self,
        id: &SeasonID,
    ) -> Result<SeasonReference>;
    async fn get_episode_reference(
        &self,
        id: &EpisodeID,
    ) -> Result<EpisodeReference>;

    async fn mark_series_finalized(
        &self,
        lib_id: &LibraryId,
        id: &SeriesID,
    ) -> Result<()>;

    async fn upsert_series_bundle_hash(
        &self,
        lib_id: &LibraryId,
        id: &SeriesID,
        hash: u64,
    ) -> Result<()>;

    async fn upsert_movie_batch_hash(
        &self,
        lib_id: &LibraryId,
        id: &MovieBatchId,
        hash: u64,
        batch_size: u32,
    ) -> Result<()>;
    // Bulk retrieval
    async fn get_all_movie_references(&self) -> Result<Vec<MovieReference>>;
    async fn get_movie_references_by_batch(
        &self,
        library_id: &LibraryId,
        batch_id: MovieBatchId,
    ) -> Result<Vec<MovieReference>>;

    /// Fetch movie references for multiple batches in a single query.
    ///
    /// The returned list is ordered deterministically by `(batch_id, movie_id)`.
    async fn get_movie_references_for_batches(
        &self,
        library_id: &LibraryId,
        batch_ids: &[MovieBatchId],
    ) -> Result<Vec<MovieReference>>;
    async fn list_finalized_movie_reference_batches(
        &self,
        library_id: &LibraryId,
    ) -> Result<Vec<MovieBatchId>>;

    /// List (batch_id, version) for finalized movie batches in a library.
    async fn list_finalized_movie_batch_versions(
        &self,
        library_id: &LibraryId,
    ) -> Result<Vec<MovieBatchVersionRecord>>;

    /// Get the current unfinalized movie batch id for a library (if any).
    ///
    /// A library is expected to have at most one unfinalized batch at a time,
    /// which is appended to until it reaches the configured batch size.
    async fn get_unfinalized_movie_reference_batch_id(
        &self,
        library_id: &LibraryId,
    ) -> Result<Option<MovieBatchId>>;

    /// Fetch the persisted hash for a movie batch (when present).
    ///
    /// Returns `Ok(None)` when the row or hash is missing.
    async fn get_movie_batch_hash(
        &self,
        library_id: &LibraryId,
        batch_id: MovieBatchId,
    ) -> Result<Option<u64>>;

    /// List movie batch ids for a library that currently contain at least one movie.
    ///
    /// This includes the trailing unfinalized batch when it has movies.
    async fn list_movie_reference_batches_with_movies(
        &self,
        library_id: &LibraryId,
    ) -> Result<Vec<MovieBatchId>>;

    /// List (batch_id, version) for movie batches that currently contain at least
    /// one movie, including the trailing unfinalized batch.
    async fn list_movie_batch_versions_with_movies(
        &self,
        library_id: &LibraryId,
    ) -> Result<Vec<MovieBatchVersionRecord>>;

    /// List (batch_id, version, content_hash) for movie batches that currently contain at least
    /// one movie, including the trailing unfinalized batch.
    ///
    /// `content_hash` is derived from the rkyv payload bytes (sha256, first 8 bytes, big-endian).
    async fn list_movie_batch_manifest_with_movies(
        &self,
        library_id: &LibraryId,
    ) -> Result<Vec<MovieBatchManifestRecord>>;
    async fn list_movie_reference_batches(
        &self,
        library_id: &LibraryId,
    ) -> Result<Vec<MovieBatchId>>;
    async fn get_series(&self) -> Result<Vec<Series>>;
    async fn get_series_seasons(
        &self,
        series_id: &SeriesID,
    ) -> Result<Vec<SeasonReference>>;
    async fn get_series_episodes(
        &self,
        series_id: &SeriesID,
    ) -> Result<Vec<EpisodeReference>>;
    async fn get_season_episodes(
        &self,
        season_id: &SeasonID,
    ) -> Result<Vec<EpisodeReference>>;

    async fn get_library_media_references(
        &self,
        library_id: LibraryId,
        library_type: LibraryType,
    ) -> Result<Vec<Media>>;
    async fn get_library_series(
        &self,
        library_id: &LibraryId,
    ) -> Result<Vec<Series>>;

    /// List series ids in a library that currently have at least one episode reference.
    ///
    /// This is used to scope player-visible series and to avoid treating orphan
    /// `series` rows as real library content.
    async fn list_library_series_ids_with_episodes(
        &self,
        library_id: &LibraryId,
    ) -> Result<Vec<SeriesID>>;

    /// List (series_id, version) for finalized series bundles in a library.
    async fn list_finalized_series_bundle_versions(
        &self,
        library_id: &LibraryId,
    ) -> Result<Vec<SeriesBundleVersionRecord>>;
    async fn get_library_seasons(
        &self,
        library_id: &LibraryId,
    ) -> Result<Vec<SeasonReference>>;
    async fn get_library_episodes(
        &self,
        library_id: &LibraryId,
    ) -> Result<Vec<EpisodeReference>>;

    async fn get_movie_references_bulk(
        &self,
        ids: &[&MovieID],
    ) -> Result<Vec<MovieReference>>;
    async fn get_series_bulk(&self, ids: &[&SeriesID]) -> Result<Vec<Series>>;
    async fn get_season_references_bulk(
        &self,
        ids: &[&SeasonID],
    ) -> Result<Vec<SeasonReference>>;
    async fn get_episode_references_bulk(
        &self,
        ids: &[&EpisodeID],
    ) -> Result<Vec<EpisodeReference>>;

    // Specialized lookups / updates
    async fn get_movie_reference_by_path(
        &self,
        path: &str,
    ) -> Result<Option<MovieReference>>;
    async fn get_series_by_tmdb_id(
        &self,
        library_id: LibraryId,
        tmdb_id: u64,
    ) -> Result<Option<Series>>;

    async fn update_movie_tmdb_id(
        &self,
        id: &MovieID,
        tmdb_id: u64,
    ) -> Result<()>;
    async fn update_series_tmdb_id(
        &self,
        id: &SeriesID,
        tmdb_id: u64,
    ) -> Result<()>;

    /// Remove orphan series/seasons that no longer have any episode references.
    ///
    /// This is useful when callers perform targeted media file deletions (e.g.
    /// demo pruning) without running a full rescan that would otherwise repair
    /// the catalog hierarchy.
    async fn cleanup_orphan_tv_references(
        &self,
        library_id: LibraryId,
    ) -> Result<TvReferenceOrphanCleanup>;
}
