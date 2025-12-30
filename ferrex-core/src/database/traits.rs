use crate::types::ids::LibraryId;
use chrono::{DateTime, Utc};
use ferrex_model::{ImageMediaType, ImageSize, MediaID};
#[cfg(feature = "rkyv")]
use rkyv::{
    Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// Scan state types
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ScanType {
    Full,
    Incremental,
    RefreshMetadata,
    Analyze,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ScanStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanState {
    pub id: Uuid,
    pub library_id: LibraryId,
    pub scan_type: ScanType,
    pub status: ScanStatus,
    pub total_folders: i32,
    pub processed_folders: i32,
    pub total_files: i32,
    pub processed_files: i32,
    pub current_path: Option<String>,
    pub error_count: i32,
    pub errors: Vec<String>,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub options: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaProcessingStatus {
    pub media_file_id: Uuid,
    pub metadata_extracted: bool,
    pub metadata_extracted_at: Option<DateTime<Utc>>,
    pub tmdb_matched: bool,
    pub tmdb_matched_at: Option<DateTime<Utc>>,
    pub images_cached: bool,
    pub images_cached_at: Option<DateTime<Utc>>,
    pub file_analyzed: bool,
    pub file_analyzed_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub error_details: Option<serde_json::Value>,
    pub retry_count: i32,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FileWatchEventType {
    Created,
    Modified,
    Deleted,
    Moved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileWatchEvent {
    pub id: Uuid,
    pub library_id: LibraryId,
    pub event_type: FileWatchEventType,
    pub file_path: String,
    pub old_path: Option<String>,
    pub file_size: Option<i64>,
    pub detected_at: DateTime<Utc>,
    pub processed: bool,
    pub processed_at: Option<DateTime<Utc>>,
    pub processing_attempts: i32,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct MediaFilters {
    pub media_type: Option<String>,
    pub show_name: Option<String>,
    pub season: Option<u32>,
    pub order_by: Option<String>,
    pub limit: Option<u64>,
    pub library_id: Option<LibraryId>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MediaStats {
    pub total_files: u64,
    pub total_size: u64,
    pub by_type: HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TvShowInfo {
    pub id: Uuid,
    pub tmdb_id: String,
    pub name: String,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub seasons: Vec<SeasonInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeasonInfo {
    pub id: Uuid,
    pub season_number: i32,
    pub name: Option<String>,
    pub episode_count: i32,
    pub poster_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeInfo {
    pub id: Uuid,
    pub episode_number: i32,
    pub name: Option<String>,
    pub overview: Option<String>,
    pub air_date: Option<chrono::NaiveDate>,
    pub still_path: Option<String>,
    pub media_file_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "rkyv", derive(Archive, RkyvSerialize, RkyvDeserialize))]
pub struct ImageRecord {
    pub iid: Uuid,
    pub imz: ImageSize,
    pub theme_color: String,
    pub dimensions: (u32, u32),
    pub cache_key: String,
    pub integrity: String,
    pub byte_len: i32,
    #[cfg_attr(
        feature = "rkyv",
        rkyv(with = crate::rkyv_wrappers::DateTimeWrapper)
    )]
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[cfg_attr(
        feature = "rkyv",
        rkyv(with = crate::rkyv_wrappers::DateTimeWrapper)
    )]
    pub modified_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "rkyv", derive(Archive, RkyvSerialize, RkyvDeserialize))]
pub struct OriginalImage {
    pub iid: Uuid,
    pub media_id: Uuid,
    pub media_type: ImageMediaType,
    pub tmdb_path: String,
    pub imz: ImageSize,
    pub iso_lang: String,
    pub vote_avg: f32,
    pub vote_cnt: u32,
    pub is_primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "rkyv", derive(Archive, RkyvSerialize, RkyvDeserialize))]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct ImageLookupParams {
    pub id: MediaID,
    pub iid: Uuid,
    pub imz: ImageSize,
    pub is_primary: bool,
}

// Folder inventory types
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FolderType {
    Root,
    Movie,
    TvShow,
    Season,
    Extra,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FolderProcessingStatus {
    Pending,
    Processing,
    Completed,
    Failed,
    Skipped,
    Queued,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FolderDiscoverySource {
    Scan,
    Watch,
    Manual,
    Import,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ScanPriority {
    Unscanned, // Never been scanned
    Changed,   // Files have changed
    Routine,   // Regular rescan
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderInventory {
    pub id: Uuid,
    pub library_id: LibraryId,
    pub folder_path: String,
    pub folder_type: FolderType,
    pub parent_folder_id: Option<Uuid>,

    // Discovery tracking
    pub discovered_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub discovery_source: FolderDiscoverySource,

    // Processing status
    pub processing_status: FolderProcessingStatus,
    pub last_processed_at: Option<DateTime<Utc>>,
    pub processing_error: Option<String>,
    pub processing_attempts: i32,
    pub next_retry_at: Option<DateTime<Utc>>,

    // Content tracking
    pub total_files: i32,
    pub processed_files: i32,
    pub total_size_bytes: i64,
    pub file_types: Vec<String>,
    pub last_modified: Option<DateTime<Utc>>,

    // Metadata
    pub metadata: serde_json::Value,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct FolderScanFilters {
    pub library_id: Option<LibraryId>,
    pub processing_status: Option<FolderProcessingStatus>,
    pub folder_type: Option<FolderType>,
    pub max_attempts: Option<i32>,
    pub stale_after_hours: Option<i32>,
    pub limit: Option<i32>,
    pub priority: Option<ScanPriority>,
    pub max_batch_size: Option<i32>,
    pub error_retry_threshold: Option<i32>,
}
