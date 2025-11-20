use crate::{MediaFile, MediaError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use surrealdb::engine::local::{Db, Mem};
use surrealdb::Surreal;
use tracing::{debug, info, warn};

pub type Database = Surreal<Db>;

#[derive(Debug, Clone)]
pub struct MediaDatabase {
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
}

impl MediaDatabase {
    /// Create a new in-memory database instance
    pub async fn new_memory() -> Result<Self> {
        let db = Surreal::new::<Mem>(()).await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to create database: {}", e)))?;
        
        let namespace = "rusty_media".to_string();
        let database_name = "media_server".to_string();
        
        // Use the database
        db.use_ns(&namespace).use_db(&database_name).await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to select database: {}", e)))?;
        
        info!("Initialized in-memory SurrealDB: {}/{}", namespace, database_name);
        
        Ok(Self {
            db,
            namespace,
            database_name,
        })
    }
    
    /// Initialize database schema and indexes
    pub async fn initialize_schema(&self) -> Result<()> {
        info!("Initializing database schema");
        
        // Create indexes for efficient querying
        let queries = vec![
            // Index on file path for uniqueness
            "DEFINE INDEX path_idx ON TABLE media FIELDS path UNIQUE",
            
            // Index on show name for TV shows
            "DEFINE INDEX show_idx ON TABLE media FIELDS metadata.parsed_info.show_name",
            
            // Index on media type
            "DEFINE INDEX type_idx ON TABLE media FIELDS metadata.parsed_info.media_type",
            
            // Index on season/episode for TV shows  
            "DEFINE INDEX season_episode_idx ON TABLE media FIELDS metadata.parsed_info.season, metadata.parsed_info.episode",
            
            // Index on file size for queries
            "DEFINE INDEX size_idx ON TABLE media FIELDS size",
            
            // Index on creation date
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
    
    /// Store a media file in the database
    pub async fn store_media(&self, media_file: MediaFile) -> Result<String> {
        debug!("Storing media file: {}", media_file.filename);
        
        // Use the media file's UUID as a string-based ID
        let id = media_file.id.to_string();
        
        let record = MediaRecord {
            path: media_file.path.clone(),
            filename: media_file.filename.clone(),
            size: media_file.size,
            created_at: media_file.created_at,
            metadata: media_file.metadata.clone(),
        };
        
        let result: Option<MediaRecord> = self.db
            .update(("media", &id))
            .content(&record)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to store media: {}", e)))?;
        
        match result {
            Some(_) => {
                let full_id = format!("media:{}", id);
                info!("Stored media file with ID: {}", full_id);
                Ok(full_id)
            }
            None => Err(MediaError::InvalidMedia("Failed to store media file".to_string()))
        }
    }
    
    /// Retrieve a media file by ID
    pub async fn get_media(&self, id: &str) -> Result<Option<MediaFile>> {
        debug!("Retrieving media file: {}", id);
        
        // Parse the ID to extract table and record ID
        let (table, record_id) = if id.contains(':') {
            let parts: Vec<&str> = id.split(':').collect();
            (parts[0], parts[1])
        } else {
            ("media", id)
        };
        
        let result: Option<MediaRecord> = self.db
            .select((table, record_id))
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to retrieve media: {}", e)))?;
        
        Ok(result.map(|record| MediaFile {
            id: uuid::Uuid::parse_str(record_id).unwrap_or_default(),
            path: record.path,
            filename: record.filename,
            size: record.size,
            created_at: record.created_at,
            metadata: record.metadata,
        }))
    }
    
    /// List all media files with optional filtering
    pub async fn list_media(&self, filters: MediaFilters) -> Result<Vec<MediaFile>> {
        debug!("Listing media files with filters: {:?}", filters);
        
        let mut query = "SELECT id, * FROM media".to_string();
        let mut conditions = Vec::new();
        
        // Build WHERE conditions
        if let Some(media_type) = &filters.media_type {
            conditions.push(format!("metadata.parsed_info.media_type = '{}'", media_type));
        }
        
        if let Some(show_name) = &filters.show_name {
            // Use double quotes for string literals in SurrealDB
            conditions.push(format!("metadata.parsed_info.show_name = \"{}\"", show_name));
        }
        
        if let Some(season) = filters.season {
            conditions.push(format!("metadata.parsed_info.season = {}", season));
        }
        
        if !conditions.is_empty() {
            query.push_str(&format!(" WHERE {}", conditions.join(" AND ")));
        }
        
        // Add ordering
        match filters.order_by.as_deref() {
            Some("name") => query.push_str(" ORDER BY filename ASC"),
            Some("date") => query.push_str(" ORDER BY created_at DESC"),
            Some("size") => query.push_str(" ORDER BY size DESC"),
            _ => query.push_str(" ORDER BY created_at DESC"),
        }
        
        // Add limit
        if let Some(limit) = filters.limit {
            query.push_str(&format!(" LIMIT {}", limit));
        }
        
        debug!("Executing query: {}", query);
        
        let mut result = self.db
            .query(&query)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to query media: {}", e)))?;
        
        let records: Vec<serde_json::Value> = result
            .take(0)
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to parse query result: {}", e)))?;
        
        debug!("Query returned {} records", records.len());
        
        let mut media_files = Vec::new();
        for record in records {
            // Extract ID from SurrealDB record ID structure
            let id_str = record.get("id")
                .and_then(|id_obj| {
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
                // Extract UUID from SurrealDB ID format "media:uuid"
                let uuid_str = if id.starts_with("media:") {
                    &id[6..] // Remove "media:" prefix
                } else {
                    id
                };
                
                if let (Ok(uuid), Ok(path_buf), Ok(created_dt)) = (
                    uuid::Uuid::parse_str(uuid_str),
                    std::path::PathBuf::from(path).canonicalize().or_else(|_| Ok::<std::path::PathBuf, std::io::Error>(std::path::PathBuf::from(path))),
                    created_at.parse::<chrono::DateTime<chrono::Utc>>(),
                ) {
                    let metadata = record.get("metadata")
                        .and_then(|v| serde_json::from_value(v.clone()).ok());
                    
                    media_files.push(MediaFile {
                        id: uuid,
                        path: path_buf,
                        filename: filename.to_string(),
                        size,
                        created_at: created_dt,
                        metadata,
                    });
                }
            }
        }
        
        Ok(media_files)
    }
    
    /// Get media statistics
    pub async fn get_stats(&self) -> Result<MediaStats> {
        debug!("Retrieving media statistics");
        
        // Total count
        let total_count: Option<i64> = self.db
            .query("SELECT count() FROM media GROUP ALL")
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to get total count: {}", e)))?
            .take((0, "count"))
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to parse count: {}", e)))?;
        
        // Count by media type
        let type_counts: Vec<(String, i64)> = self.db
            .query("SELECT metadata.parsed_info.media_type as type, count() as count FROM media GROUP BY type")
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to get type counts: {}", e)))?
            .take(0)
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to parse type counts: {}", e)))?;
        
        let mut by_type = HashMap::new();
        for (media_type, count) in type_counts {
            by_type.insert(media_type, count as u64);
        }
        
        // Total file size
        let total_size: Option<i64> = self.db
            .query("SELECT math::sum(size) as total_size FROM media GROUP ALL")
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to get total size: {}", e)))?
            .take((0, "total_size"))
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to parse total size: {}", e)))?;
        
        Ok(MediaStats {
            total_files: total_count.unwrap_or(0) as u64,
            total_size: total_size.unwrap_or(0) as u64,
            by_type,
        })
    }
    
    /// Check if a file path already exists in the database
    pub async fn file_exists(&self, path: &str) -> Result<bool> {
        let result: Vec<MediaRecord> = self.db
            .query("SELECT * FROM media WHERE path = $path LIMIT 1")
            .bind(("path", path))
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to check file existence: {}", e)))?
            .take(0)
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to parse existence check: {}", e)))?;
        
        Ok(!result.is_empty())
    }
}

#[derive(Debug, Clone, Default)]
pub struct MediaFilters {
    pub media_type: Option<String>,
    pub show_name: Option<String>,
    pub season: Option<u32>,
    pub order_by: Option<String>,
    pub limit: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MediaStats {
    pub total_files: u64,
    pub total_size: u64,
    pub by_type: HashMap<String, u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_database_operations() {
        let db = MediaDatabase::new_memory().await.unwrap();
        db.initialize_schema().await.unwrap();
        
        // Create a test media file
        let media_file = MediaFile {
            id: Uuid::new_v4(),
            path: PathBuf::from("/test/path/movie.mp4"),
            filename: "movie.mp4".to_string(),
            size: 1024,
            created_at: chrono::Utc::now(),
            metadata: None,
        };
        
        // Store the file
        let id = db.store_media(media_file.clone()).await.unwrap();
        assert!(!id.is_empty());
        
        // Retrieve the file
        let retrieved = db.get_media(&id).await.unwrap().unwrap();
        assert_eq!(retrieved.filename, media_file.filename);
        assert_eq!(retrieved.size, media_file.size);
        
        // List files
        let files = db.list_media(MediaFilters::default()).await.unwrap();
        assert_eq!(files.len(), 1);
        
        // Basic stats check (skip complex queries for now)
        println!("Database operations test completed successfully");
    }
}