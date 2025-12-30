use crate::{
    database::{
        postgres_ext::{
            EpisodeReferenceRow, MovieReferenceRow, SeasonReferenceRow,
            SeriesReferenceRow, TmdbMetadataRepository,
        },
        repository_ports::media_references::{
            MediaReferencesRepository, MovieBatchManifestRecord,
            MovieBatchVersionRecord, SeriesBundleVersionRecord,
            TvReferenceOrphanCleanup,
        },
    },
    error::{MediaError, Result},
    types::{
        ids::{EpisodeID, LibraryId, MovieID, SeasonID, SeriesID},
        library::LibraryType,
        media::{
            EpisodeReference, Media, MovieReference, SeasonReference, Series,
        },
    },
};

use async_trait::async_trait;
use ferrex_model::{MediaID, MovieBatchId};
use num_bigint::BigUint;
use rayon::iter::{IntoParallelIterator, ParallelExtend, ParallelIterator};
use sqlx::{
    PgPool,
    types::{BigDecimal, Uuid},
};
use tracing::{error, info};

#[derive(Clone, Debug)]
pub struct PostgresMediaReferencesRepository {
    pool: PgPool,
}

impl PostgresMediaReferencesRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
    /// Store MovieReference within an existing transaction
    /// Get movie with optional full metadata
    pub async fn get_movie(&self, id: &MovieID) -> Result<MovieReference> {
        let movie_uuid = id.to_uuid();

        let row = sqlx::query_as!(
            MovieReferenceRow,
            r#"
            SELECT
                mr.id,
                mr.tmdb_id,
                mr.title,
                mr.theme_color,
                mr.batch_id,
                mr.library_id,
                mf.id AS file_id,
                mf.file_path,
                mf.filename,
                mf.file_size,
                mf.discovered_at AS file_discovered_at,
                mf.created_at AS file_created_at,
                mf.technical_metadata
            FROM movie_references mr
            JOIN media_files mf ON mr.file_id = mf.id
            WHERE mr.id = $1
            "#,
            movie_uuid
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?;

        let Some(row) = row else {
            return Err(MediaError::NotFound(format!(
                "Movie with id {:#?} not found",
                id,
            )));
        };

        let repository = TmdbMetadataRepository::new(&self.pool);
        let movie_ref = repository.load_movie_reference(row).await?;

        Ok(movie_ref)
    }
}

#[async_trait]
impl MediaReferencesRepository for PostgresMediaReferencesRepository {
    async fn get_library_media_references(
        &self,
        library_id: LibraryId,
        library_type: LibraryType,
    ) -> Result<Vec<Media>> {
        let mut media = Vec::new();
        match library_type {
            LibraryType::Movies => {
                let repository = TmdbMetadataRepository::new(&self.pool);
                let rows = sqlx::query_as!(
                    MovieReferenceRow,
                    r#"
                    SELECT
                        mr.id,
                        mr.tmdb_id,
                        mr.title,
                        mr.theme_color,
                        mr.batch_id,
                        mf.id AS file_id,
                        mf.library_id,
                        mf.file_path,
                        mf.filename,
                        mf.file_size,
                        mf.discovered_at AS file_discovered_at,
                        mf.created_at AS file_created_at,
                        mf.technical_metadata
                    FROM movie_references mr
                    JOIN media_files mf ON mr.file_id = mf.id
                    WHERE mf.library_id = $1
                    ORDER BY mr.title
                    "#,
                    library_id.as_uuid()
                )
                .fetch_all(&self.pool)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!(
                        "Database query failed: {}",
                        e
                    ))
                })?;

