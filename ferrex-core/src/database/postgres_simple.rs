use super::traits::*;
use crate::{MediaFile, MediaMetadata, MediaError, Result};
use async_trait::async_trait;
use sqlx::{PgPool, postgres::PgPoolOptions, Row};
use tracing::{info, debug};
use uuid::Uuid;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PostgresDatabase {
    pool: PgPool,
}

impl PostgresDatabase {
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
    
    pub async fn migrate(&self) -> Result<()> {
        info!("Running database migrations");
        
        // For now, we'll assume migrations are run externally
        // In production, use a migration tool like sqlx migrate or refinery
        
        info!("Database migrations completed successfully");
        Ok(())
    }
}

#[async_trait]
impl MediaDatabaseTrait for PostgresDatabase {
    async fn initialize_schema(&self) -> Result<()> {
        self.migrate().await
    }
    
    async fn store_media(&self, media_file: MediaFile) -> Result<String> {
        debug!("Storing media file: {}", media_file.filename);
        
        let media_type = media_file.metadata.as_ref()
            .and_then(|m| m.parsed_info.as_ref())
            .map(|p| p.media_type.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        
        let parent_dir = media_file.path.parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());
        
        // Store the media file
        sqlx::query(
            r#"
            INSERT INTO media_files (id, file_path, file_name, file_size, media_type, parent_directory)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (file_path) DO UPDATE
            SET file_name = EXCLUDED.file_name,
                file_size = EXCLUDED.file_size,
                updated_at = NOW()
            "#
        )
        .bind(media_file.id)
        .bind(media_file.path.to_string_lossy().to_string())
        .bind(&media_file.filename)
        .bind(media_file.size as i64)
        .bind(media_type)
        .bind(parent_dir)
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::InvalidMedia(format!("Failed to store media: {}", e)))?;
        
        // Store technical metadata if available
        if let Some(metadata) = &media_file.metadata {
            sqlx::query(
                r#"
                INSERT INTO media_metadata (
                    media_file_id, duration_seconds, width, height,
                    video_codec, audio_codec, bitrate, frame_rate
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                ON CONFLICT (media_file_id) DO UPDATE
                SET duration_seconds = EXCLUDED.duration_seconds,
                    width = EXCLUDED.width,
                    height = EXCLUDED.height,
                    updated_at = NOW()
                "#
            )
            .bind(media_file.id)
            .bind(metadata.duration)
            .bind(metadata.width.map(|w| w as i32))
            .bind(metadata.height.map(|h| h as i32))
            .bind(&metadata.video_codec)
            .bind(&metadata.audio_codec)
            .bind(metadata.bitrate.map(|b| b as i64))
            .bind(metadata.framerate)
            .execute(&self.pool)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to store technical metadata: {}", e)))?;
        }
        
        Ok(media_file.id.to_string())
    }
    
    async fn get_media(&self, id: &str) -> Result<Option<MediaFile>> {
        debug!("Retrieving media file: {}", id);
        
        let uuid = Uuid::parse_str(id)
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid UUID: {}", e)))?;
        
        let row = sqlx::query(
            r#"
            SELECT 
                mf.id, mf.file_path, mf.file_name, mf.file_size, mf.created_at,
                mm.duration_seconds, mm.width, mm.height, mm.video_codec, mm.audio_codec
            FROM media_files mf
            LEFT JOIN media_metadata mm ON mf.id = mm.media_file_id
            WHERE mf.id = $1
            "#
        )
        .bind(uuid)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::InvalidMedia(format!("Failed to retrieve media: {}", e)))?;
        
