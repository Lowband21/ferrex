use crate::{
    database::{
        ports::media_references::MediaReferencesRepository,
        postgres_ext::TmdbMetadataRepository,
    },
    error::{MediaError, Result},
    player_prelude::{
        EnhancedMovieDetails, MediaDetailsOption, MediaFile, TmdbDetails,
    },
    types::{
        ids::{EpisodeID, LibraryID, MovieID, SeasonID, SeriesID},
        library::LibraryType,
        media::{
            EpisodeReference, Media, MovieReference, SeasonReference,
            SeriesReference,
        },
        titles::{MovieTitle, SeriesTitle},
        urls::{MovieURL, SeriesURL, UrlLike},
    },
};

use async_trait::async_trait;
use rayon::iter::{IntoParallelIterator, ParallelExtend, ParallelIterator};
use sqlx::{PgPool, Row, types::Uuid};
use std::path::PathBuf;
use tracing::info;

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
    pub async fn get_movie_with_metadata(
        &self,
        id: &MovieID,
        include_metadata: bool,
    ) -> Result<Option<(MovieReference, Option<EnhancedMovieDetails>)>> {
        let movie_uuid = id.to_uuid();

        let row = sqlx::query(
            r#"
            SELECT
                mr.id,
                mr.tmdb_id,
                mr.title,
                mr.theme_color,
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
        )
        .bind(movie_uuid)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?;

        let Some(row) = row else {
            return Ok(None);
        };

        if include_metadata {
            let repository = TmdbMetadataRepository::new(&self.pool);
            let movie_ref = repository.load_movie_reference(row).await?;
            let metadata = match &movie_ref.details {
                MediaDetailsOption::Details(details) => {
                    match details.as_ref() {
                        TmdbDetails::Movie(details) => Some(details.clone()),
                        _ => None,
                    }
                }
                _ => None,
            };

            Ok(Some((movie_ref, metadata)))
        } else {
            let library_id = LibraryID(row.try_get("library_id")?);

            let technical_metadata: Option<serde_json::Value> =
                row.try_get("technical_metadata").ok();
            let media_file_metadata = technical_metadata
                .map(serde_json::from_value)
                .transpose()
                .map_err(|e| {
                    MediaError::Internal(format!(
                        "Failed to deserialize metadata: {}",
                        e
                    ))
                })?;

            let media_file = MediaFile {
                id: row.try_get("file_id")?,
                path: PathBuf::from(row.try_get::<String, _>("file_path")?),
                filename: row.try_get("filename")?,
                size: row.try_get::<i64, _>("file_size")? as u64,
                discovered_at: row.try_get("file_discovered_at")?,
                created_at: row.try_get("file_created_at")?,
                media_file_metadata,
                library_id,
            };

            let tmdb_id: i64 = row.try_get("tmdb_id")?;
            let title: String = row.try_get("title")?;
            let movie_id: Uuid = row.try_get("id")?;
            let file_id: Uuid = row.try_get("file_id")?;
            let theme_color: Option<String> = row.try_get("theme_color")?;

            let movie_ref = MovieReference {
                id: MovieID(movie_id),
                library_id,
                tmdb_id: tmdb_id as u64,
                title: MovieTitle::new(title.clone()).map_err(|e| {
                    MediaError::Internal(format!(
                        "Invalid stored movie title '{}': {}",
                        title, e
                    ))
                })?,
                details: MediaDetailsOption::Endpoint(format!(
                    "/movie/{}",
                    movie_id
                )),
                endpoint: MovieURL::from_string(format!("/stream/{}", file_id)),
                file: media_file,
                theme_color,
            };

            Ok(Some((movie_ref, None)))
        }
    }
}

#[async_trait]
impl MediaReferencesRepository for PostgresMediaReferencesRepository {
    async fn get_library_media_references(
        &self,
        library_id: LibraryID,
        library_type: LibraryType,
    ) -> Result<Vec<Media>> {
        let mut media = Vec::new();
        match library_type {
            LibraryType::Movies => {
                let repository = TmdbMetadataRepository::new(&self.pool);
                let rows = sqlx::query(
                    r#"
                    SELECT
                        mr.id,
                        mr.tmdb_id,
                        mr.title,
                        mr.theme_color,
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
                )
                .bind(library_id.as_uuid())
                .fetch_all(&self.pool)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!(
                        "Database query failed: {}",
                        e
                    ))
                })?;

