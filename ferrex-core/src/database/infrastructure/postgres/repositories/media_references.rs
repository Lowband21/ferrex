use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;

use crate::database::ports::media_references::MediaReferencesRepository;
use crate::database::postgres::PostgresDatabase;
use crate::database::traits::MediaDatabaseTrait;
use crate::error::Result;
use crate::types::ids::{EpisodeID, LibraryID, MovieID, SeasonID, SeriesID};
use crate::types::library::LibraryType;
use crate::types::media::{
    EpisodeReference, Media, MovieReference, SeasonReference, SeriesReference,
};

#[derive(Clone)]
pub struct PostgresMediaReferencesRepository {
    db: Arc<PostgresDatabase>,
}

impl PostgresMediaReferencesRepository {
    pub fn new(db: Arc<PostgresDatabase>) -> Self {
        Self { db }
    }
}

impl fmt::Debug for PostgresMediaReferencesRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pool = self.db.pool();
        f.debug_struct("PostgresMediaReferencesRepository")
            .field("pool_size", &pool.size())
            .field("idle_connections", &pool.num_idle())
            .finish()
    }
}

#[async_trait]
impl MediaReferencesRepository for PostgresMediaReferencesRepository {
    async fn store_movie_reference(
        &self,
        movie: &MovieReference,
    ) -> Result<()> {
        self.db.store_movie_reference(movie).await
    }

    async fn store_series_reference(
        &self,
        series: &SeriesReference,
    ) -> Result<()> {
        self.db.store_series_reference(series).await
    }

    async fn store_season_reference(
        &self,
        season: &SeasonReference,
    ) -> Result<uuid::Uuid> {
        self.db.store_season_reference(season).await
    }

    async fn store_episode_reference(
        &self,
        episode: &EpisodeReference,
    ) -> Result<()> {
        self.db.store_episode_reference(episode).await
    }

    async fn get_movie_reference(
        &self,
        id: &MovieID,
    ) -> Result<MovieReference> {
        self.db.get_movie_reference(id).await
    }

    async fn get_series_reference(
        &self,
        id: &SeriesID,
    ) -> Result<SeriesReference> {
        self.db.get_series_reference(id).await
    }

    async fn get_season_reference(
        &self,
        id: &SeasonID,
    ) -> Result<SeasonReference> {
        self.db.get_season_reference(id).await
    }

    async fn get_episode_reference(
        &self,
        id: &EpisodeID,
    ) -> Result<EpisodeReference> {
        self.db.get_episode_reference(id).await
    }

    async fn get_all_movie_references(&self) -> Result<Vec<MovieReference>> {
        self.db.get_all_movie_references().await
    }

    async fn get_series_references(&self) -> Result<Vec<SeriesReference>> {
        self.db.get_series_references().await
    }

    async fn get_series_seasons(
        &self,
        series_id: &SeriesID,
    ) -> Result<Vec<SeasonReference>> {
        self.db.get_series_seasons(series_id).await
    }

    async fn get_season_episodes(
        &self,
        season_id: &SeasonID,
    ) -> Result<Vec<EpisodeReference>> {
        self.db.get_season_episodes(season_id).await
    }

    async fn get_library_media_references(
        &self,
        library_id: LibraryID,
        library_type: LibraryType,
    ) -> Result<Vec<Media>> {
        self.db
            .get_library_media_references(library_id, library_type)
            .await
    }

    async fn get_library_series(
        &self,
        library_id: &LibraryID,
    ) -> Result<Vec<SeriesReference>> {
        self.db.get_library_series(library_id).await
    }

    async fn get_library_seasons(
        &self,
        library_id: &LibraryID,
    ) -> Result<Vec<SeasonReference>> {
        self.db.get_library_seasons(library_id).await
    }

    async fn get_library_episodes(
        &self,
        library_id: &LibraryID,
    ) -> Result<Vec<EpisodeReference>> {
        self.db.get_library_episodes(library_id).await
    }

    async fn get_movie_references_bulk(
        &self,
        ids: &[&MovieID],
    ) -> Result<Vec<MovieReference>> {
        self.db.get_movie_references_bulk(ids).await
    }

    async fn get_series_references_bulk(
        &self,
        ids: &[&SeriesID],
    ) -> Result<Vec<SeriesReference>> {
        self.db.get_series_references_bulk(ids).await
    }

    async fn get_season_references_bulk(
        &self,
        ids: &[&SeasonID],
    ) -> Result<Vec<SeasonReference>> {
        self.db.get_season_references_bulk(ids).await
    }

    async fn get_episode_references_bulk(
        &self,
        ids: &[&EpisodeID],
    ) -> Result<Vec<EpisodeReference>> {
        self.db.get_episode_references_bulk(ids).await
    }

    async fn get_movie_reference_by_path(
        &self,
        path: &str,
    ) -> Result<Option<MovieReference>> {
        self.db.get_movie_reference_by_path(path).await
    }

    async fn get_series_by_tmdb_id(
        &self,
        library_id: LibraryID,
        tmdb_id: u64,
    ) -> Result<Option<SeriesReference>> {
        self.db.get_series_by_tmdb_id(library_id, tmdb_id).await
    }

    async fn find_series_by_name(
        &self,
        library_id: LibraryID,
        name: &str,
    ) -> Result<Option<SeriesReference>> {
        self.db.find_series_by_name(library_id, name).await
    }

    async fn update_movie_tmdb_id(
        &self,
        id: &MovieID,
        tmdb_id: u64,
    ) -> Result<()> {
        self.db.update_movie_tmdb_id(id, tmdb_id).await
    }

    async fn update_series_tmdb_id(
        &self,
        id: &SeriesID,
        tmdb_id: u64,
    ) -> Result<()> {
        self.db.update_series_tmdb_id(id, tmdb_id).await
    }
}
