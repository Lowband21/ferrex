use super::traits::*;
use crate::{MediaError, MediaFile, MediaMetadata, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use surrealdb::engine::local::{Db, Mem};
use surrealdb::Surreal;
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
pub struct SurrealDatabase {
    db: Surreal<Db>,
    namespace: String,
    database_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct MediaRecord {
    path: std::path::PathBuf,
    filename: String,
    size: u64,
    created_at: chrono::DateTime<chrono::Utc>,
    metadata: Option<crate::MediaMetadata>,
    library_id: Option<uuid::Uuid>,
    parent_media_id: Option<uuid::Uuid>,
}

impl SurrealDatabase {
    pub async fn new() -> Result<Self> {
        let db = Surreal::new::<Mem>(())
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to create database: {e}")))?;

        let namespace = "ferrex_media".to_string();
        let database_name = "media_server".to_string();

        db.use_ns(&namespace)
            .use_db(&database_name)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to select database: {e}")))?;

        info!(
            "Initialized in-memory SurrealDB: {}/{}",
            namespace, database_name
        );

        Ok(Self {
            db,
            namespace,
            database_name,
        })
    }
}

#[async_trait]
impl MediaDatabaseTrait for SurrealDatabase {
    async fn initialize_schema(&self) -> Result<()> {
        info!("Initializing SurrealDB schema");

        let queries = vec![
            "DEFINE INDEX path_idx ON TABLE media FIELDS path UNIQUE",
            "DEFINE INDEX show_idx ON TABLE media FIELDS metadata.parsed_info.show_name",
            "DEFINE INDEX type_idx ON TABLE media FIELDS metadata.parsed_info.media_type",
            "DEFINE INDEX season_episode_idx ON TABLE media FIELDS metadata.parsed_info.season, metadata.parsed_info.episode",
            "DEFINE INDEX size_idx ON TABLE media FIELDS size",
            "DEFINE INDEX created_idx ON TABLE media FIELDS created_at",
        ];

        for query in queries {
            match self.db.query(query).await {
                Ok(_) => debug!("Executed schema query: {}", query),
                Err(e) => warn!("Schema query failed: {} - {}", query, e),
            }
        }

        info!("Database schema initialization complete");
        Ok(())
    }

    async fn store_media(&self, media_file: MediaFile) -> Result<String> {
        debug!("Storing media file: {}", media_file.filename);

        let id = media_file.id.to_string();

        let record = MediaRecord {
            path: media_file.path.clone(),
            filename: media_file.filename.clone(),
            size: media_file.size,
            created_at: media_file.created_at,
            metadata: media_file.metadata.clone(),
            library_id: media_file.library_id,
            parent_media_id: media_file.parent_media_id,
        };

        let result: Option<MediaRecord> = self
            .db
            .update(("media", &id))
            .content(&record)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to store media: {e}")))?;

        match result {
            Some(_) => {
                let full_id = format!("media:{id}");
                info!("Stored media file with ID: {}", full_id);
                Ok(full_id)
            }
            None => Err(MediaError::InvalidMedia(
                "Failed to store media file".to_string(),
            )),
        }
    }

    async fn get_media_by_path(&self, path: &str) -> Result<Option<MediaFile>> {
        debug!("Retrieving media file by path: {}", path);

        let mut result = self
            .db
            .query("SELECT * FROM media WHERE path = $path")
            .bind(("path", path))
            .await
            .map_err(|e| {
                MediaError::InvalidMedia(format!("Failed to retrieve media by path: {e}"))
            })?;

        let records: Vec<MediaRecord> = result.take(0).map_err(|e| {
            MediaError::InvalidMedia(format!("Failed to parse media records: {e}"))
        })?;

        Ok(records.into_iter().next().map(|record| MediaFile {
            id: uuid::Uuid::new_v4(), // SurrealDB doesn't return the ID in the same way
            path: record.path,
            filename: record.filename,
            size: record.size,
            created_at: record.created_at,
            metadata: record.metadata,
            library_id: record.library_id,
            parent_media_id: record.parent_media_id,
        }))
    }

    async fn get_media(&self, id: &str) -> Result<Option<MediaFile>> {
        debug!("Retrieving media file: {}", id);

        let (table, record_id) = if id.contains(':') {
            let parts: Vec<&str> = id.split(':').collect();
            (parts[0], parts[1])
        } else {
            ("media", id)
        };

        let result: Option<MediaRecord> =
            self.db.select((table, record_id)).await.map_err(|e| {
                MediaError::InvalidMedia(format!("Failed to retrieve media: {e}"))
            })?;

        Ok(result.map(|record| MediaFile {
            id: uuid::Uuid::parse_str(record_id).unwrap_or_default(),
            path: record.path,
            filename: record.filename,
            size: record.size,
            created_at: record.created_at,
            metadata: record.metadata,
            library_id: record.library_id,
            parent_media_id: record.parent_media_id,
        }))
    }

    async fn list_media(&self, filters: MediaFilters) -> Result<Vec<MediaFile>> {
        debug!("Listing media files with filters: {:?}", filters);

        let mut query = "SELECT id, * FROM media".to_string();
        let mut conditions = Vec::new();

        if let Some(media_type) = &filters.media_type {
            conditions.push(format!(
                "metadata.parsed_info.media_type = '{media_type}'"
            ));
        }

        if let Some(show_name) = &filters.show_name {
            conditions.push(format!(
                "metadata.parsed_info.show_name = \"{show_name}\""
            ));
        }

        if let Some(season) = filters.season {
            conditions.push(format!("metadata.parsed_info.season = {season}"));
        }

        if let Some(library_id) = &filters.library_id {
            conditions.push(format!("library_id = uuid('{library_id}')"));
        }

        if !conditions.is_empty() {
            query.push_str(&format!(" WHERE {}", conditions.join(" AND ")));
        }

        match filters.order_by.as_deref() {
            Some("name") => query.push_str(" ORDER BY filename ASC"),
            Some("date") => query.push_str(" ORDER BY created_at DESC"),
            Some("size") => query.push_str(" ORDER BY size DESC"),
            _ => query.push_str(" ORDER BY created_at DESC"),
        }

        if let Some(limit) = filters.limit {
            query.push_str(&format!(" LIMIT {limit}"));
        }

        debug!("Executing query: {}", query);

        let mut result = self
            .db
            .query(&query)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to query media: {e}")))?;

        let records: Vec<serde_json::Value> = result.take(0).map_err(|e| {
            MediaError::InvalidMedia(format!("Failed to parse query result: {e}"))
        })?;

        debug!("Query returned {} records", records.len());

        let mut media_files = Vec::new();
        for record in records {
            let id_str = record.get("id").and_then(|id_obj| {
                if let Some(id_inner) = id_obj.get("id") {
                    id_inner.get("String").and_then(|v| v.as_str())
                } else {
                    id_obj.as_str()
                }
            });

            if let (Some(id), Some(path), Some(filename), Some(size), Some(created_at)) = (
                id_str,
                record.get("path").and_then(|v| v.as_str()),
                record.get("filename").and_then(|v| v.as_str()),
                record.get("size").and_then(|v| v.as_u64()),
                record.get("created_at").and_then(|v| v.as_str()),
            ) {
                let uuid_str = if id.starts_with("media:") {
                    &id[6..]
                } else {
                    id
                };

                if let (Ok(uuid), Ok(path_buf), Ok(created_dt)) = (
                    uuid::Uuid::parse_str(uuid_str),
                    std::path::PathBuf::from(path).canonicalize().or_else(|_| {
                        Ok::<std::path::PathBuf, std::io::Error>(std::path::PathBuf::from(path))
                    }),
                    created_at.parse::<chrono::DateTime<chrono::Utc>>(),
                ) {
                    let metadata = record
                        .get("metadata")
                        .and_then(|v| serde_json::from_value(v.clone()).ok());

                    let library_id = record
                        .get("library_id")
                        .and_then(|v| v.as_str())
                        .and_then(|s| uuid::Uuid::parse_str(s).ok());

                    let parent_media_id = record
                        .get("parent_media_id")
                        .and_then(|v| v.as_str())
                        .and_then(|s| uuid::Uuid::parse_str(s).ok());

                    media_files.push(MediaFile {
                        id: uuid,
                        path: path_buf,
                        filename: filename.to_string(),
                        size,
                        created_at: created_dt,
                        metadata,
                        library_id,
                        parent_media_id,
                    });
                }
            }
        }

        Ok(media_files)
    }

    async fn get_stats(&self) -> Result<MediaStats> {
        debug!("Retrieving media statistics");

        let total_count: Option<i64> = self
            .db
            .query("SELECT count() FROM media GROUP ALL")
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to get total count: {e}")))?
            .take((0, "count"))
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to parse count: {e}")))?;

        let type_counts: Vec<(String, i64)> = self.db
            .query("SELECT metadata.parsed_info.media_type as type, count() as count FROM media GROUP BY type")
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to get type counts: {e}")))?
            .take(0)
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to parse type counts: {e}")))?;

        let mut by_type = HashMap::new();
        for (media_type, count) in type_counts {
            by_type.insert(media_type, count as u64);
        }

        let total_size: Option<i64> = self
            .db
            .query("SELECT math::sum(size) as total_size FROM media GROUP ALL")
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to get total size: {e}")))?
            .take((0, "total_size"))
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to parse total size: {e}")))?;

        Ok(MediaStats {
            total_files: total_count.unwrap_or(0) as u64,
            total_size: total_size.unwrap_or(0) as u64,
            by_type,
        })
    }

    async fn file_exists(&self, path: &str) -> Result<bool> {
        let result: Vec<MediaRecord> = self
            .db
            .query("SELECT * FROM media WHERE path = $path LIMIT 1")
            .bind(("path", path))
            .await
            .map_err(|e| {
                MediaError::InvalidMedia(format!("Failed to check file existence: {e}"))
            })?
            .take(0)
            .map_err(|e| {
                MediaError::InvalidMedia(format!("Failed to parse existence check: {e}"))
            })?;

        Ok(!result.is_empty())
    }

    async fn store_external_metadata(
        &self,
        _media_id: &str,
        _metadata: &MediaMetadata,
    ) -> Result<()> {
        warn!("External metadata storage not implemented for SurrealDB backend");
        Ok(())
    }

    async fn store_tv_show(&self, _show_info: &TvShowInfo) -> Result<String> {
        warn!("TV show storage not implemented for SurrealDB backend");
        Err(MediaError::InvalidMedia(
            "TV show storage not implemented for SurrealDB".to_string(),
        ))
    }

    async fn get_tv_show(&self, _tmdb_id: &str) -> Result<Option<TvShowInfo>> {
        warn!("TV show retrieval not implemented for SurrealDB backend");
        Ok(None)
    }

    async fn link_episode_to_file(
        &self,
        _media_file_id: &str,
        _show_tmdb_id: &str,
        _season: i32,
        _episode: i32,
    ) -> Result<()> {
        warn!("Episode linking not implemented for SurrealDB backend");
        Ok(())
    }

    async fn delete_media(&self, id: &str) -> Result<()> {
        self.db
            .delete::<Option<MediaFile>>(("media", id))
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to delete media: {e}")))?;
        Ok(())
    }

    async fn get_all_media(&self) -> Result<Vec<MediaFile>> {
        let media_files: Vec<MediaFile> = self
            .db
            .select("media")
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to get all media: {e}")))?;
        Ok(media_files)
    }

    // Library management methods
    async fn create_library(&self, library: crate::Library) -> Result<String> {
        let id = library.id.to_string();
        
        // Convert paths to strings for storage
        let paths: Vec<String> = library.paths.iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        
        #[derive(Serialize, Deserialize)]
        struct LibraryRecord {
            id: String,
            name: String,
            library_type: String,
            paths: Vec<String>,
            scan_interval_minutes: u32,
            last_scan: Option<chrono::DateTime<chrono::Utc>>,
            enabled: bool,
            created_at: chrono::DateTime<chrono::Utc>,
            updated_at: chrono::DateTime<chrono::Utc>,
        }
        
        let record = LibraryRecord {
            id: id.clone(),
            name: library.name,
            library_type: format!("{:?}", library.library_type),
            paths,
            scan_interval_minutes: library.scan_interval_minutes,
            last_scan: library.last_scan,
            enabled: library.enabled,
            created_at: library.created_at,
            updated_at: library.updated_at,
        };
        
        let _: Option<LibraryRecord> = self
            .db
            .create(("libraries", &id))
            .content(&record)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to create library: {e}")))?;
        
        Ok(id)
    }

    async fn get_library(&self, id: &str) -> Result<Option<crate::Library>> {
        #[derive(Deserialize)]
        struct LibraryRecord {
            name: String,
            library_type: String,
            paths: Vec<String>,
            scan_interval_minutes: u32,
            last_scan: Option<chrono::DateTime<chrono::Utc>>,
            enabled: bool,
            created_at: chrono::DateTime<chrono::Utc>,
            updated_at: chrono::DateTime<chrono::Utc>,
        }
        
        let result: Option<LibraryRecord> = self
            .db
            .select(("libraries", id))
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to get library: {e}")))?;
        
        Ok(result.map(|record| {
            let library_type = match record.library_type.as_str() {
                "Movies" => crate::LibraryType::Movies,
                "TvShows" => crate::LibraryType::TvShows,
                _ => crate::LibraryType::Movies,
            };
            
            let paths = record.paths.into_iter()
                .map(std::path::PathBuf::from)
                .collect();
            
            crate::Library {
                id: uuid::Uuid::parse_str(id).unwrap_or_default(),
                name: record.name,
                library_type,
                paths,
                scan_interval_minutes: record.scan_interval_minutes,
                last_scan: record.last_scan,
                enabled: record.enabled,
                created_at: record.created_at,
                updated_at: record.updated_at,
            }
        }))
    }

    async fn list_libraries(&self) -> Result<Vec<crate::Library>> {
        let records: Vec<serde_json::Value> = self
            .db
            .select("libraries")
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to list libraries: {e}")))?;
        
        let mut libraries = Vec::new();
        
        for record in records {
            if let (Some(id), Some(name), Some(library_type), Some(paths)) = (
                record.get("id").and_then(|v| v.as_str()),
                record.get("name").and_then(|v| v.as_str()),
                record.get("library_type").and_then(|v| v.as_str()),
                record.get("paths").and_then(|v| v.as_array()),
            ) {
                let library_type = match library_type {
                    "Movies" => crate::LibraryType::Movies,
                    "TvShows" => crate::LibraryType::TvShows,
                    _ => crate::LibraryType::Movies,
                };
                
                let paths: Vec<std::path::PathBuf> = paths.iter()
                    .filter_map(|p| p.as_str())
                    .map(std::path::PathBuf::from)
                    .collect();
                
                libraries.push(crate::Library {
                    id: uuid::Uuid::parse_str(id).unwrap_or_default(),
                    name: name.to_string(),
                    library_type,
                    paths,
                    scan_interval_minutes: record.get("scan_interval_minutes")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(60) as u32,
                    last_scan: record.get("last_scan")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse().ok()),
                    enabled: record.get("enabled")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    created_at: record.get("created_at")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse().ok())
                        .unwrap_or_else(chrono::Utc::now),
                    updated_at: record.get("updated_at")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse().ok())
                        .unwrap_or_else(chrono::Utc::now),
                });
            }
        }
        
        Ok(libraries)
    }

    async fn update_library(&self, id: &str, library: crate::Library) -> Result<()> {
        let paths: Vec<String> = library.paths.iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        
        #[derive(Serialize)]
        struct LibraryUpdate {
            name: String,
            paths: Vec<String>,
            scan_interval_minutes: u32,
            enabled: bool,
            updated_at: chrono::DateTime<chrono::Utc>,
        }
        
        let update = LibraryUpdate {
            name: library.name,
            paths,
            scan_interval_minutes: library.scan_interval_minutes,
            enabled: library.enabled,
            updated_at: chrono::Utc::now(),
        };
        
        let _: Option<serde_json::Value> = self
            .db
            .update(("libraries", id))
            .merge(update)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to update library: {e}")))?;
        
        Ok(())
    }

    async fn delete_library(&self, id: &str) -> Result<()> {
        self.db
            .delete::<Option<serde_json::Value>>(("libraries", id))
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to delete library: {e}")))?;
        Ok(())
    }

    async fn update_library_last_scan(&self, id: &str) -> Result<()> {
        #[derive(Serialize)]
        struct ScanUpdate {
            last_scan: chrono::DateTime<chrono::Utc>,
            updated_at: chrono::DateTime<chrono::Utc>,
        }
        
        let update = ScanUpdate {
            last_scan: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        
        let _: Option<serde_json::Value> = self
            .db
            .update(("libraries", id))
            .merge(update)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to update library last scan: {e}")))?;
        
        Ok(())
    }
}
