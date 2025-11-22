use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    error::Result,
    types::{
        EpisodeID, EpisodeReference, LibraryId, Media, MovieID, MovieReference,
        SeasonID, SeasonReference, SeriesID, SeriesReference,
        library::LibraryType,
    },
};

/// Repository port for media references in the catalog context.
///
/// Focuses on lightweight references (Movie/Series/Season/Episode) used by
/// navigation, lists, and query flows.
#[async_trait]
pub trait MediaReferencesRepository: Send + Sync {
    // Store operations
    async fn store_movie_reference(&self, movie: &MovieReference)
    -> Result<()>;
    async fn store_series_reference(
        &self,
        series: &SeriesReference,
    ) -> Result<()>;
    async fn store_season_reference(
        &self,
        season: &SeasonReference,
    ) -> Result<Uuid>;
    async fn store_episode_reference(
        &self,
        episode: &EpisodeReference,
    ) -> Result<()>;

    // Individual lookups
    async fn get_movie_reference(&self, id: &MovieID)
    -> Result<MovieReference>;
    async fn get_series_reference(
        &self,
        id: &SeriesID,
    ) -> Result<SeriesReference>;
    async fn get_season_reference(
        &self,
        id: &SeasonID,
    ) -> Result<SeasonReference>;
    async fn get_episode_reference(
        &self,
        id: &EpisodeID,
    ) -> Result<EpisodeReference>;

    // Bulk retrieval
    async fn get_all_movie_references(&self) -> Result<Vec<MovieReference>>;
    async fn get_series_references(&self) -> Result<Vec<SeriesReference>>;
    async fn get_series_seasons(
        &self,
        series_id: &SeriesID,
    ) -> Result<Vec<SeasonReference>>;
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
    ) -> Result<Vec<SeriesReference>>;
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
    async fn get_series_references_bulk(
        &self,
        ids: &[&SeriesID],
    ) -> Result<Vec<SeriesReference>>;
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
    ) -> Result<Option<SeriesReference>>;
    async fn find_series_by_name(
        &self,
        library_id: LibraryId,
        name: &str,
    ) -> Result<Option<SeriesReference>>;

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
}