                for row in rows {
                    let movie = repository.load_movie_reference(row).await?;
                    media.push(Media::Movie(movie));
                }
            }
            LibraryType::Series => {
                // Execute bulk queries in parallel using tokio::join!
                let (series_result, seasons_result, episodes_result) = tokio::join!(
                    self.get_library_series(&library_id),
                    self.get_library_seasons(&library_id),
                    self.get_library_episodes(&library_id)
                );
                if let Ok(series) = series_result {
                    media.par_extend(series.into_par_iter().map(Media::Series));
                }
                if let Ok(seasons) = seasons_result {
                    media
                        .par_extend(seasons.into_par_iter().map(Media::Season));
                }
                if let Ok(episodes) = episodes_result {
                    media.par_extend(
                        episodes.into_par_iter().map(Media::Episode),
                    );
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
        let row = sqlx::query(
            r#"
            SELECT
                mr.id,
                mr.tmdb_id,
                mr.title,
                mr.theme_color,
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
        )
        .bind(path)
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

        let rows = sqlx::query(
            r#"
            SELECT
                mr.id,
                mr.tmdb_id,
                mr.title,
                mr.theme_color,
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
        )
        .bind(uuids.as_slice())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?;

        let mut movies = Vec::with_capacity(rows.len());
        for row in rows {
            let movie = repository.load_movie_reference(row).await?;
            movies.push(movie);
        }

        Ok(movies)
    }

    async fn get_series_references_bulk(
        &self,
        ids: &[&SeriesID],
    ) -> Result<Vec<SeriesReference>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        // Convert IDs to UUIDs
        let uuids: Vec<Uuid> = ids.iter().map(|id| id.to_uuid()).collect();

        let repository = TmdbMetadataRepository::new(&self.pool);

        let rows = sqlx::query(
            r#"
            SELECT
                sr.id,
                sr.library_id,
                sr.tmdb_id,
                sr.title,
                sr.theme_color,
                sr.discovered_at,
                sr.created_at
            FROM series_references sr
            WHERE sr.id = ANY($1)
            "#,
        )
        .bind(uuids.as_slice())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?;

        let mut series_list = Vec::with_capacity(rows.len());
        for row in rows {
            let series = repository.load_series_reference(row).await?;
            series_list.push(series);
        }

        Ok(series_list)
    }

    async fn get_library_series(
        &self,
        library_id: &LibraryID,
    ) -> Result<Vec<SeriesReference>> {
        let repository = TmdbMetadataRepository::new(&self.pool);

        let rows = sqlx::query(
            r#"
            SELECT
                sr.id,
                sr.library_id,
                sr.tmdb_id,
                sr.title,
                sr.theme_color,
                sr.discovered_at,
                sr.created_at
            FROM series_references sr
            WHERE sr.library_id = $1
            ORDER BY sr.title
            "#,
        )
        .bind(library_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Database query failed: {}", e))
        })?;

        let mut series_list = Vec::with_capacity(rows.len());
        for row in rows {
            let series = repository.load_series_reference(row).await?;
            series_list.push(series);
        }

        Ok(series_list)
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

        let rows = sqlx::query(
            r#"
            SELECT
                sr.id,
                sr.series_id,
                sr.season_number,
                sr.library_id,
                sr.tmdb_series_id,
                sr.discovered_at,
                sr.created_at,
                sr.theme_color
            FROM season_references sr
            WHERE sr.id = ANY($1)
            "#,
        )
        .bind(uuids.as_slice())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to get seasons: {}", e))
        })?;

        let mut seasons = Vec::with_capacity(rows.len());
        for row in rows {
            let season = repository.load_season_reference(row).await?;
            seasons.push(season);
        }

        Ok(seasons)
    }

    async fn get_library_seasons(
        &self,
        library_id: &LibraryID,
    ) -> Result<Vec<SeasonReference>> {
        let repository = TmdbMetadataRepository::new(&self.pool);

        let rows = sqlx::query(
            r#"
            SELECT
                sr.id,
                sr.series_id,
                sr.season_number,
                sr.library_id,
                sr.tmdb_series_id,
                sr.discovered_at,
                sr.created_at,
                sr.theme_color
            FROM season_references sr
            WHERE sr.library_id = $1
            ORDER BY sr.series_id, sr.season_number
            "#,
        )
        .bind(library_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to get seasons: {}", e))
        })?;

        let mut seasons = Vec::with_capacity(rows.len());
        for row in rows {
            let season = repository.load_season_reference(row).await?;
            seasons.push(season);
        }

        Ok(seasons)
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

        let rows = sqlx::query(
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
        )
        .bind(uuids.as_slice())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to get episodes: {}", e))
        })?;

        let mut episodes = Vec::with_capacity(rows.len());
        for row in rows {
            let episode = repository.load_episode_reference(row).await?;
            episodes.push(episode);
        }

        Ok(episodes)
    }

    async fn get_library_episodes(
        &self,
        library_id: &LibraryID,
    ) -> Result<Vec<EpisodeReference>> {
        let repository = TmdbMetadataRepository::new(&self.pool);

        let rows = sqlx::query(
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
            WHERE mf.library_id = $1
            ORDER BY er.series_id ASC, er.season_number ASC, er.episode_number ASC
            "#,
        )
        .bind(library_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get episodes: {}", e)))?;

        let mut episodes = Vec::with_capacity(rows.len());
        for row in rows {
            let episode = repository.load_episode_reference(row).await?;
            episodes.push(episode);
        }

        Ok(episodes)
    }
    async fn store_movie_reference(
        &self,
        movie: &MovieReference,
    ) -> Result<()> {
        TmdbMetadataRepository::new(&self.pool)
            .store_movie_reference(movie)
            .await
    }

    async fn store_series_reference(
        &self,
        series: &SeriesReference,
    ) -> Result<()> {
        TmdbMetadataRepository::new(&self.pool)
            .store_series_reference(series)
            .await
    }

    async fn store_season_reference(
        &self,
        season: &SeasonReference,
    ) -> Result<Uuid> {
        TmdbMetadataRepository::new(&self.pool)
            .store_season_reference(season)
            .await
    }

    async fn store_episode_reference(
        &self,
        episode: &EpisodeReference,
    ) -> Result<()> {
        TmdbMetadataRepository::new(&self.pool)
            .store_episode_reference(episode)
            .await
    }

    async fn get_movie_reference(
        &self,
        id: &MovieID,
    ) -> Result<MovieReference> {
        // Include full metadata when fetching individual movie references
        // This is used by the /media endpoint to provide complete data
        match self.get_movie_with_metadata(id, true).await? {
            Some((movie_ref, _)) => Ok(movie_ref),
            None => Err(MediaError::NotFound("Movie not found".to_string())),
        }
    }

    async fn get_series_reference(
        &self,
        id: &SeriesID,
    ) -> Result<SeriesReference> {
        let series_uuid = id.to_uuid();

        let row = sqlx::query(
            r#"
            SELECT id, library_id, tmdb_id, title, theme_color, discovered_at, created_at
            FROM series_references
            WHERE id = $1
            "#,
        )
        .bind(series_uuid)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?
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

        let row = sqlx::query(
            r#"
            SELECT
                id,
                series_id,
                season_number,
                library_id,
                tmdb_series_id,
                discovered_at,
                created_at,
                theme_color
            FROM season_references
            WHERE id = $1
            "#,
        )
        .bind(season_uuid)
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

        let row = sqlx::query(
            r#"
            SELECT
                er.id,
                er.episode_number,
                er.season_number,
                er.season_id,
                er.series_id,
                er.tmdb_series_id,
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
        )
        .bind(episode_uuid)
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

    async fn get_all_movie_references(&self) -> Result<Vec<MovieReference>> {
        let repository = TmdbMetadataRepository::new(&self.pool);

        let rows = sqlx::query(
            r#"
            SELECT
                mr.id,
                mr.tmdb_id,
                mr.title,
                mr.theme_color,
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

        let mut movies = Vec::with_capacity(rows.len());
        for row in rows {
            let movie_ref = repository.load_movie_reference(row).await?;
            movies.push(movie_ref);
        }

        Ok(movies)
    }

    async fn get_series_references(&self) -> Result<Vec<SeriesReference>> {
        // TODO: Implement series references fetching
        Ok(vec![])
    }

    async fn get_series_seasons(
        &self,
        series_id: &SeriesID,
    ) -> Result<Vec<SeasonReference>> {
        let series_uuid = series_id.to_uuid();

        info!("Getting seasons for series: {}", series_uuid);

        let rows = sqlx::query(
            r#"
            SELECT
                id,
                series_id,
                season_number,
                library_id,
                tmdb_series_id,
                discovered_at,
                created_at,
                theme_color
            FROM season_references
            WHERE series_id = $1
            ORDER BY season_number
            "#,
        )
        .bind(series_uuid)
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
        let mut seasons = Vec::with_capacity(rows.len());

        for row in rows {
            let season = repository.load_season_reference(row).await?;
            seasons.push(season);
        }

        Ok(seasons)
    }

    async fn get_season_episodes(
        &self,
        season_id: &SeasonID,
    ) -> Result<Vec<EpisodeReference>> {
        let repository = TmdbMetadataRepository::new(&self.pool);

        let rows = sqlx::query(
            r#"
            SELECT
                er.id,
                er.episode_number,
                er.season_number,
                er.season_id,
                er.series_id,
                er.tmdb_series_id,
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
        )
        .bind(season_id.to_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to get season episodes: {}",
                e
            ))
        })?;

        let mut episodes = Vec::with_capacity(rows.len());
        for row in rows {
            let episode = repository.load_episode_reference(row).await?;
            episodes.push(episode);
        }

        Ok(episodes)
    }

    async fn get_series_by_tmdb_id(
        &self,
        library_id: LibraryID,
        tmdb_id: u64,
    ) -> Result<Option<SeriesReference>> {
        let repository = TmdbMetadataRepository::new(&self.pool);

        let row = sqlx::query(
            r#"
            SELECT id, library_id, tmdb_id, title, theme_color, discovered_at, created_at
            FROM series_references
            WHERE library_id = $1 AND tmdb_id = $2
            "#,
        )
        .bind(library_id.as_uuid())
        .bind(tmdb_id as i64)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        match row {
            Some(row) => {
                let series = repository.load_series_reference(row).await?;
                Ok(Some(series))
            }
            None => Ok(None),
        }
    }

    async fn find_series_by_name(
        &self,
        library_id: LibraryID,
        name: &str,
    ) -> Result<Option<SeriesReference>> {
        // Normalize an input "slug" in Rust to avoid repeating logic in many callers.
        // Keep this conservative: only lowercase and collapse non-alphanumerics to dashes.
        let mut slug = String::new();
        for ch in name.trim().chars() {
            if ch.is_ascii_alphanumeric() {
                slug.push(ch.to_ascii_lowercase());
            } else if !slug.ends_with('-') {
                slug.push('-');
            }
        }
        while slug.ends_with('-') {
            slug.pop();
        }

        // Use a three-tiered match strategy to reduce false positives:
        // 1) Exact case-insensitive title match
        // 2) Exact slug match (title normalized to slug on the DB side)
        // 3) Prefix match, then finally a fuzzy ILIKE catch‑all
        let search_pattern = format!("%{}%", name);

        let row = sqlx::query!(
            r#"
            WITH candidates AS (
                SELECT
                    id,
                    library_id,
                    tmdb_id,
                    title,
                    theme_color,
                    discovered_at,
                    created_at,
                    -- Compute a conservative slug server-side for exact matching
                    TRIM(BOTH '-' FROM REGEXP_REPLACE(LOWER(title), '[^a-z0-9]+', '-', 'g')) AS title_slug
                FROM series_references
                WHERE library_id = $1
                  AND (
                        LOWER(title) = LOWER($2)
                     OR TRIM(BOTH '-' FROM REGEXP_REPLACE(LOWER(title), '[^a-z0-9]+', '-', 'g')) = $3
                     OR LOWER(title) LIKE LOWER($2 || '%')
                     OR title ILIKE $4
                  )
            )
            SELECT id, library_id, tmdb_id as "tmdb_id?", title, theme_color, discovered_at, created_at
            FROM candidates
            ORDER BY
                CASE
                    WHEN LOWER(title) = LOWER($2) THEN 0
                    WHEN title_slug = $3 THEN 1
                    WHEN LOWER(title) LIKE LOWER($2 || '%') THEN 2
                    ELSE 3
                END,
                LENGTH(title)
            LIMIT 1
            "#,
            library_id.as_uuid(),
            name,
            slug,
            search_pattern
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        if let Some(row) = row {
            // Handle nullable tmdb_id - use 0 if null (indicates no TMDB match)
            let tmdb_id = row.tmdb_id.unwrap_or(0) as u64;

            Ok(Some(SeriesReference {
                id: SeriesID(row.id),
                library_id: LibraryID(row.library_id),
                tmdb_id,
                title: SeriesTitle::new(row.title)?,
                details: MediaDetailsOption::Endpoint(format!(
                    "/series/{}",
                    row.id
                )),
                endpoint: SeriesURL::from_string(format!("/series/{}", row.id)),
                discovered_at: row.discovered_at,
                created_at: row.created_at.unwrap_or(row.discovered_at),
                theme_color: row.theme_color,
            }))
        } else {
            Ok(None)
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
            "UPDATE series_references SET tmdb_id = $1, updated_at = NOW() WHERE id = $2",
            tmdb_id as i64,
            series_uuid
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Update failed: {}", e)))?;

        Ok(())
    }
}
