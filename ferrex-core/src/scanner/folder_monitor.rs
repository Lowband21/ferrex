use crate::database::traits::{
    FolderDiscoverySource, FolderInventory, FolderProcessingStatus, FolderScanFilters, FolderType,
    MediaDatabaseTrait,
};
use crate::{Library, LibraryID, LibraryType, MediaError, Result};
use chrono::Utc;
use std::collections::HashSet;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::{interval, MissedTickBehavior};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::fs::{FileSystem, RealFs};

/// Configuration for the folder monitor
#[derive(Debug, Clone)]
pub struct FolderMonitorConfig {
    /// Interval between folder inventory updates (in seconds)
    pub scan_interval_secs: u64,
    /// Maximum number of retries for failed folders
    pub max_retry_attempts: i32,
    /// Hours after which to consider a folder stale
    pub stale_folder_hours: i32,
    /// Batch size for processing folders
    pub batch_size: i32,
    /// Error retry threshold for failed folders
    pub error_retry_threshold: i32,
}

impl Default for FolderMonitorConfig {
    fn default() -> Self {
        Self {
            scan_interval_secs: 300, // 5 minutes
            max_retry_attempts: 3,
            stale_folder_hours: 24,
            batch_size: 100,
            error_retry_threshold: 3,
        }
    }
}

/// Monitors and maintains an inventory of folders in media libraries
pub struct FolderMonitor {
    /// Database connection
    database: Arc<dyn MediaDatabaseTrait>,
    /// Libraries to monitor
    pub libraries: Arc<RwLock<Vec<Library>>>,
    /// Configuration
    config: FolderMonitorConfig,
    /// Filesystem abstraction
    fs: Arc<dyn FileSystem>,
    /// Shutdown flag
    shutdown: Arc<RwLock<bool>>,
}

impl FolderMonitor {
/// Create a new FolderMonitor instance
    pub fn new(
        database: Arc<dyn MediaDatabaseTrait>,
        libraries: Arc<RwLock<Vec<Library>>>,
        config: FolderMonitorConfig,
    ) -> Self {
        Self {
            database,
            libraries,
            config,
            fs: Arc::new(RealFs::new()),
            shutdown: Arc::new(RwLock::new(false)),
        }
    }

    /// Create a FolderMonitor with a custom filesystem (useful for tests)
    pub fn new_with_fs(
        database: Arc<dyn MediaDatabaseTrait>,
        libraries: Arc<RwLock<Vec<Library>>>,
        config: FolderMonitorConfig,
        fs: Arc<dyn FileSystem>,
    ) -> Self {
        Self {
            database,
            libraries,
            config,
            fs,
            shutdown: Arc::new(RwLock::new(false)),
        }
    }

    /// Start the folder monitor background task
    pub async fn start(self: Arc<Self>) -> Result<()> {
        let monitor = Arc::clone(&self);

        tokio::spawn(async move {
            info!(
                "FolderMonitor started with interval of {} seconds",
                monitor.config.scan_interval_secs
            );

            let mut ticker = interval(Duration::from_secs(monitor.config.scan_interval_secs));
            ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

            loop {
                ticker.tick().await;

                // Check shutdown flag
                if *monitor.shutdown.read().await {
                    info!("FolderMonitor shutting down");
                    break;
                }

                // Run inventory update for all libraries
                if let Err(e) = monitor.run_inventory_cycle().await {
                    error!("Error in folder inventory cycle: {}", e);
                }
            }
        });

        Ok(())
    }

    /// Stop the folder monitor
    pub async fn stop(&self) {
        *self.shutdown.write().await = true;
        info!("FolderMonitor stop signal sent");
    }

