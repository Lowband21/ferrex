use super::traits::*;
use crate::{MediaFile, MediaMetadata, MediaError, Result};
use async_trait::async_trait;
use sqlx::{PgPool, postgres::PgPoolOptions, Row};
use tracing::{info, debug, warn};
use uuid::Uuid;
use std::collections::HashMap;
use std::path::PathBuf;
use serde_json;

#[derive(Debug, Clone)]
pub struct PostgresDatabase {
    pool: PgPool,
}

impl PostgresDatabase {
    fn row_to_media_file(&self, r: sqlx::postgres::PgRow) -> MediaFile {
        use sqlx::Row;
        
        let parsed_info = r.try_get::<Option<serde_json::Value>, _>("parsed_info")
            .unwrap_or(None)
            .and_then(|json| serde_json::from_value(json).ok());
        
        // Build external_info if we have external metadata
        let external_info = if r.try_get::<Option<String>, _>("external_id").unwrap_or(None).is_some() {
            Some(crate::ExternalMediaInfo {
                tmdb_id: r.try_get::<Option<String>, _>("external_id")
                    .unwrap_or(None)
                    .and_then(|id| id.parse::<u32>().ok()),
                tvdb_id: None,
                imdb_id: None,
                description: r.try_get("overview").ok(),
                poster_url: r.try_get("poster_path").ok(),
                backdrop_url: r.try_get("backdrop_path").ok(),
                genres: r.try_get::<Option<serde_json::Value>, _>("genres")
                    .unwrap_or(None)
                    .and_then(|g| serde_json::from_value::<Vec<String>>(g).ok())
                    .unwrap_or_default(),
                rating: r.try_get::<Option<sqlx::types::BigDecimal>, _>("vote_average")
                    .unwrap_or(None)
                    .map(|v| v.to_string().parse::<f32>().unwrap_or(0.0)),
                release_date: r.try_get::<Option<chrono::NaiveDate>, _>("release_date")
                    .unwrap_or(None),
                show_description: r.try_get("show_description").ok(),
                show_poster_url: r.try_get("show_poster_path").ok(),
                season_poster_url: r.try_get("season_poster_path").ok(),
                episode_still_url: r.try_get("episode_still_path").ok(),
            })
        } else {
            None
        };
        
        let duration = r.try_get::<Option<f64>, _>("duration_seconds").unwrap_or(None);
        let width = r.try_get::<Option<i32>, _>("width").unwrap_or(None);
        
        let metadata = if duration.is_some() || width.is_some() || parsed_info.is_some() || external_info.is_some() {
            Some(MediaMetadata {
                duration,
                width: width.map(|w| w as u32),
                height: r.try_get::<Option<i32>, _>("height").unwrap_or(None).map(|h| h as u32),
                video_codec: r.try_get("video_codec").ok(),
                audio_codec: r.try_get("audio_codec").ok(),
                bitrate: None,
                framerate: None,
                file_size: r.get::<i64, _>("file_size") as u64,
                parsed_info,
                external_info,
            })
        } else {
            None
        };
        
        MediaFile {
            id: r.get("id"),
            path: std::path::PathBuf::from(r.get::<String, _>("file_path")),
            filename: r.get("file_name"),
            size: r.get::<i64, _>("file_size") as u64,
            created_at: r.get("created_at"),
            metadata,
        }
    }
    pub async fn new(connection_string: &str) -> Result<Self> {
        info!("Connecting to PostgreSQL database");
        
        let pool = PgPoolOptions::new()
            .max_connections(20)
            .connect(connection_string)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to connect to PostgreSQL: {}", e)))?;
        
        info!("Successfully connected to PostgreSQL");
        
        Ok(Self { pool })
    }
    
    async fn run_migrations(pool: &sqlx::PgPool) -> Result<()> {
        info!("Running database migrations");
        
        // Simple approach: just create tables if they don't exist
        // PostgreSQL's CREATE TABLE IF NOT EXISTS handles this gracefully
        
        // Create media_files table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS media_files (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                file_path TEXT NOT NULL UNIQUE,
                file_name TEXT NOT NULL,
                file_size BIGINT NOT NULL,
                media_type TEXT NOT NULL DEFAULT 'unknown',
                parent_directory TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                last_scanned_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#
        )
        .execute(pool)
        .await
        .map_err(|e| MediaError::InvalidMedia(format!("Failed to create media_files table: {}", e)))?;
        