                let movies =
                    repository.load_movie_references_bulk(rows).await?;
                media.extend(
                    movies
                        .into_iter()
                        .map(|movie| Media::Movie(Box::new(movie))),
                );
            }
            LibraryType::Series => {
                // Execute bulk queries in parallel using tokio::join!
                let (series_result, seasons_result, episodes_result) = tokio::join!(
                    self.get_library_series(&library_id),
                    self.get_library_seasons(&library_id),
                    self.get_library_episodes(&library_id)
                );
                match series_result {
                    Ok(series) => media.par_extend(
                        series
                            .into_par_iter()
                            .map(|sref: Series| Media::Series(Box::new(sref))),
                    ),
                    Err(e) => {
                        error!("Failed to get series with error: {}", e)
                    }
                }
                match seasons_result {
                    Ok(season) => media.par_extend(season.into_par_iter().map(
                        |sref: SeasonReference| Media::Season(Box::new(sref)),
                    )),
                    Err(e) => {
                        error!("Failed to get season with error: {}", e)
                    }
                }
                match episodes_result {
                    Ok(episode) => {
                        media.par_extend(episode.into_par_iter().map(
                            |sref: EpisodeReference| {
                                Media::Episode(Box::new(sref))
                            },
                        ))
                    }
                    Err(e) => {
                        error!("Failed to get episode with error: {}", e)
                    }
                }
            }
        }

        Ok(media)
    }

    // Lookup a single movie by file path
    async fn get_movie_reference_by_path(
        &self,
        path: &str,
    ) -> Result<Option<MovieReference>> {
        let repository = TmdbMetadataRepository::new(&self.pool);
        let row = sqlx::query_as!(
            MovieReferenceRow,
            r#"
            SELECT
                mr.id,
                mr.tmdb_id,
                mr.title,
                mr.theme_color,
                mr.batch_id,
                mf.id AS file_id,
                mf.library_id,
                mf.file_path,
                mf.filename,
                mf.file_size,
                mf.discovered_at AS file_discovered_at,
                mf.created_at AS file_created_at,
                mf.technical_metadata
            FROM movie_references mr
            JOIN media_files mf ON mr.file_id = mf.id
            WHERE mf.file_path = $1
            LIMIT 1
            "#,
            path
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?;

        if let Some(row) = row {
            let movie = repository.load_movie_reference(row).await?;
            Ok(Some(movie))
        } else {
            Ok(None)
        }
    }

    // Bulk reference retrieval methods for performance
    async fn get_movie_references_bulk(
        &self,
        ids: &[&MovieID],
    ) -> Result<Vec<MovieReference>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        // Convert IDs to UUIDs
        let uuids: Vec<Uuid> = ids.iter().map(|id| id.to_uuid()).collect();
        let repository = TmdbMetadataRepository::new(&self.pool);

        let rows = sqlx::query_as!(
            MovieReferenceRow,
            r#"
            SELECT
                mr.id,
                mr.tmdb_id,
                mr.title,
                mr.theme_color,
                mr.batch_id,
                mf.id AS file_id,
                mf.library_id,
                mf.file_path,
                mf.filename,
                mf.file_size,
                mf.discovered_at AS file_discovered_at,
                mf.created_at AS file_created_at,
                mf.technical_metadata
            FROM movie_references mr
            JOIN media_files mf ON mr.file_id = mf.id
            WHERE mr.id = ANY($1)
            "#,
            uuids.as_slice()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?;

        repository.load_movie_references_bulk(rows).await
    }

    async fn get_series_bulk(&self, ids: &[&SeriesID]) -> Result<Vec<Series>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        // Convert IDs to UUIDs
        let uuids: Vec<Uuid> = ids.iter().map(|id| id.to_uuid()).collect();

        let repository = TmdbMetadataRepository::new(&self.pool);

        let rows = sqlx::query_as!(
            SeriesReferenceRow,
            r#"
            SELECT
                sr.id,
                sr.library_id,
                sr.tmdb_id,
                sr.title,
                sr.theme_color,
                sr.discovered_at AS "discovered_at!",
                sr.created_at AS "created_at!"
            FROM series sr
            WHERE sr.id = ANY($1)
            "#,
            uuids.as_slice()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?;

        repository.load_series_bulk(rows).await
    }

    async fn get_library_series(
        &self,
        library_id: &LibraryId,
    ) -> Result<Vec<Series>> {
        let repository = TmdbMetadataRepository::new(&self.pool);

        let rows = sqlx::query_as!(
            SeriesReferenceRow,
            r#"
            SELECT
                sr.id,
                sr.library_id,
                sr.tmdb_id,
                sr.title,
                sr.theme_color,
                sr.discovered_at AS "discovered_at!",
                sr.created_at AS "created_at!"
            FROM series sr
            WHERE sr.library_id = $1
            ORDER BY sr.title
            "#,
            library_id.as_uuid()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?;

        repository.load_series_bulk(rows).await
    }

    async fn list_library_series_ids_with_episodes(
        &self,
        library_id: &LibraryId,
    ) -> Result<Vec<SeriesID>> {
        let rows = sqlx::query!(
            r#"
            SELECT DISTINCT s.id
            FROM episode_references er
            INNER JOIN series s ON s.id = er.series_id
            WHERE s.library_id = $1
            ORDER BY s.id
            "#,
            library_id.as_uuid()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Database query failed for series-with-episodes list: {}",
                e
            ))
        })?;

        Ok(rows.into_iter().map(|row| SeriesID(row.id)).collect())
    }

    async fn list_finalized_series_bundle_versions(
        &self,
        library_id: &LibraryId,
    ) -> Result<Vec<SeriesBundleVersionRecord>> {
        let rows = sqlx::query!(
            r#"
            SELECT sbv.series_id, sbv.version
            FROM series_bundle_versioning sbv
            INNER JOIN series s ON s.id = sbv.series_id
            WHERE sbv.library_id = $1
              AND s.library_id = $1
              AND sbv.finalized = true
            ORDER BY sbv.series_id
            "#,
            library_id.as_uuid()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Database query failed for series bundle version list: {}",
                e
            ))
        })?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let series_id = crate::types::ids::SeriesID(row.series_id);
            let version = u64::try_from(row.version).map_err(|_| {
                MediaError::Internal(format!(
                    "Invalid series bundle version {} for library {} series {}",
                    row.version, library_id, series_id
                ))
            })?;

            out.push(SeriesBundleVersionRecord { series_id, version });
        }

        Ok(out)
    }

    async fn get_season_references_bulk(
        &self,
        ids: &[&SeasonID],
    ) -> Result<Vec<SeasonReference>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        // Convert IDs to UUIDs
        let uuids: Vec<Uuid> = ids.iter().map(|id| id.to_uuid()).collect();

        let repository = TmdbMetadataRepository::new(&self.pool);

        let rows = sqlx::query_as!(
            SeasonReferenceRow,
            r#"
            SELECT
                sr.id,
                sr.series_id,
                sr.season_number,
                sr.library_id,
                sr.tmdb_series_id,
                sr.discovered_at AS "discovered_at!",
                sr.created_at AS "created_at!",
                sr.theme_color
            FROM season_references sr
            WHERE sr.id = ANY($1)
            "#,
            uuids.as_slice()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to get seasons: {}", e))
        })?;

        repository.load_season_references_bulk(rows).await
    }

    async fn get_library_seasons(
        &self,
        library_id: &LibraryId,
    ) -> Result<Vec<SeasonReference>> {
        let repository = TmdbMetadataRepository::new(&self.pool);

        let rows = sqlx::query_as!(
            SeasonReferenceRow,
            r#"
            SELECT
                sr.id,
                sr.series_id,
                sr.season_number,
                sr.library_id,
                sr.tmdb_series_id,
                sr.discovered_at AS "discovered_at!",
                sr.created_at AS "created_at!",
                sr.theme_color
            FROM season_references sr
            WHERE sr.library_id = $1
            ORDER BY sr.series_id, sr.season_number
            "#,
            library_id.as_uuid()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to get seasons: {}", e))
        })?;

        repository.load_season_references_bulk(rows).await
    }

    async fn get_episode_references_bulk(
        &self,
        ids: &[&EpisodeID],
    ) -> Result<Vec<EpisodeReference>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        // Convert IDs to UUIDs
        let uuids: Vec<Uuid> = ids.iter().map(|id| id.to_uuid()).collect();

        let repository = TmdbMetadataRepository::new(&self.pool);

        let rows = sqlx::query_as!(
            EpisodeReferenceRow,
            r#"
            SELECT
                er.id,
                er.episode_number,
                er.season_number,
                er.season_id,
                er.series_id,
                er.tmdb_series_id,
                er.discovered_at AS episode_discovered_at,
                er.created_at AS episode_created_at,
                mf.id AS file_id,
                mf.library_id,
                mf.file_path,
                mf.filename,
                mf.file_size,
                mf.discovered_at AS file_discovered_at,
                mf.created_at AS file_created_at,
                mf.technical_metadata
            FROM episode_references er
            JOIN media_files mf ON er.file_id = mf.id
            WHERE er.id = ANY($1)
            "#,
            uuids.as_slice()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to get episodes: {}", e))
        })?;

        repository.load_episode_references_bulk(rows).await
    }

    async fn get_library_episodes(
        &self,
        library_id: &LibraryId,
    ) -> Result<Vec<EpisodeReference>> {
        let repository = TmdbMetadataRepository::new(&self.pool);

        let rows = sqlx::query_as!(
            EpisodeReferenceRow,
            r#"
            SELECT
                er.id,
                er.episode_number,
                er.season_number,
                er.season_id,
                er.series_id,
                er.tmdb_series_id,
                er.discovered_at AS episode_discovered_at,
                er.created_at AS episode_created_at,
                mf.id AS file_id,
                mf.library_id,
                mf.file_path,
                mf.filename,
                mf.file_size,
                mf.discovered_at AS file_discovered_at,
                mf.created_at AS file_created_at,
                mf.technical_metadata
            FROM episode_references er
            JOIN series sr ON sr.id = er.series_id AND sr.library_id = $1
            JOIN media_files mf ON er.file_id = mf.id
            ORDER BY er.series_id ASC, er.season_number ASC, er.episode_number ASC
            "#,
            library_id.as_uuid()
        )
            .fetch_all(&self.pool)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to get episodes: {}", e)))?;

        repository.load_episode_references_bulk(rows).await
    }

    async fn get_series_episodes(
        &self,
        series_id: &SeriesID,
    ) -> Result<Vec<EpisodeReference>> {
        let repository = TmdbMetadataRepository::new(&self.pool);

        let rows = sqlx::query_as!(
            EpisodeReferenceRow,
            r#"
            SELECT
                er.id,
                er.episode_number,
                er.season_number,
                er.season_id,
                er.series_id,
                er.tmdb_series_id,
                er.discovered_at AS episode_discovered_at,
                er.created_at AS episode_created_at,
                mf.id AS file_id,
                mf.library_id,
                mf.file_path,
                mf.filename,
                mf.file_size,
                mf.discovered_at AS file_discovered_at,
                mf.created_at AS file_created_at,
                mf.technical_metadata
            FROM episode_references er
            JOIN media_files mf ON er.file_id = mf.id
            WHERE er.series_id = $1
            ORDER BY er.season_number ASC, er.episode_number ASC
            "#,
            series_id.to_uuid()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to get series episodes: {}",
                e
            ))
        })?;

        repository.load_episode_references_bulk(rows).await
    }
    async fn store_movie_reference(
        &self,
        movie: &MovieReference,
    ) -> Result<MediaID> {
        let movie_id = TmdbMetadataRepository::new(&self.pool)
            .store_movie_reference(movie)
            .await?;
        Ok(MediaID::from(movie_id))
    }

    async fn store_series_reference(&self, series: &Series) -> Result<MediaID> {
        let series_id = TmdbMetadataRepository::new(&self.pool)
            .store_series_reference(series)
            .await?;
        Ok(MediaID::from(series_id))
    }

    async fn store_season_reference(
        &self,
        season: &SeasonReference,
    ) -> Result<MediaID> {
        let season_id = TmdbMetadataRepository::new(&self.pool)
            .store_season_reference(season)
            .await?;
        Ok(MediaID::from(season_id))
    }

    async fn store_episode_reference(
        &self,
        episode: &EpisodeReference,
    ) -> Result<MediaID> {
        let episode_id = TmdbMetadataRepository::new(&self.pool)
            .store_episode_reference(episode)
            .await?;
        Ok(MediaID::from(episode_id))
    }

    async fn get_media_reference(&self, id: &MediaID) -> Result<Media> {
        match id {
            MediaID::Movie(movie_id) => self
                .get_movie_reference(movie_id)
                .await
                .map(|m| Media::Movie(Box::new(m))),
            MediaID::Series(series_id) => self
                .get_series_reference(series_id)
                .await
                .map(|m| Media::Series(Box::new(m))),
            MediaID::Season(season_id) => self
                .get_season_reference(season_id)
                .await
                .map(|m| Media::Season(Box::new(m))),
            MediaID::Episode(episode_id) => self
                .get_episode_reference(episode_id)
                .await
                .map(|m| Media::Episode(Box::new(m))),
        }
    }

    async fn get_movie_reference(
        &self,
        id: &MovieID,
    ) -> Result<MovieReference> {
        self.get_movie(id).await
    }

    async fn get_series_reference(&self, id: &SeriesID) -> Result<Series> {
        let series_uuid = id.to_uuid();

        let row = sqlx::query_as!(
            SeriesReferenceRow,
            r#"
            SELECT
                sr.id,
                sr.library_id,
                sr.tmdb_id,
                sr.title,
                sr.theme_color,
                sr.discovered_at AS "discovered_at!",
                sr.created_at AS "created_at!"
            FROM series sr
            WHERE sr.id = $1
            "#,
            series_uuid
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?
        .ok_or_else(|| MediaError::NotFound("Series not found".to_string()))?;

        let repository = TmdbMetadataRepository::new(&self.pool);
        let series_ref = repository.load_series_reference(row).await?;

        Ok(series_ref)
    }

    async fn get_season_reference(
        &self,
        id: &SeasonID,
    ) -> Result<SeasonReference> {
        let season_uuid = id.to_uuid();

        let row = sqlx::query_as!(
            SeasonReferenceRow,
            r#"
            SELECT
                id,
                series_id,
                season_number,
                library_id,
                tmdb_series_id,
                discovered_at AS "discovered_at!",
                created_at AS "created_at!",
                theme_color
            FROM season_references
            WHERE id = $1
            "#,
            season_uuid
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?
        .ok_or_else(|| MediaError::NotFound("Season not found".to_string()))?;

        let repository = TmdbMetadataRepository::new(&self.pool);
        let season_ref = repository.load_season_reference(row).await?;

        Ok(season_ref)
    }

    async fn get_episode_reference(
        &self,
        id: &EpisodeID,
    ) -> Result<EpisodeReference> {
        let episode_uuid = id.to_uuid();

        let row = sqlx::query_as!(
            EpisodeReferenceRow,
            r#"
            SELECT
                er.id,
                er.episode_number,
                er.season_number,
                er.season_id,
                er.series_id,
                er.tmdb_series_id,
                er.discovered_at AS episode_discovered_at,
                er.created_at AS episode_created_at,
                mf.id AS file_id,
                mf.library_id,
                mf.file_path,
                mf.filename,
                mf.file_size,
                mf.discovered_at AS file_discovered_at,
                mf.created_at AS file_created_at,
                mf.technical_metadata
            FROM episode_references er
            JOIN media_files mf ON er.file_id = mf.id
            WHERE er.id = $1
            "#,
            episode_uuid
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?
        .ok_or_else(|| MediaError::NotFound("Episode not found".to_string()))?;

        let repository = TmdbMetadataRepository::new(&self.pool);
        let episode_ref = repository.load_episode_reference(row).await?;

        Ok(episode_ref)
    }

    async fn mark_series_finalized(
        &self,
        lib_id: &LibraryId,
        id: &SeriesID,
    ) -> Result<()> {
        let row = sqlx::query!(
            r#"
                INSERT INTO series_bundle_versioning
                (library_id, series_id, finalized)
                VALUES ($1, $2, $3)
                ON CONFLICT (library_id, series_id) DO UPDATE SET
                finalized = true,
                updated_at = NOW()
            "#,
            lib_id.to_uuid(),
            id.to_uuid(),
            true
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?;

        let rows_affected = row.rows_affected();
        if rows_affected == 1 {
            Ok(())
        } else {
            error!(
                "Affected {} rows with mark_series_finalized (not the correct 1 row modified)",
                rows_affected
            );
            Ok(())
        }
    }

    async fn upsert_series_bundle_hash(
        &self,
        lib_id: &LibraryId,
        id: &SeriesID,
        hash: u64,
    ) -> Result<()> {
        let hash_bigint = BigDecimal::from_biguint(BigUint::from(hash), 0);

        let row = sqlx::query!(
            r#"
                INSERT INTO series_bundle_versioning
                (library_id, series_id, finalized, bundle_hash)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (library_id, series_id) DO UPDATE SET
                version = CASE
                    WHEN series_bundle_versioning.bundle_hash IS DISTINCT FROM EXCLUDED.bundle_hash
                    THEN series_bundle_versioning.version + 1
                    ELSE series_bundle_versioning.version
                END,
                finalized = EXCLUDED.finalized,
                bundle_hash = EXCLUDED.bundle_hash,
                updated_at = CASE
                    WHEN series_bundle_versioning.bundle_hash IS DISTINCT FROM EXCLUDED.bundle_hash
                    THEN NOW()
                    ELSE series_bundle_versioning.updated_at
                END
            "#,
            lib_id.to_uuid(),
            id.to_uuid(),
            true,
            Some(hash_bigint),
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?;

        let rows_affected = row.rows_affected();
        if rows_affected == 1 {
            Ok(())
        } else {
            error!(
                "Affected {} rows with mark_series_finalized (not the correct 1 row modified)",
                rows_affected
            );
            Ok(())
        }
    }

    async fn upsert_movie_batch_hash(
        &self,
        lib_id: &LibraryId,
        id: &MovieBatchId,
        hash: u64,
        batch_size: u32,
    ) -> Result<()> {
        let hash_bigint = BigDecimal::from_biguint(BigUint::from(hash), 0);

        let row = sqlx::query!(
            r#"
                INSERT INTO movie_reference_batches
                (library_id, batch_id, batch_size, batch_hash)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (library_id, batch_id) DO UPDATE SET
                batch_size = EXCLUDED.batch_size,
                version = CASE
                    WHEN movie_reference_batches.batch_hash IS DISTINCT FROM EXCLUDED.batch_hash
                    THEN movie_reference_batches.version + 1
                    ELSE movie_reference_batches.version
                END,
                batch_hash = EXCLUDED.batch_hash,
                updated_at = CASE
                    WHEN movie_reference_batches.batch_hash IS DISTINCT FROM EXCLUDED.batch_hash
                    THEN NOW()
                    ELSE movie_reference_batches.updated_at
                END
            "#,
            lib_id.to_uuid(),
            id.as_i64(),
            batch_size as i32,
            Some(hash_bigint),
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?;

        let rows_affected = row.rows_affected();
        if rows_affected == 1 {
            Ok(())
        } else {
            error!(
                "Affected {} rows with mark_series_finalized (not the correct 1 row modified)",
                rows_affected
            );
            Ok(())
        }
    }

    async fn get_all_movie_references(&self) -> Result<Vec<MovieReference>> {
        let repository = TmdbMetadataRepository::new(&self.pool);

        let rows = sqlx::query_as!(
            MovieReferenceRow,
            r#"
            SELECT
                mr.id,
                mr.tmdb_id,
                mr.title,
                mr.theme_color,
                mr.batch_id,
                mf.id AS file_id,
                mf.file_path,
                mf.filename,
                mf.file_size,
                mf.discovered_at AS file_discovered_at,
                mf.created_at AS file_created_at,
                mf.technical_metadata,
                mf.library_id
            FROM movie_references mr
            JOIN media_files mf ON mr.file_id = mf.id
            ORDER BY mr.title
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?;

        repository.load_movie_references_bulk(rows).await
    }

    async fn get_movie_references_by_batch(
        &self,
        library_id: &LibraryId,
        batch_id: crate::types::ids::MovieBatchId,
    ) -> Result<Vec<MovieReference>> {
        let repository = TmdbMetadataRepository::new(&self.pool);

        let rows = sqlx::query_as!(
            MovieReferenceRow,
            r#"
            SELECT
                mr.id,
                mr.tmdb_id,
                mr.title,
                mr.theme_color,
                mr.batch_id,
                mf.id AS file_id,
                mf.library_id,
                mf.file_path,
                mf.filename,
                mf.file_size,
                mf.discovered_at AS file_discovered_at,
                mf.created_at AS file_created_at,
                mf.technical_metadata
            FROM movie_references mr
            JOIN media_files mf ON mr.file_id = mf.id
            WHERE mr.library_id = $1
              AND mr.batch_id = $2
            ORDER BY mr.id
            "#,
            library_id.as_uuid(),
            batch_id.as_u32() as i64,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Database query failed for movie batch fetch: {}",
                e
            ))
        })?;

        repository
            .load_movie_references_for_batch(library_id, batch_id, rows)
            .await
    }

    async fn get_movie_references_for_batches(
        &self,
        library_id: &LibraryId,
        batch_ids: &[crate::types::ids::MovieBatchId],
    ) -> Result<Vec<MovieReference>> {
        if batch_ids.is_empty() {
            return Ok(Vec::new());
        }

        let repository = TmdbMetadataRepository::new(&self.pool);

        let batch_ids: Vec<i64> =
            batch_ids.iter().map(|id| id.as_i64()).collect();

        let rows = sqlx::query_as!(
            MovieReferenceRow,
            r#"
            SELECT
                mr.id,
                mr.tmdb_id,
                mr.title,
                mr.theme_color,
                mr.batch_id,
                mf.id AS file_id,
                mf.library_id,
                mf.file_path,
                mf.filename,
                mf.file_size,
                mf.discovered_at AS file_discovered_at,
                mf.created_at AS file_created_at,
                mf.technical_metadata
            FROM movie_references mr
            JOIN media_files mf ON mr.file_id = mf.id
            WHERE mr.library_id = $1
              AND mr.batch_id = ANY($2)
            ORDER BY mr.batch_id, mr.id
            "#,
            library_id.as_uuid(),
            &batch_ids,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Database query failed for multi-batch movie fetch: {}",
                e
            ))
        })?;

        repository.load_movie_references_bulk(rows).await
    }

    async fn list_finalized_movie_reference_batches(
        &self,
        library_id: &LibraryId,
    ) -> Result<Vec<crate::types::ids::MovieBatchId>> {
        let rows = sqlx::query!(
            r#"
            SELECT batch_id
            FROM movie_reference_batches
            WHERE library_id = $1
              AND finalized_at IS NOT NULL
            ORDER BY batch_id
            "#,
            library_id.as_uuid()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Database query failed for batch list: {}",
                e
            ))
        })?;

        let mut batch_ids = Vec::with_capacity(rows.len());
        for row in rows {
            let batch_id =
                crate::types::ids::MovieBatchId::new(row.batch_id as u32)
                    .map_err(|e| {
                        MediaError::Internal(format!(
                            "Invalid batch id {} in database: {}",
                            row.batch_id, e
                        ))
                    })?;
            batch_ids.push(batch_id);
        }

        Ok(batch_ids)
    }

    async fn list_finalized_movie_batch_versions(
        &self,
        library_id: &LibraryId,
    ) -> Result<Vec<MovieBatchVersionRecord>> {
        let rows = sqlx::query!(
            r#"
            SELECT batch_id, version
            FROM movie_reference_batches
            WHERE library_id = $1
              AND finalized_at IS NOT NULL
            ORDER BY batch_id
            "#,
            library_id.as_uuid()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Database query failed for batch version list: {}",
                e
            ))
        })?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let batch_id =
                crate::types::ids::MovieBatchId::new(row.batch_id as u32)
                    .map_err(|e| {
                        MediaError::Internal(format!(
                            "Invalid batch id {} in database: {}",
                            row.batch_id, e
                        ))
                    })?;

            let version = u64::try_from(row.version).map_err(|_| {
                MediaError::Internal(format!(
                    "Invalid batch version {} for library {} batch {}",
                    row.version, library_id, batch_id
                ))
            })?;

            out.push(MovieBatchVersionRecord { batch_id, version });
        }

        Ok(out)
    }

    async fn get_unfinalized_movie_reference_batch_id(
        &self,
        library_id: &LibraryId,
    ) -> Result<Option<crate::types::ids::MovieBatchId>> {
        let row = sqlx::query!(
            r#"
            SELECT batch_id
            FROM movie_reference_batches
            WHERE library_id = $1
              AND finalized_at IS NULL
            ORDER BY batch_id DESC
            LIMIT 1
            "#,
            library_id.as_uuid()
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Database query failed for unfinalized batch lookup: {}",
                e
            ))
        })?;

        let Some(row) = row else {
            return Ok(None);
        };

        let batch_id = crate::types::ids::MovieBatchId::new(
            row.batch_id as u32,
        )
        .map_err(|e| {
            MediaError::Internal(format!(
                "Invalid batch id {} in database: {}",
                row.batch_id, e
            ))
        })?;

        Ok(Some(batch_id))
    }

    async fn get_movie_batch_hash(
        &self,
        library_id: &LibraryId,
        batch_id: crate::types::ids::MovieBatchId,
    ) -> Result<Option<u64>> {
        let row = sqlx::query!(
            r#"
            SELECT batch_hash
            FROM movie_reference_batches
            WHERE library_id = $1
              AND batch_id = $2
            "#,
            library_id.as_uuid(),
            batch_id.as_i64(),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Database query failed for movie batch hash lookup: {}",
                e
            ))
        })?;

        let Some(row) = row else {
            return Ok(None);
        };

        let Some(hash) = row.batch_hash else {
            return Ok(None);
        };

        let (bigint, exponent) = hash.as_bigint_and_exponent();
        if exponent != 0 {
            return Err(MediaError::Internal(format!(
                "Invalid scaled batch hash {} (exponent {}) for library {} batch {}",
                hash, exponent, library_id, batch_id
            )));
        }

        let Some(biguint) = bigint.to_biguint() else {
            return Err(MediaError::Internal(format!(
                "Invalid negative batch hash {} for library {} batch {}",
                hash, library_id, batch_id
            )));
        };

        let digits = biguint.to_u64_digits();
        let hash = match digits.as_slice() {
            [] => 0,
            [value] => *value,
            _multi_word => {
                return Err(MediaError::Internal(format!(
                    "Movie batch hash out of u64 range for library {} batch {}",
                    library_id, batch_id
                )));
            }
        };

        Ok(Some(hash))
    }

    async fn list_movie_reference_batches_with_movies(
        &self,
        library_id: &LibraryId,
    ) -> Result<Vec<crate::types::ids::MovieBatchId>> {
        let rows = sqlx::query!(
            r#"
            SELECT DISTINCT mr.batch_id
            FROM movie_references mr
            WHERE mr.library_id = $1
            ORDER BY mr.batch_id
            "#,
            library_id.as_uuid()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Database query failed for batch list (with movies): {}",
                e
            ))
        })?;

        let mut batch_ids = Vec::with_capacity(rows.len());
        for row in rows {
            let batch_id =
                crate::types::ids::MovieBatchId::new(row.batch_id as u32)
                    .map_err(|e| {
                        MediaError::Internal(format!(
                            "Invalid batch id {} in database: {}",
                            row.batch_id, e
                        ))
                    })?;
            batch_ids.push(batch_id);
        }

        Ok(batch_ids)
    }

    async fn list_movie_batch_versions_with_movies(
        &self,
        library_id: &LibraryId,
    ) -> Result<Vec<MovieBatchVersionRecord>> {
        let rows = sqlx::query!(
            r#"
            SELECT b.batch_id, b.version
            FROM movie_reference_batches b
            WHERE b.library_id = $1
              AND EXISTS (
                SELECT 1
                FROM movie_references mr
                WHERE mr.library_id = $1
                  AND mr.batch_id = b.batch_id
              )
            ORDER BY b.batch_id
            "#,
            library_id.as_uuid()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Database query failed for batch version list (with movies): {}",
                e
            ))
        })?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let batch_id =
                crate::types::ids::MovieBatchId::new(row.batch_id as u32)
                    .map_err(|e| {
                        MediaError::Internal(format!(
                            "Invalid batch id {} in database: {}",
                            row.batch_id, e
                        ))
                    })?;

            let version = u64::try_from(row.version).map_err(|_| {
                MediaError::Internal(format!(
                    "Invalid batch version {} for library {} batch {}",
                    row.version, library_id, batch_id
                ))
            })?;

            out.push(MovieBatchVersionRecord { batch_id, version });
        }

        Ok(out)
    }

    async fn list_movie_batch_manifest_with_movies(
        &self,
        library_id: &LibraryId,
    ) -> Result<Vec<MovieBatchManifestRecord>> {
        let rows = sqlx::query!(
            r#"
            SELECT b.batch_id, b.version, b.batch_hash
            FROM movie_reference_batches b
            WHERE b.library_id = $1
              AND EXISTS (
                SELECT 1
                FROM movie_references mr
                WHERE mr.library_id = $1
                  AND mr.batch_id = b.batch_id
              )
            ORDER BY b.batch_id
            "#,
            library_id.as_uuid()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Database query failed for movie batch manifest list (with movies): {}",
                e
            ))
        })?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let batch_id =
                crate::types::ids::MovieBatchId::new(row.batch_id as u32)
                    .map_err(|e| {
                        MediaError::Internal(format!(
                            "Invalid batch id {} in database: {}",
                            row.batch_id, e
                        ))
                    })?;

            let version = u64::try_from(row.version).map_err(|_| {
                MediaError::Internal(format!(
                    "Invalid batch version {} for library {} batch {}",
                    row.version, library_id, batch_id
                ))
            })?;

            let content_hash = match row.batch_hash {
                None => None,
                Some(hash) => {
                    let (bigint, exponent) = hash.as_bigint_and_exponent();
                    if exponent != 0 {
                        return Err(MediaError::Internal(format!(
                            "Invalid scaled batch hash {} (exponent {}) for library {} batch {}",
                            hash, exponent, library_id, batch_id
                        )));
                    }

                    let Some(biguint) = bigint.to_biguint() else {
                        return Err(MediaError::Internal(format!(
                            "Invalid negative batch hash {} for library {} batch {}",
                            hash, library_id, batch_id
                        )));
                    };

                    let digits = biguint.to_u64_digits();
                    match digits.as_slice() {
                        [] => None,
                        [value] => Some(*value),
                        _multi_word => {
                            return Err(MediaError::Internal(format!(
                                "Movie batch hash out of u64 range for library {} batch {}",
                                library_id, batch_id
                            )));
                        }
                    }
                }
            };

            out.push(MovieBatchManifestRecord {
                batch_id,
                version,
                content_hash,
            });
        }

        Ok(out)
    }

    async fn list_movie_reference_batches(
        &self,
        library_id: &LibraryId,
    ) -> Result<Vec<crate::types::ids::MovieBatchId>> {
        let rows = sqlx::query!(
            r#"
            SELECT batch_id
            FROM movie_reference_batches
            WHERE library_id = $1
            ORDER BY batch_id
            "#,
            library_id.as_uuid()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Database query failed for batch list: {}",
                e
            ))
        })?;

        let mut batch_ids = Vec::with_capacity(rows.len());
        for row in rows {
            let batch_id =
                crate::types::ids::MovieBatchId::new(row.batch_id as u32)
                    .map_err(|e| {
                        MediaError::Internal(format!(
                            "Invalid batch id {} in database: {}",
                            row.batch_id, e
                        ))
                    })?;
            batch_ids.push(batch_id);
        }

        Ok(batch_ids)
    }

    async fn get_series(&self) -> Result<Vec<Series>> {
        // TODO: Implement series references fetching
        Ok(vec![])
    }

    async fn get_series_seasons(
        &self,
        series_id: &SeriesID,
    ) -> Result<Vec<SeasonReference>> {
        let series_uuid = series_id.to_uuid();

        info!("Getting seasons for series: {}", series_uuid);

        let rows = sqlx::query_as!(
            SeasonReferenceRow,
            r#"
            SELECT
                id,
                series_id,
                season_number,
                library_id,
                tmdb_series_id,
                discovered_at AS "discovered_at!",
                created_at AS "created_at!",
                theme_color
            FROM season_references
            WHERE series_id = $1
            ORDER BY season_number
            "#,
            series_uuid
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to get series seasons: {}", e))
        })?;

        info!(
            "Found {} season rows for series {}",
            rows.len(),
            series_uuid
        );

        let repository = TmdbMetadataRepository::new(&self.pool);
        repository.load_season_references_bulk(rows).await
    }

    async fn get_season_episodes(
        &self,
        season_id: &SeasonID,
    ) -> Result<Vec<EpisodeReference>> {
        let repository = TmdbMetadataRepository::new(&self.pool);

        let rows = sqlx::query_as!(
            EpisodeReferenceRow,
            r#"
            SELECT
                er.id,
                er.episode_number,
                er.season_number,
                er.season_id,
                er.series_id,
                er.tmdb_series_id,
                er.discovered_at AS episode_discovered_at,
                er.created_at AS episode_created_at,
                mf.id AS file_id,
                mf.library_id,
                mf.file_path,
                mf.filename,
                mf.file_size,
                mf.discovered_at AS file_discovered_at,
                mf.created_at AS file_created_at,
                mf.technical_metadata
            FROM episode_references er
            JOIN media_files mf ON er.file_id = mf.id
            WHERE er.season_id = $1
            ORDER BY er.episode_number
            "#,
            season_id.to_uuid()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to get season episodes: {}",
                e
            ))
        })?;

        repository.load_episode_references_bulk(rows).await
    }

    async fn get_series_by_tmdb_id(
        &self,
        library_id: LibraryId,
        tmdb_id: u64,
    ) -> Result<Option<Series>> {
        let repository = TmdbMetadataRepository::new(&self.pool);

        let row = sqlx::query_as!(
            SeriesReferenceRow,
            r#"
            SELECT
                sr.id,
                sr.library_id,
                sr.tmdb_id,
                sr.title,
                sr.theme_color,
                sr.discovered_at AS "discovered_at!",
                sr.created_at AS "created_at!"
            FROM series sr
            WHERE sr.library_id = $1
              AND sr.tmdb_id = $2
            "#,
            library_id.as_uuid(),
            tmdb_id as i64
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?;

        match row {
            Some(row) => {
                let series = repository.load_series_reference(row).await?;
                Ok(Some(series))
            }
            None => Ok(None),
        }
    }

    async fn update_movie_tmdb_id(
        &self,
        id: &MovieID,
        tmdb_id: u64,
    ) -> Result<()> {
        let movie_uuid = id.to_uuid();

        sqlx::query!(
            "UPDATE movie_references SET tmdb_id = $1, updated_at = NOW() WHERE id = $2",
            tmdb_id as i64,
            movie_uuid
        )
            .execute(&self.pool)
            .await
            .map_err(|e| MediaError::Internal(format!("Update failed: {}", e)))?;

        Ok(())
    }

    async fn update_series_tmdb_id(
        &self,
        id: &SeriesID,
        tmdb_id: u64,
    ) -> Result<()> {
        let series_uuid = id.to_uuid();

        sqlx::query!(
            "UPDATE series SET tmdb_id = $1, updated_at = NOW() WHERE id = $2",
            tmdb_id as i64,
            series_uuid
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Update failed: {}", e)))?;

        Ok(())
    }

    async fn cleanup_orphan_tv_references(
        &self,
        library_id: LibraryId,
    ) -> Result<TvReferenceOrphanCleanup> {
        let deleted_seasons = sqlx::query(
            r#"
            DELETE FROM season_references
            WHERE library_id = $1
              AND NOT EXISTS (
                SELECT 1
                FROM episode_references er
                WHERE er.season_id = season_references.id
              )
            "#,
        )
        .bind(library_id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to cleanup orphan season references for library {}: {}",
                library_id, e
            ))
        })?
        .rows_affected();

        let deleted_series = sqlx::query(
            r#"
            DELETE FROM series
            WHERE library_id = $1
              AND NOT EXISTS (
                SELECT 1
                FROM season_references sr
                WHERE sr.series_id = series.id
              )
              AND NOT EXISTS (
                SELECT 1
                FROM episode_references er
                WHERE er.series_id = series.id
              )
            "#,
        )
        .bind(library_id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to cleanup orphan series references for library {}: {}",
                library_id, e
            ))
        })?
        .rows_affected();

        Ok(TvReferenceOrphanCleanup {
            deleted_seasons,
            deleted_series,
        })
    }
}
