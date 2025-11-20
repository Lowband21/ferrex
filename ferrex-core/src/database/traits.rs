use crate::{
    auth::device::{AuthenticatedDevice, DeviceUpdateParams},
    error::Result,
    image::{
        MediaImageKind,
        records::{MediaImageVariantKey, MediaImageVariantRecord},
    },
    query::types::{MediaQuery, MediaWithStatus},
    rbac::{Permission, Role, UserPermissions},
    sync_session::{Participant, PlaybackState, SyncSession},
    types::{
        details::LibraryReference,
        files::{MediaFile, MediaFileMetadata},
        ids::{EpisodeID, LibraryID, MovieID, SeasonID, SeriesID},
        library::{Library, LibraryType},
        media::{EpisodeReference, Media, MovieReference, SeasonReference, SeriesReference},
    },
    user::{User, UserSession},
    watch_status::{InProgressItem, UpdateProgressRequest, UserWatchState},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
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
    pub library_id: LibraryID,
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
    pub library_id: LibraryID,
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
    pub library_id: Option<LibraryID>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct ImageRecord {
    pub id: Uuid,
    pub tmdb_path: String,
    pub file_hash: Option<String>,
    pub file_size: Option<i32>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub format: Option<String>,
    #[rkyv(with = crate::rkyv_wrappers::DateTimeWrapper)]
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct ImageVariant {
    pub id: Uuid,
    pub image_id: Uuid,
    pub variant: String,
    pub file_path: String,
    pub file_size: i32,
    pub width: Option<i32>,
    pub height: Option<i32>,
    #[rkyv(with = crate::rkyv_wrappers::DateTimeWrapper)]
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[rkyv(with = crate::rkyv_wrappers::OptionDateTime)]
    pub downloaded_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaImage {
    pub media_type: String,
    pub media_id: Uuid,
    pub image_id: Uuid,
    pub image_type: MediaImageKind,
    pub order_index: i32,
    pub is_primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct ImageLookupParams {
    pub media_type: String,
    pub media_id: String,
    pub image_type: MediaImageKind,
    pub index: u32,
    pub variant: Option<String>,
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
    pub library_id: LibraryID,
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
    pub library_id: Option<LibraryID>,
    pub processing_status: Option<FolderProcessingStatus>,
    pub folder_type: Option<FolderType>,
    pub max_attempts: Option<i32>,
    pub stale_after_hours: Option<i32>,
    pub limit: Option<i32>,
    pub priority: Option<ScanPriority>,
    pub max_batch_size: Option<i32>,
    pub error_retry_threshold: Option<i32>,
}

#[async_trait]
pub trait MediaDatabaseTrait: Send + Sync {
    /// Get self as Any for downcasting
    fn as_any(&self) -> &dyn std::any::Any;

    async fn initialize_schema(&self) -> Result<()>;

    // Generic store media? Shouldn't we store the media file with the associated library?
    async fn store_media(&self, media_file: MediaFile) -> Result<Uuid>;
    async fn store_media_batch(&self, media_files: Vec<MediaFile>) -> Result<Vec<Uuid>>;
    async fn get_media(&self, id: &Uuid) -> Result<Option<MediaFile>>;
    async fn get_media_by_path(&self, path: &str) -> Result<Option<MediaFile>>;
    async fn list_media(&self, filters: MediaFilters) -> Result<Vec<MediaFile>>;
    async fn get_stats(&self) -> Result<MediaStats>;
    async fn file_exists(&self, path: &str) -> Result<bool>;
    async fn delete_media(&self, id: &str) -> Result<()>;
    async fn get_all_media(&self) -> Result<Vec<MediaFile>>;

    async fn store_external_metadata(
        &self,
        media_id: &str,
        metadata: &MediaFileMetadata,
    ) -> Result<()>;
    async fn store_tv_show(&self, show_info: &TvShowInfo) -> Result<String>;
    async fn get_tv_show(&self, tmdb_id: &str) -> Result<Option<TvShowInfo>>;
    async fn link_episode_to_file(
        &self,
        media_file_id: &str,
        show_tmdb_id: &str,
        season: i32,
        episode: i32,
    ) -> Result<()>;

    // Library management methods
    async fn create_library(&self, library: Library) -> Result<String>;
    async fn get_library(&self, id: &LibraryID) -> Result<Option<Library>>;
    async fn list_libraries(&self) -> Result<Vec<Library>>;
    async fn update_library(&self, id: &str, library: Library) -> Result<()>;
    async fn delete_library(&self, id: &str) -> Result<()>;
    async fn update_library_last_scan(&self, id: &LibraryID) -> Result<()>;

    // New reference type methods
    async fn store_movie_reference(&self, movie: &MovieReference) -> Result<()>;
    async fn store_series_reference(&self, series: &SeriesReference) -> Result<()>;
    async fn store_season_reference(&self, season: &SeasonReference) -> Result<Uuid>;

    // Series lookup methods
    async fn get_series_by_tmdb_id(
        &self,
        library_id: LibraryID,
        tmdb_id: u64,
    ) -> Result<Option<SeriesReference>>;
    async fn find_series_by_name(
        &self,
        library_id: LibraryID,
        name: &str,
    ) -> Result<Option<SeriesReference>>;
    async fn store_episode_reference(&self, episode: &EpisodeReference) -> Result<()>;

    async fn get_all_movie_references(&self) -> Result<Vec<MovieReference>>;
    async fn get_series_references(&self) -> Result<Vec<SeriesReference>>;
    async fn get_series_seasons(&self, series_id: &SeriesID) -> Result<Vec<SeasonReference>>;
    async fn get_season_episodes(&self, season_id: &SeasonID) -> Result<Vec<EpisodeReference>>;

    // Individual reference retrieval
    async fn get_movie_reference(&self, id: &MovieID) -> Result<MovieReference>;
    async fn get_series_reference(&self, id: &SeriesID) -> Result<SeriesReference>;
    async fn get_season_reference(&self, id: &SeasonID) -> Result<SeasonReference>;
    async fn get_episode_reference(&self, id: &EpisodeID) -> Result<EpisodeReference>;

    // Bulk reference retrieval methods for performance
    async fn get_library_media_references(
        &self,
        library_id: LibraryID,
        library_type: LibraryType,
    ) -> Result<Vec<Media>>;
    async fn get_library_series(&self, library_id: &LibraryID) -> Result<Vec<SeriesReference>>;
    async fn get_library_seasons(&self, library_id: &LibraryID) -> Result<Vec<SeasonReference>>;
    async fn get_library_episodes(&self, library_id: &LibraryID) -> Result<Vec<EpisodeReference>>;
    async fn get_movie_references_bulk(&self, ids: &[&MovieID]) -> Result<Vec<MovieReference>>;

    /// Lookup a movie reference by its media file path
    async fn get_movie_reference_by_path(&self, path: &str) -> Result<Option<MovieReference>>;
    async fn get_series_references_bulk(&self, ids: &[&SeriesID]) -> Result<Vec<SeriesReference>>;
    async fn get_season_references_bulk(&self, ids: &[&SeasonID]) -> Result<Vec<SeasonReference>>;
    async fn get_episode_references_bulk(
        &self,
        ids: &[&EpisodeID],
    ) -> Result<Vec<EpisodeReference>>;

    // TMDB ID updates
    async fn update_movie_tmdb_id(&self, id: &MovieID, tmdb_id: u64) -> Result<()>;
    async fn update_series_tmdb_id(&self, id: &SeriesID, tmdb_id: u64) -> Result<()>;

    // Library management
    async fn list_library_references(&self) -> Result<Vec<LibraryReference>>;
    async fn get_library_reference(&self, id: Uuid) -> Result<LibraryReference>;

    // Image management methods
    async fn create_image(&self, tmdb_path: &str) -> Result<ImageRecord>;
    async fn get_image_by_tmdb_path(&self, tmdb_path: &str) -> Result<Option<ImageRecord>>;
    async fn get_image_by_hash(&self, hash: &str) -> Result<Option<ImageRecord>>;
    async fn update_image_metadata(
        &self,
        image_id: Uuid,
        hash: &str,
        size: i32,
        width: i32,
        height: i32,
        format: &str,
    ) -> Result<()>;

    // Image variant management
    async fn create_image_variant(
        &self,
        image_id: Uuid,
        variant: &str,
        file_path: &str,
        size: i32,
        width: Option<i32>,
        height: Option<i32>,
    ) -> Result<ImageVariant>;
    async fn get_image_variant(
        &self,
        image_id: Uuid,
        variant: &str,
    ) -> Result<Option<ImageVariant>>;
    async fn get_image_variants(&self, image_id: Uuid) -> Result<Vec<ImageVariant>>;

    // Media image linking
    async fn link_media_image(
        &self,
        media_type: &str,
        media_id: Uuid,
        image_id: Uuid,
        image_type: MediaImageKind,
        order_index: i32,
        is_primary: bool,
    ) -> Result<()>;
    async fn get_media_images(&self, media_type: &str, media_id: Uuid) -> Result<Vec<MediaImage>>;
    async fn get_media_primary_image(
        &self,
        media_type: &str,
        media_id: Uuid,
        image_type: MediaImageKind,
    ) -> Result<Option<MediaImage>>;

    // Combined lookup for image serving
    async fn lookup_image_variant(
        &self,
        params: &ImageLookupParams,
    ) -> Result<Option<(ImageRecord, Option<ImageVariant>)>>;

    async fn upsert_media_image_variant(
        &self,
        record: &MediaImageVariantRecord,
    ) -> Result<MediaImageVariantRecord>;
    async fn mark_media_image_variant_cached(
        &self,
        key: &MediaImageVariantKey,
        width: Option<i32>,
        height: Option<i32>,
        content_hash: Option<&str>,
        theme_color: Option<&str>,
    ) -> Result<MediaImageVariantRecord>;
    async fn list_media_image_variants(
        &self,
        media_type: &str,
        media_id: Uuid,
    ) -> Result<Vec<MediaImageVariantRecord>>;

    async fn update_media_theme_color(
        &self,
        media_type: &str,
        media_id: Uuid,
        theme_color: Option<&str>,
    ) -> Result<()>;

    // Cleanup and maintenance
    async fn cleanup_orphaned_images(&self) -> Result<u32>;
    async fn get_image_cache_stats(&self) -> Result<HashMap<String, u64>>;

    // Scan state management
    async fn create_scan_state(&self, scan_state: &ScanState) -> Result<()>;
    async fn update_scan_state(&self, scan_state: &ScanState) -> Result<()>;
    async fn get_scan_state(&self, id: Uuid) -> Result<Option<ScanState>>;
    async fn get_active_scans(&self, library_id: Option<Uuid>) -> Result<Vec<ScanState>>;
    async fn get_latest_scan(
        &self,
        library_id: LibraryID,
        scan_type: ScanType,
    ) -> Result<Option<ScanState>>;

    // Media processing status
    async fn create_or_update_processing_status(
        &self,
        status: &MediaProcessingStatus,
    ) -> Result<()>;
    async fn get_processing_status(
        &self,
        media_file_id: Uuid,
    ) -> Result<Option<MediaProcessingStatus>>;
    async fn get_unprocessed_files(
        &self,
        library_id: LibraryID,
        status_type: &str,
        limit: i32,
    ) -> Result<Vec<MediaFile>>;
    async fn get_failed_files(
        &self,
        library_id: LibraryID,
        max_retries: i32,
    ) -> Result<Vec<MediaFile>>;
    async fn reset_processing_status(&self, media_file_id: Uuid) -> Result<()>;

    // File watch events
    async fn create_file_watch_event(&self, event: &FileWatchEvent) -> Result<()>;
    async fn get_unprocessed_events(
        &self,
        library_id: LibraryID,
        limit: i32,
    ) -> Result<Vec<FileWatchEvent>>;
    async fn mark_event_processed(&self, event_id: Uuid) -> Result<()>;
    async fn cleanup_old_events(&self, days_to_keep: i32) -> Result<u32>;

    // User management methods
    async fn create_user(&self, user: &User) -> Result<()>;
    async fn get_user_by_id(&self, id: Uuid) -> Result<Option<User>>;
    async fn get_user_by_username(&self, username: &str) -> Result<Option<User>>;
    async fn get_all_users(&self) -> Result<Vec<User>>;
    async fn update_user(&self, user: &User) -> Result<()>;
    async fn delete_user(&self, id: Uuid) -> Result<()>;

    // User password management
    async fn get_user_password_hash(&self, user_id: Uuid) -> Result<Option<String>>;
    async fn update_user_password(&self, user_id: Uuid, password_hash: &str) -> Result<()>;

    // Atomic user operations
    async fn delete_user_atomic(&self, user_id: Uuid, check_last_admin: bool) -> Result<()>;

    // RBAC methods
    async fn get_user_permissions(&self, user_id: Uuid) -> Result<UserPermissions>;
    async fn get_all_roles(&self) -> Result<Vec<Role>>;
    async fn get_all_permissions(&self) -> Result<Vec<Permission>>;
    async fn assign_user_role(&self, user_id: Uuid, role_id: Uuid, granted_by: Uuid) -> Result<()>;
    async fn remove_user_role(&self, user_id: Uuid, role_id: Uuid) -> Result<()>;
    async fn remove_user_role_atomic(
        &self,
        user_id: Uuid,
        role_id: Uuid,
        check_last_admin: bool,
    ) -> Result<()>;
    async fn override_user_permission(
        &self,
        user_id: Uuid,
        permission: &str,
        granted: bool,
        granted_by: Uuid,
        reason: Option<String>,
    ) -> Result<()>;

    // RBAC query operations
    async fn get_admin_count(&self, exclude_user_id: Option<Uuid>) -> Result<usize>;
    async fn user_has_role(&self, user_id: Uuid, role_name: &str) -> Result<bool>;
    async fn get_users_with_role(&self, role_name: &str) -> Result<Vec<Uuid>>;

    // Authentication methods
    async fn store_refresh_token(
        &self,
        token: &str,
        user_id: Uuid,
        device_name: Option<String>,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<()>;
    async fn get_refresh_token(
        &self,
        token: &str,
    ) -> Result<Option<(Uuid, chrono::DateTime<chrono::Utc>)>>;
    async fn delete_refresh_token(&self, token: &str) -> Result<()>;
    async fn delete_user_refresh_tokens(&self, user_id: Uuid) -> Result<()>;

    // Session management
    async fn create_session(&self, session: &UserSession) -> Result<()>;
    async fn get_user_sessions(&self, user_id: Uuid) -> Result<Vec<UserSession>>;
    async fn delete_session(&self, session_id: Uuid) -> Result<()>;

    // Watch status methods
    async fn update_watch_progress(
        &self,
        user_id: Uuid,
        progress: &UpdateProgressRequest,
    ) -> Result<()>;
    async fn get_user_watch_state(&self, user_id: Uuid) -> Result<UserWatchState>;
    async fn get_continue_watching(
        &self,
        user_id: Uuid,
        limit: usize,
    ) -> Result<Vec<InProgressItem>>;
    async fn clear_watch_progress(&self, user_id: Uuid, media_id: &Uuid) -> Result<()>;
    async fn is_media_completed(&self, user_id: Uuid, media_id: &Uuid) -> Result<bool>;

    // Sync session methods
    async fn create_sync_session(&self, session: &SyncSession) -> Result<()>;
    async fn get_sync_session_by_code(&self, room_code: &str) -> Result<Option<SyncSession>>;
    async fn get_sync_session(&self, id: Uuid) -> Result<Option<SyncSession>>;
    async fn update_sync_session_state(&self, id: Uuid, state: &PlaybackState) -> Result<()>;
    async fn update_sync_session(&self, id: Uuid, session: &SyncSession) -> Result<()>;
    async fn add_sync_participant(&self, session_id: Uuid, participant: &Participant)
    -> Result<()>;
    async fn remove_sync_participant(&self, session_id: Uuid, user_id: Uuid) -> Result<()>;
    async fn delete_sync_session(&self, id: Uuid) -> Result<()>;
    async fn end_sync_session(&self, id: Uuid) -> Result<()>;
    async fn cleanup_expired_sync_sessions(&self) -> Result<u32>;

    // Query system
    async fn query_media(&self, query: &MediaQuery) -> Result<Vec<MediaWithStatus>>;

    // Device authentication methods (legacy: removed in favor of auth domain repositories)

    /// Update failed login attempts
    async fn update_device_failed_attempts(
        &self,
        user_id: Uuid,
        device_id: Uuid,
        attempts: i32,
        locked_until: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<()>;

    // Kept intentionally empty. Use auth/domain repositories for device trust,
    // sessions, and audit events.

    // Folder inventory management methods
    /// Get folders that need scanning based on filters
    async fn get_folders_needing_scan(
        &self,
        filters: &FolderScanFilters,
    ) -> Result<Vec<FolderInventory>>;

    /// Update folder processing status
    async fn update_folder_status(
        &self,
        folder_id: Uuid,
        status: FolderProcessingStatus,
        error: Option<String>,
    ) -> Result<()>;

    /// Record a folder scan error and update retry information
    async fn record_folder_scan_error(
        &self,
        folder_id: Uuid,
        error: &str,
        next_retry: Option<DateTime<Utc>>,
    ) -> Result<()>;

    /// Get complete folder inventory for a library
    async fn get_folder_inventory(&self, library_id: LibraryID) -> Result<Vec<FolderInventory>>;

    /// Upsert a folder (insert or update if exists)
    async fn upsert_folder(&self, folder: &FolderInventory) -> Result<Uuid>;

    /// Cleanup stale folders that haven't been seen in the specified time
    async fn cleanup_stale_folders(
        &self,
        library_id: LibraryID,
        stale_after_hours: i32,
    ) -> Result<u32>;

    /// Get folder by path
    async fn get_folder_by_path(
        &self,
        library_id: LibraryID,
        path: &Path,
    ) -> Result<Option<FolderInventory>>;

    /// Update folder content statistics
    async fn update_folder_stats(
        &self,
        folder_id: Uuid,
        total_files: i32,
        processed_files: i32,
        total_size_bytes: i64,
        file_types: Vec<String>,
    ) -> Result<()>;

    /// Mark folder as processed
    async fn mark_folder_processed(&self, folder_id: Uuid) -> Result<()>;

    /// Get child folders of a parent folder
    async fn get_child_folders(&self, parent_folder_id: Uuid) -> Result<Vec<FolderInventory>>;

    /// Get season folders under a series folder
    /// This queries folder_inventory table for all folders where parent_folder_id matches
    /// the series folder and folder_type is 'season'
    async fn get_season_folders(&self, parent_folder_id: Uuid) -> Result<Vec<FolderInventory>>;
}