    /// Discover folders for a specific library immediately
    /// This is useful when a library is created to populate inventory without waiting for next cycle
    pub async fn discover_library_folders_immediate(&self, library_id: &LibraryID) -> Result<()> {
        info!(
            "Starting immediate folder discovery for library: {}",
            library_id
        );

        // Get the specific library
        let libraries = self.libraries.read().await;
        let library = libraries
            .iter()
            .find(|l| &l.id == library_id)
            .ok_or_else(|| {
                MediaError::NotFound(format!("Library {} not found in memory", library_id))
            })?
            .clone(); // Clone to avoid holding the lock

        drop(libraries); // Release the lock early

        // Run inventory update for this specific library
        self.update_library_inventory(&library).await?;

        info!(
            "Immediate folder discovery completed for library: {}",
            library_id
        );
        Ok(())
    }

    /// Run a single inventory cycle for all libraries
    async fn run_inventory_cycle(&self) -> Result<()> {
        let libraries = self.libraries.read().await.clone();

        info!("Running inventory cycle for {} libraries", libraries.len());

        for library in libraries {
            if !library.enabled {
                debug!(
                    "Skipping disabled library: {} (ID: {})",
                    library.name, library.id
                );
                continue;
            }

            info!(
                "Starting folder inventory update for library: {} (ID: {}, Type: {:?})",
                library.name, library.id, library.library_type
            );

            match self.update_library_inventory(&library).await { Err(e) => {
                error!(
                    "Failed to update inventory for library {} (ID: {}): {}",
                    library.name, library.id, e
                );
            } _ => {
                info!(
                    "Successfully updated inventory for library: {} (ID: {})",
                    library.name, library.id
                );
            }}

            // Process folders needing scan
            if let Err(e) = self.process_pending_folders(&library).await {
                error!(
                    "Failed to process pending folders for library {} (ID: {}): {}",
                    library.name, library.id, e
                );
            }

            // Cleanup stale folders
            if let Err(e) = self.cleanup_stale_folders(&library).await {
                error!(
                    "Failed to cleanup stale folders for library {} (ID: {}): {}",
                    library.name, library.id, e
                );
            }
        }

        Ok(())
    }

    /// Update folder inventory for a specific library
    async fn update_library_inventory(&self, library: &Library) -> Result<()> {
        // First, ensure the library exists in the database
        match self.database.get_library(&library.id).await {
            Ok(Some(_)) => {
                // Library exists, proceed with inventory
                debug!(
                    "Library {} exists in database, updating inventory",
                    library.name
                );
            }
            Ok(None) => {
                // Library doesn't exist, create it first
                info!(
                    "Library {} not found in database, creating it first",
                    library.name
                );
                self.database
                    .create_library(library.clone())
                    .await
                    .map_err(|e| {
                        MediaError::Internal(format!(
                            "Failed to create library {}: {}",
                            library.name, e
                        ))
                    })?;
                info!("Library {} created successfully", library.name);
            }
            Err(e) => {
                error!("Failed to check library {} existence: {}", library.name, e);
                return Err(MediaError::Internal(format!(
                    "Failed to check library existence: {}",
                    e
                )));
            }
        }

        // Now proceed with inventory update
        match library.library_type {
            LibraryType::Movies => self.inventory_movie_folders(library).await,
            LibraryType::Series => self.inventory_tv_folders(library).await,
        }
    }

    /// Inventory movie folders in the library
    async fn inventory_movie_folders(&self, library: &Library) -> Result<()> {
        for path in &library.paths {
            if !self.fs.path_exists(path).await {
                warn!("Library path does not exist: {}", path.display());
                continue;
            }


            self.traverse_movie_directory(library.id, path, None).await?;
        }

        Ok(())
    }

    /// Traverse a directory and inventory movie folders
    fn traverse_movie_directory<'a>(
        &'a self,
        library_id: LibraryID,
        dir: &'a Path,
        parent_id: Option<Uuid>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let mut entries = self
                .fs
                .read_dir(dir)
                .await
                .map_err(|e| MediaError::Internal(format!("Failed to read directory {:?}: {}", dir, e)))?;

            let mut video_files: Vec<(std::path::PathBuf, u64, Option<std::time::SystemTime>)> = Vec::new();
            let mut subdirs = Vec::new();