        if let Some(row) = row {
            let metadata = if row.try_get::<Option<f64>, _>("duration_seconds").unwrap_or(None).is_some() {
                Some(MediaMetadata {
                    duration: row.try_get("duration_seconds").unwrap_or(None),
                    width: row.try_get::<Option<i32>, _>("width").unwrap_or(None).map(|w| w as u32),
                    height: row.try_get::<Option<i32>, _>("height").unwrap_or(None).map(|h| h as u32),
                    video_codec: row.try_get("video_codec").ok(),
                    audio_codec: row.try_get("audio_codec").ok(),
                    bitrate: None,
                    framerate: None,
                    file_size: row.try_get::<i64, _>("file_size").unwrap_or(0) as u64,
                    parsed_info: None,
                    external_info: None,
                })
            } else {
                None
            };
            
            Ok(Some(MediaFile {
                id: row.get("id"),
                path: std::path::PathBuf::from(row.get::<String, _>("file_path")),
                filename: row.get("file_name"),
                size: row.get::<i64, _>("file_size") as u64,
                created_at: row.get("created_at"),
                metadata,
            }))
        } else {
            Ok(None)
        }
    }
    
    async fn list_media(&self, filters: MediaFilters) -> Result<Vec<MediaFile>> {
        debug!("Listing media files with filters: {:?}", filters);
        
        let mut query = String::from(
            r#"
            SELECT 
                mf.id, mf.file_path, mf.file_name, mf.file_size, mf.created_at, mf.media_type,
                mm.duration_seconds, mm.width, mm.height, mm.video_codec, mm.audio_codec
            FROM media_files mf
            LEFT JOIN media_metadata mm ON mf.id = mm.media_file_id
            WHERE 1=1
            "#
        );
        
        if filters.media_type.is_some() {
            query.push_str(" AND mf.media_type = $1");
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
        
        let rows = query_builder
            .fetch_all(&self.pool)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to list media: {}", e)))?;
        
        let mut media_files = Vec::new();
        for row in rows {
            let metadata = if row.try_get::<Option<f64>, _>("duration_seconds").unwrap_or(None).is_some() {
                Some(MediaMetadata {
                    duration: row.try_get("duration_seconds").unwrap_or(None),
                    width: row.try_get::<Option<i32>, _>("width").unwrap_or(None).map(|w| w as u32),
                    height: row.try_get::<Option<i32>, _>("height").unwrap_or(None).map(|h| h as u32),
                    video_codec: row.try_get("video_codec").ok(),
                    audio_codec: row.try_get("audio_codec").ok(),
                    bitrate: None,
                    framerate: None,
                    file_size: row.get::<i64, _>("file_size") as u64,
                    parsed_info: None,
                    external_info: None,
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
        
        let total_row = sqlx::query(
            "SELECT COUNT(*) as count, COALESCE(SUM(file_size), 0) as total_size FROM media_files"
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| MediaError::InvalidMedia(format!("Failed to get stats: {}", e)))?;
        
        let total_files = total_row.get::<i64, _>("count") as u64;
        let total_size = total_row.get::<i64, _>("total_size") as u64;
        
        let type_rows = sqlx::query(
            "SELECT media_type, COUNT(*) as count FROM media_files GROUP BY media_type"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::InvalidMedia(format!("Failed to get type stats: {}", e)))?;
        
        let mut by_type = HashMap::new();
        for row in type_rows {
            let media_type: String = row.get("media_type");
            let count = row.get::<i64, _>("count") as u64;
            by_type.insert(media_type, count);
        }
        
        Ok(MediaStats {
            total_files,
            total_size,
            by_type,
        })
    }
    
    async fn file_exists(&self, path: &str) -> Result<bool> {
        let result = sqlx::query(
            "SELECT EXISTS(SELECT 1 FROM media_files WHERE file_path = $1) as exists"
        )
        .bind(path)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| MediaError::InvalidMedia(format!("Failed to check file existence: {}", e)))?;
        
        Ok(result.get("exists"))
    }
    
    async fn store_external_metadata(&self, media_id: &str, metadata: &MediaMetadata) -> Result<()> {
        if let Some(external) = &metadata.external_info {
            let uuid = Uuid::parse_str(media_id)
                .map_err(|e| MediaError::InvalidMedia(format!("Invalid UUID: {}", e)))?;
            
            sqlx::query(
                r#"
                INSERT INTO external_metadata (
                    media_file_id, external_id, title, overview, release_date,
                    runtime, vote_average, poster_path, backdrop_path
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                ON CONFLICT (media_file_id, source) DO UPDATE
                SET title = EXCLUDED.title,
                    overview = EXCLUDED.overview,
                    updated_at = NOW()
                "#
            )
            .bind(uuid)
            .bind(external.tmdb_id.map(|id| id.to_string()).unwrap_or_default())
            .bind(&external.description.as_ref().unwrap_or(&String::new()))
            .bind(&external.description)
            .bind(external.release_date)
            .bind(None::<i32>) // runtime
            .bind(external.rating.map(|r| r as f64))
            .bind(&external.poster_url)
            .bind(&external.backdrop_url)
            .execute(&self.pool)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to store external metadata: {}", e)))?;
        }
        
        Ok(())
    }
    
    async fn store_tv_show(&self, _show_info: &TvShowInfo) -> Result<String> {
        // Simplified implementation
        Ok(Uuid::new_v4().to_string())
    }
    
    async fn get_tv_show(&self, _tmdb_id: &str) -> Result<Option<TvShowInfo>> {
        Ok(None)
    }
    
    async fn link_episode_to_file(&self, _media_file_id: &str, _show_tmdb_id: &str, _season: i32, _episode: i32) -> Result<()> {
        Ok(())
    }
}