        // Create indexes
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_media_files_path ON media_files(file_path)")
            .execute(pool).await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to create index: {}", e)))?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_media_files_parent_dir ON media_files(parent_directory)")
            .execute(pool).await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to create index: {}", e)))?;
        
        // Create media_metadata table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS media_metadata (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                media_file_id UUID NOT NULL REFERENCES media_files(id) ON DELETE CASCADE,
                duration_seconds DOUBLE PRECISION,
                width INTEGER,
                height INTEGER,
                video_codec TEXT,
                audio_codec TEXT,
                bitrate BIGINT,
                frame_rate DOUBLE PRECISION,
                parsed_info JSONB,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE(media_file_id)
            )
            "#
        )
        .execute(pool)
        .await
        .map_err(|e| MediaError::InvalidMedia(format!("Failed to create table: {}", e)))?;
        
        // Create external_metadata table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS external_metadata (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                media_file_id UUID NOT NULL REFERENCES media_files(id) ON DELETE CASCADE,
                external_id TEXT,
                title TEXT,
                overview TEXT,
                release_date DATE,
                vote_average DECIMAL(3, 1),
                poster_path TEXT,
                backdrop_path TEXT,
                genres JSONB,
                show_description TEXT,
                show_poster_path TEXT,
                season_poster_path TEXT,
                episode_still_path TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE(media_file_id)
            )
            "#
        )
        .execute(pool)
        .await
        .map_err(|e| MediaError::InvalidMedia(format!("Failed to create table: {}", e)))?;
        
        info!("Database migrations completed successfully");
        Ok(())
    }
}

#[async_trait]
impl MediaDatabaseTrait for PostgresDatabase {
    async fn initialize_schema(&self) -> Result<()> {
        Self::run_migrations(&self.pool).await
    }
    
    async fn store_media(&self, media_file: MediaFile) -> Result<String> {
        debug!("Storing media file: {}", media_file.filename);
        
        let media_type = media_file.metadata.as_ref()
            .and_then(|m| m.parsed_info.as_ref())
            .map(|p| match p.media_type {
                crate::MediaType::Movie => "movie",
                crate::MediaType::TvEpisode => "tv_show",
                crate::MediaType::Unknown => "unknown",
            })
            .unwrap_or("unknown")
            .to_string();
        
        let parent_dir = media_file.path.parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());
            