            while let Some(path) = entries
                .next_entry()
                .await
                .map_err(|e| MediaError::Internal(format!("Failed to read entry: {}", e)))?
            {
                let metadata = self
                    .fs
                    .metadata(&path)
                    .await
                    .map_err(|e| MediaError::Internal(format!("Failed to get metadata: {}", e)))?;

if metadata.is_dir {
                    subdirs.push(path);
                } else if metadata.is_file {
                    if let Some(ext) = path.extension() {
                        let ext_str = ext.to_string_lossy().to_lowercase();
                        if is_video_extension(&ext_str) {
                            video_files.push((path, metadata.len, metadata.modified));
                        }
                    }
                }
            }

            // Determine folder type based on content
            let folder_type = if !video_files.is_empty() {
                FolderType::Movie
            } else if subdirs.len() > 0 {
                FolderType::Unknown // Will be determined by subdirectory content
            } else {
                FolderType::Extra
            };

            // Compute aggregate stats
            let total_size: i64 = video_files.iter().map(|(_, size, _)| *size as i64).sum();
            let file_types: HashSet<String> = video_files
                .iter()
                .filter_map(|(path, _, _)| path.extension())
                .map(|ext| ext.to_string_lossy().to_lowercase())
                .collect();
            // Compute max modified time across video files
            let max_mtime = video_files
                .iter()
                .filter_map(|(_, _, m)| *m)
                .max();
            let max_mtime_utc = max_mtime.and_then(|t| {
                let dur = t.duration_since(std::time::UNIX_EPOCH).ok()?;
                chrono::DateTime::<chrono::Utc>::from_timestamp(dur.as_secs() as i64, dur.subsec_nanos())
            });

            // Fetch existing folder (if any) to decide status
            let existing = self.database.get_folder_by_path(library_id, dir).await?;

            // Decide processing status and carry-forward fields
            let (id, processing_status, processed_files, last_processed_at, processing_attempts, next_retry_at, processing_error) =
                if let Some(existing) = existing {
                    // Normalize file_types for stable comparison
                    let mut new_types_vec: Vec<String> = file_types.iter().cloned().collect();
                    new_types_vec.sort();
                    let mut existing_types = existing.file_types.clone();
                    existing_types.sort();

                    let changed = existing.total_files != video_files.len() as i32
                        || existing.total_size_bytes != total_size
                        || existing_types != new_types_vec
                        || match (max_mtime_utc, existing.last_modified) {
                            (Some(new_m), Some(old_m)) => new_m > old_m,
                            (Some(_), None) => true,
                            _ => false,
                        };
                    if changed {
                        (existing.id, FolderProcessingStatus::Pending, 0, existing.last_processed_at, existing.processing_attempts, None, None)
                    } else {
                        (
                            existing.id,
                            existing.processing_status,
                            existing.processed_files,
                            existing.last_processed_at,
                            existing.processing_attempts,
                            existing.next_retry_at,
                            existing.processing_error,
                        )
                    }
                } else {
                    (Uuid::now_v7(), FolderProcessingStatus::Pending, 0, None, 0, None, None)
                };

            let folder_inventory = FolderInventory {
                id,
                library_id,
                folder_path: dir.to_string_lossy().to_string(),
                folder_type,
                parent_folder_id: parent_id,
                discovered_at: Utc::now(),
                last_seen_at: Utc::now(),
                discovery_source: FolderDiscoverySource::Scan,
                processing_status,
                last_processed_at,
                processing_error,
                processing_attempts,
                next_retry_at,
                total_files: video_files.len() as i32,
                processed_files,
                total_size_bytes: total_size,
                file_types: file_types.clone().into_iter().collect(),
                last_modified: max_mtime_utc,
                metadata: serde_json::json!({
                    "subdirectory_count": subdirs.len(),
                    "video_file_count": video_files.len()
                }),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };

            let folder_id = self.database.upsert_folder(&folder_inventory).await?;