        let id: (Uuid,) = sqlx::query_as(
            r#"
            INSERT INTO media_files (id, file_path, file_name, file_size, media_type, parent_directory)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (file_path) DO UPDATE
            SET file_name = EXCLUDED.file_name,
                file_size = EXCLUDED.file_size,
                updated_at = NOW()
            RETURNING id
            "#
        )
        .bind(media_file.id)
        .bind(media_file.path.to_string_lossy().to_string())
        .bind(&media_file.filename)
        .bind(media_file.size as i64)
        .bind(media_type)
        .bind(parent_dir)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| MediaError::InvalidMedia(format!("Failed to store media: {}", e)))?;
        
        if let Some(metadata) = &media_file.metadata {
            let parsed_info_json = metadata.parsed_info.as_ref()
                .map(|pi| serde_json::to_value(pi).ok())
                .flatten();

            sqlx::query!(
                r#"
                INSERT INTO media_metadata (
                    media_file_id, duration_seconds, width, height,
                    video_codec, audio_codec, bitrate, frame_rate, parsed_info
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                ON CONFLICT (media_file_id) DO UPDATE
                SET duration_seconds = EXCLUDED.duration_seconds,
                    width = EXCLUDED.width,
                    height = EXCLUDED.height,
                    parsed_info = EXCLUDED.parsed_info,
                    updated_at = NOW()
                "#,
                id.0,
                metadata.duration,
                metadata.width.map(|w| w as i32),
                metadata.height.map(|h| h as i32),
                metadata.video_codec.as_deref(),
                metadata.audio_codec.as_deref(),
                metadata.bitrate.map(|b| b as i64),
                metadata.framerate,
                parsed_info_json
            )
            .execute(&self.pool)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to store technical metadata: {}", e)))?;
            
            // Store external metadata if available
            if metadata.external_info.is_some() {
                if let Err(e) = self.store_external_metadata(&id.0.to_string(), metadata).await {
                    warn!("Failed to store external metadata: {}", e);
                }
            }
        }
        
        Ok(id.0.to_string())
    }
    
    async fn get_media_by_path(&self, path: &str) -> Result<Option<MediaFile>> {
        debug!("Retrieving media file by path: {}", path);
        
        let row = sqlx::query(
            r#"
            SELECT 
                mf.id, mf.file_path, mf.file_name, mf.file_size, mf.created_at,
                mm.duration_seconds, mm.width, mm.height, mm.video_codec, mm.audio_codec,
                mm.parsed_info,
                em.external_id, em.title, em.overview, em.release_date,
                em.vote_average, em.poster_path, em.backdrop_path, em.genres,
                em.show_description, em.show_poster_path, em.season_poster_path, em.episode_still_path
            FROM media_files mf
            LEFT JOIN media_metadata mm ON mf.id = mm.media_file_id
            LEFT JOIN external_metadata em ON mf.id = em.media_file_id
            WHERE mf.file_path = $1
            "#
        )
        .bind(path)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::InvalidMedia(format!("Failed to retrieve media by path: {}", e)))?;
        
        match row {
            Some(row) => Ok(Some(self.row_to_media_file(row))),
            None => Ok(None),
        }
    }
    
    async fn get_media(&self, id: &str) -> Result<Option<MediaFile>> {
        debug!("Retrieving media file: {}", id);
        
        // Strip "media:" prefix if present (for compatibility with SurrealDB format)
        let uuid_str = if id.starts_with("media:") {
            id.strip_prefix("media:").unwrap_or(id)
        } else {
            id
        };
        
        let uuid = Uuid::parse_str(uuid_str)
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid UUID: {}", e)))?;
        
        let row = sqlx::query(
            r#"
            SELECT 
                mf.id, mf.file_path, mf.file_name, mf.file_size, mf.created_at,
                mm.duration_seconds, mm.width, mm.height, mm.video_codec, mm.audio_codec,
                mm.parsed_info,
                em.external_id, em.title, em.overview, em.release_date,
                em.vote_average, em.poster_path, em.backdrop_path, em.genres,
                em.show_description, em.show_poster_path, em.season_poster_path, em.episode_still_path
            FROM media_files mf
            LEFT JOIN media_metadata mm ON mf.id = mm.media_file_id
            LEFT JOIN external_metadata em ON mf.id = em.media_file_id
            WHERE mf.id = $1
            "#
        )
        .bind(uuid)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::InvalidMedia(format!("Failed to retrieve media: {}", e)))?;
        
        Ok(row.map(|r| self.row_to_media_file(r)))
    }
    
    async fn list_media(&self, filters: MediaFilters) -> Result<Vec<MediaFile>> {
        debug!("Listing media files with filters: {:?}", filters);
        
        let mut query = String::from(
            r#"
            SELECT 
                mf.id, mf.file_path, mf.file_name, mf.file_size, mf.created_at, mf.media_type,
                mm.duration_seconds, mm.width, mm.height, mm.video_codec, mm.audio_codec,
                mm.parsed_info,
                em.external_id, em.title, em.overview, em.release_date,
                em.vote_average, em.poster_path, em.backdrop_path, em.genres,
                em.show_description, em.show_poster_path, em.season_poster_path, em.episode_still_path
            FROM media_files mf
            LEFT JOIN media_metadata mm ON mf.id = mm.media_file_id
            LEFT JOIN external_metadata em ON mf.id = em.media_file_id
            WHERE 1=1
            "#
        );
        
        let mut param_count = 0;
        
        if filters.media_type.is_some() {
            param_count += 1;
            query.push_str(&format!(" AND mf.media_type = ${}", param_count));
        }
        
        if filters.show_name.is_some() {
            param_count += 1;
            query.push_str(&format!(" AND mm.parsed_info->>'show_name' = ${}", param_count));
        }
        
        if filters.season.is_some() {
            param_count += 1;
            query.push_str(&format!(" AND (mm.parsed_info->>'season')::int = ${}", param_count));
        }
        
        match filters.order_by.as_deref() {
            Some("name") => query.push_str(" ORDER BY mf.file_name ASC"),
            Some("date") => query.push_str(" ORDER BY mf.created_at DESC"),
            Some("size") => query.push_str(" ORDER BY mf.file_size DESC"),
            _ => query.push_str(" ORDER BY mf.created_at DESC"),
        }
        
        if let Some(limit) = filters.limit {
            query.push_str(&format!(" LIMIT {}", limit));
        }
        
        let mut query_builder = sqlx::query(&query);
        
        if let Some(media_type) = &filters.media_type {
            query_builder = query_builder.bind(media_type);
        }
        
        if let Some(show_name) = &filters.show_name {
            query_builder = query_builder.bind(show_name);
        }
        
        if let Some(season) = &filters.season {
            query_builder = query_builder.bind(*season as i32);
        }
        
        let rows = query_builder
            .fetch_all(&self.pool)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to list media: {}", e)))?;
        
        let mut media_files = Vec::new();
        for row in rows {
            // Try to get parsed_info regardless of whether other metadata exists
            let parsed_info = row.try_get::<Option<serde_json::Value>, _>("parsed_info")
                .unwrap_or(None)
                .and_then(|json| serde_json::from_value(json).ok());
            
            // Build external_info if we have external metadata
            let external_info = if row.try_get::<Option<String>, _>("external_id").unwrap_or(None).is_some() {
                Some(crate::ExternalMediaInfo {
                    tmdb_id: row.try_get::<Option<String>, _>("external_id")
                        .unwrap_or(None)
                        .and_then(|id| id.parse::<u32>().ok()),
                    tvdb_id: None,
                    imdb_id: None,
                    description: row.try_get("overview").ok(),
                    poster_url: row.try_get("poster_path").ok(),
                    backdrop_url: row.try_get("backdrop_path").ok(),
                    genres: row.try_get::<Option<serde_json::Value>, _>("genres")
                        .unwrap_or(None)
                        .and_then(|g| serde_json::from_value::<Vec<String>>(g).ok())
                        .unwrap_or_default(),
                    rating: row.try_get::<Option<sqlx::types::BigDecimal>, _>("vote_average")
                        .unwrap_or(None)
                        .map(|v| v.to_string().parse::<f32>().unwrap_or(0.0)),
                    release_date: row.try_get::<Option<chrono::NaiveDate>, _>("release_date")
                        .unwrap_or(None),
                    show_description: row.try_get("show_description").ok(),
                    show_poster_url: row.try_get("show_poster_path").ok(),
                    season_poster_url: row.try_get("season_poster_path").ok(),
                    episode_still_url: row.try_get("episode_still_path").ok(),
                })
            } else {
                None
            };
            
            let duration = row.try_get::<Option<f64>, _>("duration_seconds").unwrap_or(None);
            let width = row.try_get::<Option<i32>, _>("width").unwrap_or(None);
            
            let metadata = if duration.is_some() || width.is_some() || parsed_info.is_some() || external_info.is_some() {
                Some(MediaMetadata {
                    duration,
                    width: width.map(|w| w as u32),
                    height: row.try_get::<Option<i32>, _>("height").unwrap_or(None).map(|h| h as u32),
                    video_codec: row.try_get("video_codec").ok(),
                    audio_codec: row.try_get("audio_codec").ok(),
                    bitrate: None,
                    framerate: None,
                    file_size: row.get::<i64, _>("file_size") as u64,
                    parsed_info,
                    external_info,
                })
            } else {
                None
            };
            
            media_files.push(MediaFile {
                id: row.get("id"),
                path: std::path::PathBuf::from(row.get::<String, _>("file_path")),
                filename: row.get("file_name"),
                size: row.get::<i64, _>("file_size") as u64,
                created_at: row.get("created_at"),
                metadata,
            });
        }
        
        Ok(media_files)
    }
    
    async fn get_stats(&self) -> Result<MediaStats> {
        debug!("Retrieving media statistics");
        
        let total_row = sqlx::query!(
            "SELECT COUNT(*) as count, COALESCE(SUM(file_size), 0) as total_size FROM media_files"
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| MediaError::InvalidMedia(format!("Failed to get stats: {}", e)))?;
        
        let type_rows = sqlx::query!(
            "SELECT media_type, COUNT(*) as count FROM media_files GROUP BY media_type"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::InvalidMedia(format!("Failed to get type stats: {}", e)))?;
        
        let mut by_type = HashMap::new();
        for row in type_rows {
            by_type.insert(row.media_type, row.count.unwrap_or(0) as u64);
        }
        
        Ok(MediaStats {
            total_files: total_row.count.unwrap_or(0) as u64,
            total_size: total_row.total_size.map(|v| {
                use sqlx::types::BigDecimal;
                let big_dec: BigDecimal = v;
                big_dec.to_string().parse::<u64>().unwrap_or(0)
            }).unwrap_or(0),
            by_type,
        })
    }
    
    async fn file_exists(&self, path: &str) -> Result<bool> {
        let result = sqlx::query!(
            "SELECT EXISTS(SELECT 1 FROM media_files WHERE file_path = $1) as exists",
            path
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| MediaError::InvalidMedia(format!("Failed to check file existence: {}", e)))?;
        
        Ok(result.exists.unwrap_or(false))
    }
    
    async fn store_external_metadata(&self, media_id: &str, metadata: &MediaMetadata) -> Result<()> {
        if let Some(external) = &metadata.external_info {
            // Strip "media:" prefix if present
            let uuid_str = if media_id.starts_with("media:") {
                media_id.strip_prefix("media:").unwrap_or(media_id)
            } else {
                media_id
            };
            
            let uuid = Uuid::parse_str(uuid_str)
                .map_err(|e| MediaError::InvalidMedia(format!("Invalid UUID: {}", e)))?;
            
            let title = external.description.as_ref()
                .map(|d| d.chars().take(100).collect::<String>())
                .unwrap_or_else(|| "Unknown".to_string());
            
            // Store genres as JSONB
            let genres_json = if !external.genres.is_empty() {
                Some(serde_json::to_value(&external.genres).unwrap_or(serde_json::Value::Null))
            } else {
                None
            };
            
            sqlx::query!(
                r#"
                INSERT INTO external_metadata (
                    media_file_id, source, external_id, title, overview, release_date,
                    vote_average, poster_path, backdrop_path, genres
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                ON CONFLICT (media_file_id, source) DO UPDATE
                SET external_id = EXCLUDED.external_id,
                    title = EXCLUDED.title,
                    overview = EXCLUDED.overview,
                    poster_path = EXCLUDED.poster_path,
                    backdrop_path = EXCLUDED.backdrop_path,
                    genres = EXCLUDED.genres,
                    vote_average = EXCLUDED.vote_average,
                    updated_at = NOW()
                "#,
                uuid,
                "tmdb", // source
                external.tmdb_id.map(|id| id.to_string()).unwrap_or_else(|| "unknown".to_string()), // external_id (required)
                title,
                external.description.as_deref(),
                external.release_date,
                external.rating.map(|r| {
                    use sqlx::types::BigDecimal;
                    use std::str::FromStr;
                    BigDecimal::from_str(&r.to_string()).unwrap_or_default()
                }),
                external.poster_url.as_deref(),
                external.backdrop_url.as_deref(),
                genres_json
            )
            .execute(&self.pool)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to store external metadata: {}", e)))?;
        }
        
        Ok(())
    }
    
    async fn store_tv_show(&self, show_info: &TvShowInfo) -> Result<String> {
        sqlx::query!(
            r#"
            INSERT INTO tv_shows (id, tmdb_id, name, overview, poster_path, backdrop_path)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (tmdb_id) DO UPDATE
            SET name = EXCLUDED.name,
                overview = EXCLUDED.overview,
                updated_at = NOW()
            RETURNING id
            "#,
            show_info.id,
            show_info.tmdb_id,
            show_info.name,
            show_info.overview.as_deref(),
            show_info.poster_path.as_deref(),
            show_info.backdrop_path.as_deref()
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| MediaError::InvalidMedia(format!("Failed to store TV show: {}", e)))?;
        
        for season in &show_info.seasons {
            sqlx::query!(
                r#"
                INSERT INTO tv_seasons (id, tv_show_id, season_number, name, episode_count, poster_path)
                VALUES ($1, $2, $3, $4, $5, $6)
                ON CONFLICT (tv_show_id, season_number) DO UPDATE
                SET name = EXCLUDED.name,
                    episode_count = EXCLUDED.episode_count,
                    updated_at = NOW()
                "#,
                season.id,
                show_info.id,
                season.season_number,
                season.name.as_deref(),
                season.episode_count,
                season.poster_path.as_deref()
            )
            .execute(&self.pool)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to store season: {}", e)))?;
        }
        
        Ok(show_info.id.to_string())
    }
    
    async fn get_tv_show(&self, tmdb_id: &str) -> Result<Option<TvShowInfo>> {
        let show_row = sqlx::query!(
            "SELECT id, tmdb_id, name, overview, poster_path, backdrop_path FROM tv_shows WHERE tmdb_id = $1",
            tmdb_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::InvalidMedia(format!("Failed to get TV show: {}", e)))?;
        
        if let Some(show) = show_row {
            let seasons = sqlx::query!(
                "SELECT id, season_number, name, episode_count, poster_path FROM tv_seasons WHERE tv_show_id = $1 ORDER BY season_number",
                show.id
            )
            .fetch_all(&self.pool)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to get seasons: {}", e)))?
            .into_iter()
            .map(|s| SeasonInfo {
                id: s.id,
                season_number: s.season_number,
                name: s.name,
                episode_count: s.episode_count.unwrap_or(0),
                poster_path: s.poster_path,
            })
            .collect();
            
            Ok(Some(TvShowInfo {
                id: show.id,
                tmdb_id: show.tmdb_id,
                name: show.name,
                overview: show.overview,
                poster_path: show.poster_path,
                backdrop_path: show.backdrop_path,
                seasons,
            }))
        } else {
            Ok(None)
        }
    }
    
    async fn link_episode_to_file(&self, media_file_id: &str, show_tmdb_id: &str, season: i32, episode: i32) -> Result<()> {
        // Strip "media:" prefix if present
        let uuid_str = if media_file_id.starts_with("media:") {
            media_file_id.strip_prefix("media:").unwrap_or(media_file_id)
        } else {
            media_file_id
        };
        
        let file_uuid = Uuid::parse_str(uuid_str)
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid file UUID: {}", e)))?;
        
        sqlx::query!(
            r#"
            UPDATE tv_episodes
            SET media_file_id = $1
            WHERE tv_show_id = (SELECT id FROM tv_shows WHERE tmdb_id = $2)
              AND season_id = (SELECT id FROM tv_seasons WHERE tv_show_id = (SELECT id FROM tv_shows WHERE tmdb_id = $2) AND season_number = $3)
              AND episode_number = $4
            "#,
            file_uuid,
            show_tmdb_id,
            season,
            episode
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::InvalidMedia(format!("Failed to link episode to file: {}", e)))?;
        
        Ok(())
    }
    
    async fn delete_media(&self, id: &str) -> Result<()> {
        // Strip "media:" prefix if present
        let uuid_str = if id.starts_with("media:") {
            id.strip_prefix("media:").unwrap_or(id)
        } else {
            id
        };
        
        let uuid = Uuid::parse_str(uuid_str)
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid UUID: {}", e)))?;
        
        sqlx::query!(
            "DELETE FROM media_files WHERE id = $1",
            uuid
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::InvalidMedia(format!("Failed to delete media: {}", e)))?;
        
        Ok(())
    }
    
    async fn get_all_media(&self) -> Result<Vec<MediaFile>> {
        let rows = sqlx::query(
            r#"
            SELECT 
                mf.id, mf.file_path, mf.file_name, mf.file_size, mf.created_at,
                mm.duration_seconds, mm.width, mm.height, mm.video_codec, mm.audio_codec,
                mm.parsed_info,
                em.external_id, em.title, em.overview, em.release_date,
                em.vote_average, em.poster_path, em.backdrop_path, em.genres,
                em.show_description, em.show_poster_path, em.season_poster_path, em.episode_still_path
            FROM media_files mf
            LEFT JOIN media_metadata mm ON mf.id = mm.media_file_id
            LEFT JOIN external_metadata em ON mf.id = em.media_file_id
            ORDER BY mf.created_at DESC
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::InvalidMedia(format!("Failed to fetch all media: {}", e)))?;
        
        let mut media_files = Vec::new();
        for row in rows {
            media_files.push(self.row_to_media_file(row));
        }
        
        Ok(media_files)
    }
}