            // Always recursively traverse subdirectories
            for subdir in subdirs {
                if let Err(e) = self
                    .traverse_movie_directory(library_id, &subdir, Some(folder_id))
                    .await
                {
                    warn!("Failed to traverse subdirectory {:?}: {}", subdir, e);
                }
            }

            Ok(())
        })
    }

    /// Inventory TV show folders in the library
    async fn inventory_tv_folders(&self, library: &Library) -> Result<()> {
for path in &library.paths {
            if !self.fs.path_exists(path).await {
                warn!("Library path does not exist: {}", path.display());
                continue;
            }

            // Traverse and inventory TV show folders (root folder will be created by traverse)
            self.traverse_tv_directory(library.id, path, None, 0)
                .await?;
        }

        Ok(())
    }

    /// Traverse a directory and inventory TV show folders
    fn traverse_tv_directory<'a>(
        &'a self,
        library_id: LibraryID,
        dir: &'a Path,
        parent_id: Option<Uuid>,
        depth: usize,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
let mut entries = self
                .fs
                .read_dir(dir)
                .await
                .map_err(|e| MediaError::Internal(format!("Failed to read directory {:?}: {}", dir, e)))?;

            let mut video_files: Vec<(std::path::PathBuf, u64, Option<std::time::SystemTime>)> = Vec::new();
            let mut subdirs = Vec::new();

            while let Some(path) = entries
                .next_entry()
                .await
                .map_err(|e| MediaError::Internal(format!("Failed to read entry: {}", e)))?
            {
                let metadata = self
                    .fs
                    .metadata(&path)
                    .await
                    .map_err(|e| MediaError::Internal(format!("Failed to get metadata: {}", e)))?;

if metadata.is_dir {
                    subdirs.push(path);
                } else if metadata.is_file {
                    if let Some(ext) = path.extension() {
                        let ext_str = ext.to_string_lossy().to_lowercase();
                        if is_video_extension(&ext_str) {
                            video_files.push((path, metadata.len, metadata.modified));
                        }
                    }
                }
            }

            // Determine folder type based on depth and content
            let folder_type = match depth {
                0 => FolderType::Root,
                1 => FolderType::TvShow,
                2 => {
                    // Check if this is a season folder
                    let dir_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");

                    if is_season_folder(dir_name) {
                        FolderType::Season
                    } else {
                        FolderType::Extra
                    }
                }
                _ => FolderType::Extra,
            };

            // Compute aggregate stats
            let total_size: i64 = video_files.iter().map(|(_, size, _)| *size as i64).sum();
            let file_types: HashSet<String> = video_files
                .iter()
                .filter_map(|(path, _, _)| path.extension())
                .map(|ext| ext.to_string_lossy().to_lowercase())
                .collect();
            // Compute max modified time across video files
            let max_mtime = video_files
                .iter()
                .filter_map(|(_, _, m)| *m)
                .max();
            let max_mtime_utc = max_mtime.and_then(|t| {
                let dur = t.duration_since(std::time::UNIX_EPOCH).ok()?;
                chrono::DateTime::<chrono::Utc>::from_timestamp(dur.as_secs() as i64, dur.subsec_nanos())
            });

            // Fetch existing folder (if any) to decide status
            let existing = self.database.get_folder_by_path(library_id, dir).await?;

            // Decide processing status and carry-forward fields
            let (id, processing_status, processed_files, last_processed_at, processing_attempts, next_retry_at, processing_error) =
                if let Some(existing) = existing {
                    // Normalize file_types for stable comparison
                    let mut new_types_vec: Vec<String> = file_types.iter().cloned().collect();
                    new_types_vec.sort();
                    let mut existing_types = existing.file_types.clone();
                    existing_types.sort();

                    let changed = existing.total_files != video_files.len() as i32
                        || existing.total_size_bytes != total_size
                        || existing_types != new_types_vec
                        || match (max_mtime_utc, existing.last_modified) {
                            (Some(new_m), Some(old_m)) => new_m > old_m,
                            (Some(_), None) => true,
                            _ => false,
                        };
                    if changed {
                        (existing.id, FolderProcessingStatus::Pending, 0, existing.last_processed_at, existing.processing_attempts, None, None)
                    } else {
                        (
                            existing.id,
                            existing.processing_status,
                            existing.processed_files,
                            existing.last_processed_at,
                            existing.processing_attempts,
                            existing.next_retry_at,
                            existing.processing_error,
                        )
                    }
                } else {
                    (Uuid::new_v4(), FolderProcessingStatus::Pending, 0, None, 0, None, None)
                };

            let folder_inventory = FolderInventory {
                id,
                library_id,
                folder_path: dir.to_string_lossy().to_string(),
                folder_type,
                parent_folder_id: parent_id,
                discovered_at: Utc::now(),
                last_seen_at: Utc::now(),
                discovery_source: FolderDiscoverySource::Scan,
                processing_status,
                last_processed_at,
                processing_error,
                processing_attempts,
                next_retry_at,
                total_files: video_files.len() as i32,
                processed_files,
                total_size_bytes: total_size,
                file_types: file_types.clone().into_iter().collect(),
                last_modified: max_mtime_utc,
                metadata: serde_json::json!({
                    "depth": depth,
                    "subdirectory_count": subdirs.len(),
                    "video_file_count": video_files.len()
                }),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };

            let folder_id = self.database.upsert_folder(&folder_inventory).await?;

            // Always recursively traverse subdirectories
            for subdir in subdirs {
                if let Err(e) = self
                    .traverse_tv_directory(library_id, &subdir, Some(folder_id), depth + 1)
                    .await
                {
                    warn!("Failed to traverse subdirectory {:?}: {}", subdir, e);
                }
            }

            Ok(())
        })
    }

    /// Process folders that need scanning
    async fn process_pending_folders(&self, library: &Library) -> Result<()> {
        let filters = FolderScanFilters {
            library_id: Some(library.id),
            processing_status: Some(FolderProcessingStatus::Pending),
            folder_type: None,
            max_attempts: Some(self.config.max_retry_attempts),
            stale_after_hours: None,
            limit: Some(self.config.batch_size),
            priority: None,
            max_batch_size: Some(self.config.batch_size),
            error_retry_threshold: Some(self.config.error_retry_threshold),
        };

        let folders = self.database.get_folders_needing_scan(&filters).await?;

        info!(
            "Found {} folders needing scan in library {}",
            folders.len(),
            library.name
        );

        for folder in folders {
            debug!("Processing folder: {}", folder.folder_path);

            // Mark as processing
            self.database
                .update_folder_status(folder.id, FolderProcessingStatus::Processing, None)
                .await?;

            // Here you would typically trigger the actual media scanning for this folder
            // For now, we'll just mark it as completed
            // In a real implementation, this would integrate with the existing scanner

            self.database.mark_folder_processed(folder.id).await?;
        }

        Ok(())
    }

    /// Cleanup stale folders that haven't been seen recently
    async fn cleanup_stale_folders(&self, library: &Library) -> Result<()> {
        let deleted_count = self
            .database
            .cleanup_stale_folders(library.id, self.config.stale_folder_hours)
            .await?;

        if deleted_count > 0 {
            info!(
                "Cleaned up {} stale folders from library {}",
                deleted_count, library.name
            );
        }

        Ok(())
    }
}

/// Check if a file extension is a video format
fn is_video_extension(ext: &str) -> bool {
    matches!(
        ext,
        "mp4"
            | "mkv"
            | "avi"
            | "mov"
            | "wmv"
            | "flv"
            | "webm"
            | "m4v"
            | "mpg"
            | "mpeg"
            | "3gp"
            | "ogv"
            | "ts"
            | "m2ts"
            | "mts"
            | "vob"
            | "divx"
            | "xvid"
            | "rmvb"
            | "rm"
            | "asf"
    )
}

/// Check if a folder name indicates a season folder
fn is_season_folder(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.starts_with("season")
        || lower.starts_with("s0")
        || lower.starts_with("s1")
        || lower.starts_with("series")
        || lower == "specials"
}
