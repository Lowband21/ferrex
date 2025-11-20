use std::{
    collections::HashMap,
    convert::Infallible,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use axum_test::TestServer;
use chrono::{DateTime, Utc};
use ferrex_core::ImageService;
use ferrex_core::database::traits::{
    FileWatchEvent, FolderInventory, FolderProcessingStatus, FolderScanFilters, ImageLookupParams,
    ImageRecord, ImageVariant, MediaDatabaseTrait, MediaFilters, MediaImage, MediaProcessingStatus,
    MediaStats, ScanState, ScanType, TvShowInfo,
};
use ferrex_core::image::records::{MediaImageVariantKey, MediaImageVariantRecord};
use ferrex_core::orchestration::budget::InMemoryBudget;
use ferrex_core::orchestration::config::OrchestratorConfig;
use ferrex_core::orchestration::events::{EventMeta, JobEvent, JobEventPayload};
use ferrex_core::orchestration::job::{JobId, JobKind, JobPriority};
use ferrex_core::orchestration::lease::LeaseId;
use ferrex_core::orchestration::persistence::{PostgresCursorRepository, PostgresQueueService};
use ferrex_core::orchestration::scan_cursor::normalize_path;
use ferrex_core::providers::TmdbApiProvider;
use ferrex_core::types::files::MediaFileMetadata;
use ferrex_core::{
    EpisodeID, EpisodeReference, FileSystemEvent, FileSystemEventKind, JobEventPublisher, Library,
    LibraryActorCommand, LibraryActorConfig, LibraryID, LibraryReference, LibraryRootsId,
    LibraryType, Media, MediaDatabase, MediaError, MediaFile, MovieID, MovieReference, Result,
    SeasonID, SeasonReference, SeriesID, SeriesReference,
};
use ferrex_server::infra::scan::scan_manager::{
    ScanBroadcastFrame, ScanControlPlane, ScanEventKind, ScanHistoryEntry, ScanLifecycleStatus,
};
use ferrex_server::{
    handlers::scan::handle_scan::build_scan_progress_stream, infra::orchestration::ScanOrchestrator,
};
use sqlx::PgPool;
use tempfile::TempDir;
use tokio::sync::{RwLock, broadcast, broadcast::error::RecvError};
use tokio::{
    task::yield_now,
    time::{sleep, timeout},
};
use uuid::Uuid;

type TestScanControlPlane = ScanControlPlane;

async fn ensure_library_row(pool: &PgPool, library: &Library) -> anyhow::Result<()> {
    let paths: Vec<String> = library
        .paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let library_type = match library.library_type {
        LibraryType::Movies => "movies",
        LibraryType::Series => "tvshows",
    };

    sqlx::query(
        r#"
        INSERT INTO libraries (
            id,
            name,
            library_type,
            paths,
            scan_interval_minutes,
            enabled,
            auto_scan,
            watch_for_changes,
            analyze_on_scan,
            max_retry_attempts
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        ON CONFLICT (id) DO UPDATE SET
            name = EXCLUDED.name,
            paths = EXCLUDED.paths,
            scan_interval_minutes = EXCLUDED.scan_interval_minutes,
            enabled = EXCLUDED.enabled,
            auto_scan = EXCLUDED.auto_scan,
            watch_for_changes = EXCLUDED.watch_for_changes,
            analyze_on_scan = EXCLUDED.analyze_on_scan,
            max_retry_attempts = EXCLUDED.max_retry_attempts,
            updated_at = NOW()
        "#,
    )
    .bind(library.id.as_uuid())
    .bind(library.name.as_str())
    .bind(library_type)
    .bind(paths)
    .bind(library.scan_interval_minutes as i32)
    .bind(library.enabled)
    .bind(library.auto_scan)
    .bind(library.watch_for_changes)
    .bind(library.analyze_on_scan)
    .bind(library.max_retry_attempts as i32)
    .execute(pool)
    .await?;

    Ok(())
}

struct TestMediaBackend {
    libraries: RwLock<HashMap<LibraryID, Library>>,
}

impl TestMediaBackend {
    fn new(libraries: Vec<Library>) -> Self {
        let mut map = HashMap::new();
        for library in libraries {
            map.insert(library.id, library);
        }
        Self {
            libraries: RwLock::new(map),
        }
    }

    fn unsupported<T>(&self, method: &str) -> Result<T> {
        Err(MediaError::Internal(format!(
            "{} not supported in TestMediaBackend",
            method
        )))
    }
}

#[async_trait]
impl MediaDatabaseTrait for TestMediaBackend {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn initialize_schema(&self) -> Result<()> {
        Ok(())
    }

    async fn get_library(&self, id: &LibraryID) -> Result<Option<Library>> {
        let guard = self.libraries.read().await;
        Ok(guard.get(id).cloned())
    }

    async fn list_libraries(&self) -> Result<Vec<Library>> {
        let guard = self.libraries.read().await;
        Ok(guard.values().cloned().collect())
    }

    async fn store_media(&self, media_file: MediaFile) -> Result<Uuid> {
        self.unsupported("store_media")
    }
    async fn store_media_batch(&self, media_files: Vec<MediaFile>) -> Result<Vec<Uuid>> {
        self.unsupported("store_media_batch")
    }
    async fn get_media(&self, id: &Uuid) -> Result<Option<MediaFile>> {
        self.unsupported("get_media")
    }
    async fn get_media_by_path(&self, path: &str) -> Result<Option<MediaFile>> {
        self.unsupported("get_media_by_path")
    }
    async fn list_media(&self, filters: MediaFilters) -> Result<Vec<MediaFile>> {
        self.unsupported("list_media")
    }
    async fn get_stats(&self) -> Result<MediaStats> {
        self.unsupported("get_stats")
    }
    async fn file_exists(&self, path: &str) -> Result<bool> {
        self.unsupported("file_exists")
    }
    async fn delete_media(&self, id: &str) -> Result<()> {
        self.unsupported("delete_media")
    }
    async fn get_all_media(&self) -> Result<Vec<MediaFile>> {
        self.unsupported("get_all_media")
    }
    async fn store_external_metadata(
        &self,
        media_id: &str,
        metadata: &MediaFileMetadata,
    ) -> Result<()> {
        self.unsupported("store_external_metadata")
    }
    async fn store_tv_show(&self, show_info: &TvShowInfo) -> Result<String> {
        self.unsupported("store_tv_show")
    }
    async fn get_tv_show(&self, tmdb_id: &str) -> Result<Option<TvShowInfo>> {
        self.unsupported("get_tv_show")
    }
    async fn link_episode_to_file(
        &self,
        media_file_id: &str,
        show_tmdb_id: &str,
        season: i32,
        episode: i32,
    ) -> Result<()> {
        self.unsupported("link_episode_to_file")
    }
    async fn create_library(&self, library: Library) -> Result<String> {
        self.unsupported("create_library")
    }
    async fn update_library(&self, id: &str, library: Library) -> Result<()> {
        self.unsupported("update_library")
    }
    async fn delete_library(&self, id: &str) -> Result<()> {
        self.unsupported("delete_library")
    }
    async fn update_library_last_scan(&self, id: &LibraryID) -> Result<()> {
        self.unsupported("update_library_last_scan")
    }
    async fn store_movie_reference(&self, movie: &MovieReference) -> Result<()> {
        self.unsupported("store_movie_reference")
    }
    async fn store_series_reference(&self, series: &SeriesReference) -> Result<()> {
        self.unsupported("store_series_reference")
    }
    async fn store_season_reference(&self, season: &SeasonReference) -> Result<Uuid> {
        self.unsupported("store_season_reference")
    }
    async fn get_series_by_tmdb_id(
        &self,
        library_id: LibraryID,
        tmdb_id: u64,
    ) -> Result<Option<SeriesReference>> {
        self.unsupported("get_series_by_tmdb_id")
    }
    async fn find_series_by_name(
        &self,
        library_id: LibraryID,
        name: &str,
    ) -> Result<Option<SeriesReference>> {
        self.unsupported("find_series_by_name")
    }
    async fn store_episode_reference(&self, episode: &EpisodeReference) -> Result<()> {
        self.unsupported("store_episode_reference")
    }
    async fn get_all_movie_references(&self) -> Result<Vec<MovieReference>> {
        self.unsupported("get_all_movie_references")
    }
    async fn get_series_references(&self) -> Result<Vec<SeriesReference>> {
        self.unsupported("get_series_references")
    }
    async fn get_series_seasons(&self, series_id: &SeriesID) -> Result<Vec<SeasonReference>> {
        self.unsupported("get_series_seasons")
    }
    async fn get_season_episodes(&self, season_id: &SeasonID) -> Result<Vec<EpisodeReference>> {
        self.unsupported("get_season_episodes")
    }
    async fn get_movie_reference(&self, id: &MovieID) -> Result<MovieReference> {
        self.unsupported("get_movie_reference")
    }
    async fn get_series_reference(&self, id: &SeriesID) -> Result<SeriesReference> {
        self.unsupported("get_series_reference")
    }
    async fn get_season_reference(&self, id: &SeasonID) -> Result<SeasonReference> {
        self.unsupported("get_season_reference")
    }
    async fn get_episode_reference(&self, id: &EpisodeID) -> Result<EpisodeReference> {
        self.unsupported("get_episode_reference")
    }
    async fn get_library_media_references(
        &self,
        library_id: LibraryID,
        library_type: LibraryType,
    ) -> Result<Vec<Media>> {
        self.unsupported("get_library_media_references")
    }
    async fn get_library_series(&self, library_id: &LibraryID) -> Result<Vec<SeriesReference>> {
        self.unsupported("get_library_series")
    }
    async fn get_library_seasons(&self, library_id: &LibraryID) -> Result<Vec<SeasonReference>> {
        self.unsupported("get_library_seasons")
    }
    async fn get_library_episodes(&self, library_id: &LibraryID) -> Result<Vec<EpisodeReference>> {
        self.unsupported("get_library_episodes")
    }
    async fn get_movie_references_bulk(&self, ids: &[&MovieID]) -> Result<Vec<MovieReference>> {
        self.unsupported("get_movie_references_bulk")
    }
    async fn get_movie_reference_by_path(&self, path: &str) -> Result<Option<MovieReference>> {
        self.unsupported("get_movie_reference_by_path")
    }
    async fn get_series_references_bulk(&self, ids: &[&SeriesID]) -> Result<Vec<SeriesReference>> {
        self.unsupported("get_series_references_bulk")
    }
    async fn get_season_references_bulk(&self, ids: &[&SeasonID]) -> Result<Vec<SeasonReference>> {
        self.unsupported("get_season_references_bulk")
    }
    async fn get_episode_references_bulk(
        &self,
        ids: &[&EpisodeID],
    ) -> Result<Vec<EpisodeReference>> {
        self.unsupported("get_episode_references_bulk")
    }
    async fn update_movie_tmdb_id(&self, id: &MovieID, tmdb_id: u64) -> Result<()> {
        self.unsupported("update_movie_tmdb_id")
    }
    async fn update_series_tmdb_id(&self, id: &SeriesID, tmdb_id: u64) -> Result<()> {
        self.unsupported("update_series_tmdb_id")
    }
    async fn list_library_references(&self) -> Result<Vec<LibraryReference>> {
        self.unsupported("list_library_references")
    }
    async fn get_library_reference(&self, id: Uuid) -> Result<LibraryReference> {
        self.unsupported("get_library_reference")
    }
    async fn create_image(&self, tmdb_path: &str) -> Result<ImageRecord> {
        self.unsupported("create_image")
    }
    async fn get_image_by_tmdb_path(&self, tmdb_path: &str) -> Result<Option<ImageRecord>> {
        self.unsupported("get_image_by_tmdb_path")
    }
    async fn get_image_by_hash(&self, hash: &str) -> Result<Option<ImageRecord>> {
        self.unsupported("get_image_by_hash")
    }
    async fn update_image_metadata(
        &self,
        image_id: Uuid,
        hash: &str,
        size: i32,
        width: i32,
        height: i32,
        format: &str,
    ) -> Result<()> {
        self.unsupported("update_image_metadata")
    }
    async fn create_image_variant(
        &self,
        image_id: Uuid,
        variant: &str,
        file_path: &str,
        size: i32,
        width: Option<i32>,
        height: Option<i32>,
    ) -> Result<ImageVariant> {
        self.unsupported("create_image_variant")
    }
    async fn get_image_variant(
        &self,
        image_id: Uuid,
        variant: &str,
    ) -> Result<Option<ImageVariant>> {
        self.unsupported("get_image_variant")
    }
    async fn get_image_variants(&self, image_id: Uuid) -> Result<Vec<ImageVariant>> {
        self.unsupported("get_image_variants")
    }
    async fn link_media_image(
        &self,
        media_type: &str,
        media_id: Uuid,
        image_id: Uuid,
        image_type: &str,
        order_index: i32,
        is_primary: bool,
    ) -> Result<()> {
        self.unsupported("link_media_image")
    }
    async fn get_media_images(&self, media_type: &str, media_id: Uuid) -> Result<Vec<MediaImage>> {
        self.unsupported("get_media_images")
    }
    async fn get_media_primary_image(
        &self,
        media_type: &str,
        media_id: Uuid,
        image_type: &str,
    ) -> Result<Option<MediaImage>> {
        self.unsupported("get_media_primary_image")
    }
    async fn lookup_image_variant(
        &self,
        params: &ImageLookupParams,
    ) -> Result<Option<(ImageRecord, Option<ImageVariant>)>> {
        self.unsupported("lookup_image_variant")
    }

    async fn upsert_media_image_variant(
        &self,
        record: &MediaImageVariantRecord,
    ) -> Result<MediaImageVariantRecord> {
        let _ = record;
        self.unsupported("upsert_media_image_variant")
    }

    async fn mark_media_image_variant_cached(
        &self,
        key: &MediaImageVariantKey,
        width: Option<i32>,
        height: Option<i32>,
        content_hash: Option<&str>,
        theme_color: Option<&str>,
    ) -> Result<MediaImageVariantRecord> {
        let _ = (key, width, height, content_hash, theme_color);
        self.unsupported("mark_media_image_variant_cached")
    }

    async fn list_media_image_variants(
        &self,
        media_type: &str,
        media_id: Uuid,
    ) -> Result<Vec<MediaImageVariantRecord>> {
        let _ = (media_type, media_id);
        self.unsupported("list_media_image_variants")
    }

    async fn update_media_theme_color(
        &self,
        media_type: &str,
        media_id: Uuid,
        theme_color: Option<&str>,
    ) -> Result<()> {
        let _ = (media_type, media_id, theme_color);
        self.unsupported("update_media_theme_color")
    }
    async fn cleanup_orphaned_images(&self) -> Result<u32> {
        self.unsupported("cleanup_orphaned_images")
    }
    async fn get_image_cache_stats(&self) -> Result<HashMap<String, u64>> {
        self.unsupported("get_image_cache_stats")
    }
    async fn create_scan_state(&self, scan_state: &ScanState) -> Result<()> {
        self.unsupported("create_scan_state")
    }
    async fn update_scan_state(&self, scan_state: &ScanState) -> Result<()> {
        self.unsupported("update_scan_state")
    }
    async fn get_scan_state(&self, id: Uuid) -> Result<Option<ScanState>> {
        self.unsupported("get_scan_state")
    }
    async fn get_active_scans(&self, library_id: Option<Uuid>) -> Result<Vec<ScanState>> {
        self.unsupported("get_active_scans")
    }
    async fn get_latest_scan(
        &self,
        library_id: LibraryID,
        scan_type: ScanType,
    ) -> Result<Option<ScanState>> {
        self.unsupported("get_latest_scan")
    }
    async fn create_or_update_processing_status(
        &self,
        status: &MediaProcessingStatus,
    ) -> Result<()> {
        self.unsupported("create_or_update_processing_status")
    }
    async fn get_processing_status(
        &self,
        media_file_id: Uuid,
    ) -> Result<Option<MediaProcessingStatus>> {
        self.unsupported("get_processing_status")
    }
    async fn get_unprocessed_files(
        &self,
        library_id: LibraryID,
        status_type: &str,
        limit: i32,
    ) -> Result<Vec<MediaFile>> {
        self.unsupported("get_unprocessed_files")
    }
    async fn get_failed_files(
        &self,
        library_id: LibraryID,
        max_retries: i32,
    ) -> Result<Vec<MediaFile>> {
        self.unsupported("get_failed_files")
    }
    async fn reset_processing_status(&self, media_file_id: Uuid) -> Result<()> {
        self.unsupported("reset_processing_status")
    }
    async fn create_file_watch_event(&self, event: &FileWatchEvent) -> Result<()> {
        self.unsupported("create_file_watch_event")
    }
    async fn get_unprocessed_events(
        &self,
        library_id: LibraryID,
        limit: i32,
    ) -> Result<Vec<FileWatchEvent>> {
        self.unsupported("get_unprocessed_events")
    }
    async fn mark_event_processed(&self, event_id: Uuid) -> Result<()> {
        self.unsupported("mark_event_processed")
    }
    async fn cleanup_old_events(&self, days_to_keep: i32) -> Result<u32> {
        self.unsupported("cleanup_old_events")
    }
    async fn create_user(&self, user: &ferrex_core::User) -> Result<()> {
        self.unsupported("create_user")
    }
    async fn get_user_by_id(&self, id: Uuid) -> Result<Option<ferrex_core::User>> {
        self.unsupported("get_user_by_id")
    }
    async fn get_user_by_username(&self, username: &str) -> Result<Option<ferrex_core::User>> {
        self.unsupported("get_user_by_username")
    }
    async fn get_all_users(&self) -> Result<Vec<ferrex_core::User>> {
        self.unsupported("get_all_users")
    }
    async fn update_user(&self, user: &ferrex_core::User) -> Result<()> {
        self.unsupported("update_user")
    }
    async fn delete_user(&self, id: Uuid) -> Result<()> {
        self.unsupported("delete_user")
    }
    async fn get_user_password_hash(&self, user_id: Uuid) -> Result<Option<String>> {
        self.unsupported("get_user_password_hash")
    }
    async fn update_user_password(&self, user_id: Uuid, password_hash: &str) -> Result<()> {
        self.unsupported("update_user_password")
    }
    async fn delete_user_atomic(&self, user_id: Uuid, check_last_admin: bool) -> Result<()> {
        self.unsupported("delete_user_atomic")
    }
    async fn get_user_permissions(
        &self,
        user_id: Uuid,
    ) -> Result<ferrex_core::rbac::UserPermissions> {
        self.unsupported("get_user_permissions")
    }
    async fn get_all_roles(&self) -> Result<Vec<ferrex_core::rbac::Role>> {
        self.unsupported("get_all_roles")
    }
    async fn get_all_permissions(&self) -> Result<Vec<ferrex_core::rbac::Permission>> {
        self.unsupported("get_all_permissions")
    }
    async fn assign_user_role(&self, user_id: Uuid, role_id: Uuid, granted_by: Uuid) -> Result<()> {
        self.unsupported("assign_user_role")
    }
    async fn remove_user_role(&self, user_id: Uuid, role_id: Uuid) -> Result<()> {
        self.unsupported("remove_user_role")
    }
    async fn remove_user_role_atomic(
        &self,
        user_id: Uuid,
        role_id: Uuid,
        check_last_admin: bool,
    ) -> Result<()> {
        self.unsupported("remove_user_role_atomic")
    }
    async fn override_user_permission(
        &self,
        user_id: Uuid,
        permission: &str,
        granted: bool,
        granted_by: Uuid,
        reason: Option<String>,
    ) -> Result<()> {
        self.unsupported("override_user_permission")
    }
    async fn get_admin_count(&self, exclude_user_id: Option<Uuid>) -> Result<usize> {
        self.unsupported("get_admin_count")
    }
    async fn user_has_role(&self, user_id: Uuid, role_name: &str) -> Result<bool> {
        self.unsupported("user_has_role")
    }
    async fn get_users_with_role(&self, role_name: &str) -> Result<Vec<Uuid>> {
        self.unsupported("get_users_with_role")
    }
    async fn store_refresh_token(
        &self,
        token: &str,
        user_id: Uuid,
        device_name: Option<String>,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        self.unsupported("store_refresh_token")
    }
    async fn get_refresh_token(
        &self,
        token: &str,
    ) -> Result<Option<(Uuid, chrono::DateTime<chrono::Utc>)>> {
        self.unsupported("get_refresh_token")
    }
    async fn delete_refresh_token(&self, token: &str) -> Result<()> {
        self.unsupported("delete_refresh_token")
    }
    async fn delete_user_refresh_tokens(&self, user_id: Uuid) -> Result<()> {
        self.unsupported("delete_user_refresh_tokens")
    }
    async fn create_session(&self, session: &ferrex_core::UserSession) -> Result<()> {
        self.unsupported("create_session")
    }
    async fn get_user_sessions(&self, user_id: Uuid) -> Result<Vec<ferrex_core::UserSession>> {
        self.unsupported("get_user_sessions")
    }
    async fn delete_session(&self, session_id: Uuid) -> Result<()> {
        self.unsupported("delete_session")
    }
    async fn update_watch_progress(
        &self,
        user_id: Uuid,
        progress: &ferrex_core::UpdateProgressRequest,
    ) -> Result<()> {
        self.unsupported("update_watch_progress")
    }
    async fn get_user_watch_state(&self, user_id: Uuid) -> Result<ferrex_core::UserWatchState> {
        self.unsupported("get_user_watch_state")
    }
    async fn get_continue_watching(
        &self,
        user_id: Uuid,
        limit: usize,
    ) -> Result<Vec<ferrex_core::InProgressItem>> {
        self.unsupported("get_continue_watching")
    }
    async fn clear_watch_progress(&self, user_id: Uuid, media_id: &Uuid) -> Result<()> {
        self.unsupported("clear_watch_progress")
    }
    async fn is_media_completed(&self, user_id: Uuid, media_id: &Uuid) -> Result<bool> {
        self.unsupported("is_media_completed")
    }
    async fn create_sync_session(&self, session: &ferrex_core::SyncSession) -> Result<()> {
        self.unsupported("create_sync_session")
    }
    async fn get_sync_session_by_code(
        &self,
        room_code: &str,
    ) -> Result<Option<ferrex_core::SyncSession>> {
        self.unsupported("get_sync_session_by_code")
    }
    async fn get_sync_session(&self, id: Uuid) -> Result<Option<ferrex_core::SyncSession>> {
        self.unsupported("get_sync_session")
    }
    async fn update_sync_session_state(
        &self,
        id: Uuid,
        state: &ferrex_core::PlaybackState,
    ) -> Result<()> {
        self.unsupported("update_sync_session_state")
    }
    async fn update_sync_session(
        &self,
        id: Uuid,
        session: &ferrex_core::SyncSession,
    ) -> Result<()> {
        self.unsupported("update_sync_session")
    }
    async fn add_sync_participant(
        &self,
        session_id: Uuid,
        participant: &ferrex_core::Participant,
    ) -> Result<()> {
        self.unsupported("add_sync_participant")
    }
    async fn remove_sync_participant(&self, session_id: Uuid, user_id: Uuid) -> Result<()> {
        self.unsupported("remove_sync_participant")
    }
    async fn delete_sync_session(&self, id: Uuid) -> Result<()> {
        self.unsupported("delete_sync_session")
    }
    async fn end_sync_session(&self, id: Uuid) -> Result<()> {
        self.unsupported("end_sync_session")
    }
    async fn cleanup_expired_sync_sessions(&self) -> Result<u32> {
        self.unsupported("cleanup_expired_sync_sessions")
    }
    async fn query_media(
        &self,
        query: &ferrex_core::query::MediaQuery,
    ) -> Result<Vec<ferrex_core::query::MediaWithStatus>> {
        self.unsupported("query_media")
    }
    async fn register_device(&self, device: &ferrex_core::auth::AuthenticatedDevice) -> Result<()> {
        self.unsupported("register_device")
    }
    async fn get_device_by_fingerprint(
        &self,
        fingerprint: &str,
    ) -> Result<Option<ferrex_core::auth::AuthenticatedDevice>> {
        self.unsupported("get_device_by_fingerprint")
    }
    async fn get_device_by_id(
        &self,
        device_id: Uuid,
    ) -> Result<Option<ferrex_core::auth::AuthenticatedDevice>> {
        self.unsupported("get_device_by_id")
    }
    async fn get_user_devices(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<ferrex_core::auth::AuthenticatedDevice>> {
        self.unsupported("get_user_devices")
    }
    async fn update_device(
        &self,
        device_id: Uuid,
        updates: &ferrex_core::auth::DeviceUpdateParams,
    ) -> Result<()> {
        self.unsupported("update_device")
    }
    async fn revoke_device(&self, device_id: Uuid, revoked_by: Uuid) -> Result<()> {
        self.unsupported("revoke_device")
    }
    async fn upsert_device_credential(
        &self,
        credential: &ferrex_core::auth::DeviceUserCredential,
    ) -> Result<()> {
        self.unsupported("upsert_device_credential")
    }
    async fn get_device_credential(
        &self,
        user_id: Uuid,
        device_id: Uuid,
    ) -> Result<Option<ferrex_core::auth::DeviceUserCredential>> {
        self.unsupported("get_device_credential")
    }
    async fn update_device_pin(
        &self,
        user_id: Uuid,
        device_id: Uuid,
        pin_hash: &str,
    ) -> Result<()> {
        self.unsupported("update_device_pin")
    }
    async fn update_device_failed_attempts(
        &self,
        user_id: Uuid,
        device_id: Uuid,
        attempts: i32,
        locked_until: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<()> {
        self.unsupported("update_device_failed_attempts")
    }
    async fn create_device_session(
        &self,
        session: &ferrex_core::auth::SessionDeviceSession,
    ) -> Result<()> {
        self.unsupported("create_device_session")
    }
    async fn get_device_sessions(
        &self,
        device_id: Uuid,
    ) -> Result<Vec<ferrex_core::auth::SessionDeviceSession>> {
        self.unsupported("get_device_sessions")
    }
    async fn revoke_device_sessions(&self, device_id: Uuid) -> Result<()> {
        self.unsupported("revoke_device_sessions")
    }
    async fn log_auth_event(&self, event: &ferrex_core::auth::AuthEvent) -> Result<()> {
        self.unsupported("log_auth_event")
    }
    async fn get_user_auth_events(
        &self,
        user_id: Uuid,
        limit: usize,
    ) -> Result<Vec<ferrex_core::auth::AuthEvent>> {
        self.unsupported("get_user_auth_events")
    }
    async fn get_device_auth_events(
        &self,
        device_id: Uuid,
        limit: usize,
    ) -> Result<Vec<ferrex_core::auth::AuthEvent>> {
        self.unsupported("get_device_auth_events")
    }
    async fn get_folders_needing_scan(
        &self,
        filters: &FolderScanFilters,
    ) -> Result<Vec<FolderInventory>> {
        self.unsupported("get_folders_needing_scan")
    }
    async fn update_folder_status(
        &self,
        folder_id: Uuid,
        status: FolderProcessingStatus,
        error: Option<String>,
    ) -> Result<()> {
        self.unsupported("update_folder_status")
    }
    async fn record_folder_scan_error(
        &self,
        folder_id: Uuid,
        error: &str,
        next_retry: Option<DateTime<Utc>>,
    ) -> Result<()> {
        self.unsupported("record_folder_scan_error")
    }
    async fn get_folder_inventory(&self, library_id: LibraryID) -> Result<Vec<FolderInventory>> {
        self.unsupported("get_folder_inventory")
    }
    async fn upsert_folder(&self, folder: &FolderInventory) -> Result<Uuid> {
        self.unsupported("upsert_folder")
    }
    async fn cleanup_stale_folders(
        &self,
        library_id: LibraryID,
        stale_after_hours: i32,
    ) -> Result<u32> {
        self.unsupported("cleanup_stale_folders")
    }
    async fn get_folder_by_path(
        &self,
        library_id: LibraryID,
        path: &Path,
    ) -> Result<Option<FolderInventory>> {
        self.unsupported("get_folder_by_path")
    }
    async fn update_folder_stats(
        &self,
        folder_id: Uuid,
        total_files: i32,
        processed_files: i32,
        total_size_bytes: i64,
        file_types: Vec<String>,
    ) -> Result<()> {
        self.unsupported("update_folder_stats")
    }
    async fn mark_folder_processed(&self, folder_id: Uuid) -> Result<()> {
        self.unsupported("mark_folder_processed")
    }
    async fn get_child_folders(&self, parent_folder_id: Uuid) -> Result<Vec<FolderInventory>> {
        self.unsupported("get_child_folders")
    }
    async fn get_season_folders(&self, parent_folder_id: Uuid) -> Result<Vec<FolderInventory>> {
        self.unsupported("get_season_folders")
    }
}

fn library_actor_config(library: &Library) -> LibraryActorConfig {
    LibraryActorConfig {
        library: LibraryReference {
            id: library.id,
            name: library.name.clone(),
            library_type: library.library_type,
            paths: library.paths.clone(),
        },
        root_paths: library.paths.clone(),
        max_outstanding_jobs: 8,
    }
}

async fn setup_scan_control_with_quiescence(
    pool: PgPool,
    library: Library,
    quiescence: Duration,
) -> anyhow::Result<(
    Arc<TestScanControlPlane>,
    Arc<ScanOrchestrator>,
    TempDir,
    TempDir,
)> {
    let cache_dir = TempDir::new()?;
    let image_cache_dir = TempDir::new()?;

    // Ensure schema matches runtime expectations for queue/cursor operations.
    let backend =
        Arc::new(TestMediaBackend::new(vec![library.clone()])) as Arc<dyn MediaDatabaseTrait>;
    let db = Arc::new(MediaDatabase::with_backend(backend));

    let image_service = Arc::new(ImageService::new(
        db.clone(),
        image_cache_dir.path().to_path_buf(),
    ));

    let config = OrchestratorConfig::default();
    let queue = Arc::new(PostgresQueueService::new_with_retry(pool.clone(), config.retry).await?);
    let cursors = Arc::new(PostgresCursorRepository::new(pool.clone()));
    let budget = Arc::new(InMemoryBudget::new(config.budget.clone()));
    let orchestrator = Arc::new(ScanOrchestrator::new(
        config,
        db.clone(),
        Arc::new(TmdbApiProvider::new()),
        image_service,
        queue,
        cursors,
        budget,
    )?);

    ensure_library_row(&pool, &library).await?;

    orchestrator
        .register_library(library_actor_config(&library))
        .await?;
    orchestrator.start().await?;

    let scan_control = Arc::new(ScanControlPlane::with_quiescence_window(
        db,
        orchestrator.clone(),
        quiescence,
    ));

    Ok((scan_control, orchestrator, cache_dir, image_cache_dir))
}

async fn setup_scan_control(
    pool: PgPool,
    library: Library,
) -> anyhow::Result<(
    Arc<TestScanControlPlane>,
    Arc<ScanOrchestrator>,
    TempDir,
    TempDir,
)> {
    setup_scan_control_with_quiescence(pool, library, Duration::from_millis(500)).await
}

fn make_library(paths: Vec<PathBuf>) -> Library {
    let now = chrono::Utc::now();
    Library {
        id: LibraryID::new_uuid(),
        name: "Test Library".into(),
        library_type: LibraryType::Movies,
        paths,
        scan_interval_minutes: 60,
        last_scan: None,
        enabled: true,
        auto_scan: false,
        watch_for_changes: true,
        analyze_on_scan: false,
        max_retry_attempts: 3,
        created_at: now,
        updated_at: now,
        media: None,
    }
}

fn job_event(
    correlation_id: Uuid,
    library_id: LibraryID,
    idempotency_key: &str,
    path_key: &str,
    payload: JobEventPayload,
) -> JobEvent {
    JobEvent {
        meta: EventMeta::new(
            Some(correlation_id),
            library_id,
            idempotency_key.to_string(),
            Some(path_key.to_string()),
        ),
        payload,
    }
}

fn frame_label(frame: &ScanBroadcastFrame) -> &'static str {
    match frame.event {
        ScanEventKind::Started => "scan.started",
        ScanEventKind::Progress => "scan.progress",
        ScanEventKind::Quiescing => "scan.quiescing",
        ScanEventKind::Completed => "scan.completed",
        ScanEventKind::Failed => "scan.failed",
    }
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR", fixtures("test_libraries"))]
async fn bulk_scan_produces_correlated_folder_enqueues(pool: PgPool) -> anyhow::Result<()> {
    let root = TempDir::new()?;
    // Ensure a first-level child exists so bulk seed enqueues at least one folder.
    let seed = root.path().join("seed_a");
    std::fs::create_dir_all(&seed)?;
    let library = make_library(vec![root.path().to_path_buf()]);

    let (scan_control, orchestrator, _cache_dir, _image_cache_dir) =
        setup_scan_control(pool.clone(), library.clone()).await?;
    yield_now().await;

    let mut events_rx = orchestrator.subscribe_job_events();
    let provided = Uuid::now_v7();

    let accepted = scan_control
        .start_library_scan(library.id, Some(provided))
        .await?;
    assert_eq!(accepted.correlation_id, provided);

    let expected = library.paths.len();
    let mut observed: Vec<Uuid> = Vec::with_capacity(expected);
    let deadline = Instant::now() + Duration::from_secs(5);

    while observed.len() < expected && Instant::now() < deadline {
        match timeout(Duration::from_millis(500), events_rx.recv()).await {
            Ok(Ok(event)) => {
                if event.meta.library_id != library.id {
                    continue;
                }
                if let JobEventPayload::Enqueued {
                    kind: JobKind::FolderScan,
                    ..
                } = &event.payload
                {
                    observed.push(event.meta.correlation_id);
                    if observed.len() == expected {
                        break;
                    }
                }
            }
            Ok(Err(RecvError::Lagged(_))) => continue,
            Ok(Err(RecvError::Closed)) => break,
            Err(_) => {}
        }
    }

    assert_eq!(
        observed.len(),
        expected,
        "expected {} folder enqueues, saw {}",
        expected,
        observed.len()
    );
    assert!(observed.iter().all(|correlation| *correlation == provided));

    // No workers run in this test, so we only expect progress frames reflecting
    // enqueues (total_items should increase), not actual completions.
    let sse_deadline = Instant::now() + Duration::from_secs(10);
    let mut progress_observed = false;
    while Instant::now() < sse_deadline && !progress_observed {
        match scan_control.events(&accepted.scan_id).await {
            Ok(frames) => {
                progress_observed = frames.iter().any(|frame| {
                    matches!(frame.event, ScanEventKind::Progress) && frame.payload.total_items > 0
                });
                if progress_observed {
                    break;
                }
            }
            Err(_) => {}
        }
        sleep(Duration::from_millis(200)).await;
    }

    assert!(
        progress_observed,
        "scan never surfaced enqueue progress via SSE"
    );

    orchestrator.shutdown().await?;
    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR", fixtures("test_libraries"))]
async fn progress_sse_handles_out_of_order_events(pool: PgPool) -> anyhow::Result<()> {
    let root = TempDir::new()?;
    let library = make_library(vec![root.path().to_path_buf()]);

    let (scan_control, orchestrator, _cache_dir, _image_cache_dir) =
        setup_scan_control(pool.clone(), library.clone()).await?;
    yield_now().await;

    let accepted = scan_control
        .start_library_scan(library.id, None)
        .await
        .expect("scan start");
    yield_now().await;

    let mut receiver = scan_control
        .subscribe_scan(accepted.scan_id)
        .await
        .expect("subscribe");

    let baseline = loop {
        if let Some(snapshot) = scan_control.snapshot(&accepted.scan_id).await {
            break snapshot;
        }
        yield_now().await;
    };
    let baseline_total = baseline.total_items;
    let baseline_completed = baseline.completed_items;

    let folder_path = root.path().join("out_of_order");
    std::fs::create_dir_all(&folder_path)?;
    let path_key = normalize_path(&folder_path);
    let idempotency_key = "folder-out-of-order";

    let events_bus = orchestrator.runtime().events();
    let job_id = JobId::new();

    // Emit completion before enqueue to simulate out-of-order delivery.
    events_bus
        .publish(job_event(
            accepted.correlation_id,
            library.id,
            idempotency_key,
            &path_key,
            JobEventPayload::Completed {
                job_id,
                kind: JobKind::FolderScan,
                priority: JobPriority::P1,
            },
        ))
        .await
        .expect("publish completed");

    yield_now().await;

    events_bus
        .publish(job_event(
            accepted.correlation_id,
            library.id,
            idempotency_key,
            &path_key,
            JobEventPayload::Enqueued {
                job_id,
                kind: JobKind::FolderScan,
                priority: JobPriority::P1,
            },
        ))
        .await
        .expect("publish enqueued");

    let start_wait = Instant::now();
    let mut progress_totals: Option<(u64, u64)> = None;
    let deadline = start_wait + Duration::from_secs(5);

    while Instant::now() < deadline && progress_totals.is_none() {
        match timeout(Duration::from_millis(250), receiver.recv()).await {
            Ok(Ok(frame)) => match frame.event {
                ScanEventKind::Progress => {
                    if frame.payload.path_key.as_deref() == Some(path_key.as_str()) {
                        progress_totals =
                            Some((frame.payload.total_items, frame.payload.completed_items));
                    }
                }
                _ => {}
            },
            Ok(Err(RecvError::Lagged(_))) => continue,
            Ok(Err(RecvError::Closed)) => break,
            Err(_) => {}
        }
    }

    let (total_items, completed_items) =
        progress_totals.expect("progress payload observed for out-of-order job");
    assert!(
        total_items >= baseline_total + 1,
        "total items should account for completed job"
    );
    assert!(
        completed_items >= baseline_completed + 1,
        "completed items should account for completed job"
    );

    let completion_deadline = Instant::now() + Duration::from_secs(10);
    let mut completion_payload = None;
    while Instant::now() < completion_deadline && completion_payload.is_none() {
        match timeout(Duration::from_millis(250), receiver.recv()).await {
            Ok(Ok(frame)) => {
                if matches!(frame.event, ScanEventKind::Completed)
                    && frame.payload.correlation_id == accepted.correlation_id
                {
                    completion_payload = Some(frame.payload);
                }
            }
            Ok(Err(RecvError::Lagged(_))) => continue,
            Ok(Err(RecvError::Closed)) => break,
            Err(_) => {}
        }
    }

    let completion_payload = if let Some(payload) = completion_payload {
        payload
    } else {
        let snapshot = scan_control.snapshot(&accepted.scan_id).await;
        let history_statuses: Vec<String> = scan_control
            .history(5)
            .await
            .into_iter()
            .map(|entry| {
                format!(
                    "{:?} (total={}, completed={})",
                    entry.status, entry.total_items, entry.completed_items
                )
            })
            .collect();
        let event_log = scan_control
            .events(&accepted.scan_id)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|frame| {
                format!(
                    "{:?} total={} completed={} path={:?}",
                    frame.event,
                    frame.payload.total_items,
                    frame.payload.completed_items,
                    frame.payload.path_key
                )
            })
            .collect::<Vec<_>>();
        panic!(
            "scan should complete after quiescence window; snapshot={:?}, history={:?}, events={:?}",
            snapshot.map(|snap| format!(
                "{:?} total={} completed={} current_path={:?}",
                snap.status, snap.total_items, snap.completed_items, snap.current_path
            )),
            history_statuses,
            event_log
        );
    };
    assert_eq!(
        completion_payload.total_items, completion_payload.completed_items,
        "completed scans should have matching totals"
    );
    assert!(
        completion_payload.completed_items >= baseline_completed + 1,
        "completion payload should reflect processed job"
    );

    let history = scan_control.history(1).await;
    assert!(
        history.first().map(|entry| entry.status.clone()) == Some(ScanLifecycleStatus::Completed),
        "history should mark scan completed"
    );

    orchestrator.shutdown().await?;
    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR", fixtures("test_libraries"))]
async fn lease_renewals_keep_scan_running(pool: PgPool) -> anyhow::Result<()> {
    let root = TempDir::new()?;
    let library = make_library(vec![root.path().to_path_buf()]);

    let (scan_control, orchestrator, _cache_dir, _image_cache_dir) =
        setup_scan_control_with_quiescence(
            pool.clone(),
            library.clone(),
            Duration::from_millis(200),
        )
        .await?;
    yield_now().await;

    let accepted = scan_control
        .start_library_scan(library.id, None)
        .await
        .expect("scan start");

    let run_id = accepted.scan_id;
    let mut initial_snapshot = None;
    for _ in 0..10 {
        if let Some(snapshot) = scan_control.snapshot(&run_id).await {
            initial_snapshot = Some(snapshot);
            break;
        }
        yield_now().await;
    }
    let baseline = initial_snapshot.expect("scan snapshot available");

    let events_bus = orchestrator.runtime().events();

    for path in &library.paths {
        let path_norm = normalize_path(path);
        let idempotency = format!("scan:{}:{}", library.id, path_norm);
        let job_id = JobId::new();
        events_bus
            .publish(job_event(
                accepted.correlation_id,
                library.id,
                &idempotency,
                &path_norm,
                JobEventPayload::Completed {
                    job_id,
                    kind: JobKind::FolderScan,
                    priority: JobPriority::P1,
                },
            ))
            .await?;
    }

    let manual_path = root.path().join("lease-renew");
    std::fs::create_dir_all(&manual_path)?;
    let manual_norm = normalize_path(&manual_path);
    let manual_idempotency = format!("scan:{}:{}", library.id, manual_norm);
    let job_id = JobId::new();
    let lease_id = LeaseId::new();

    events_bus
        .publish(job_event(
            accepted.correlation_id,
            library.id,
            &manual_idempotency,
            &manual_norm,
            JobEventPayload::Enqueued {
                job_id,
                kind: JobKind::FolderScan,
                priority: JobPriority::P0,
            },
        ))
        .await?;

    yield_now().await;

    for renewals in 1..=3 {
        events_bus
            .publish(job_event(
                accepted.correlation_id,
                library.id,
                &manual_idempotency,
                &manual_norm,
                JobEventPayload::LeaseRenewed {
                    job_id,
                    lease_id,
                    renewals: renewals as u32,
                },
            ))
            .await?;
        sleep(Duration::from_millis(100)).await;
    }

    sleep(Duration::from_millis(700)).await;

    let snapshot = scan_control
        .snapshot(&run_id)
        .await
        .expect("snapshot available after renewals");

    assert_eq!(
        snapshot.status,
        ScanLifecycleStatus::Running,
        "lease renewals should keep the scan marked as running"
    );
    assert_eq!(
        snapshot.dead_lettered_items, baseline.dead_lettered_items,
        "lease renewals should not advance terminal counters",
    );

    orchestrator.shutdown().await?;
    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR", fixtures("test_libraries"))]
async fn completed_folders_from_previous_scan_are_visible_after_restart(
    pool: PgPool,
) -> anyhow::Result<()> {
    let root = TempDir::new()?;
    let library = make_library(vec![root.path().to_path_buf()]);

    let (scan_control, orchestrator, _cache_dir, _image_cache_dir) =
        setup_scan_control(pool.clone(), library.clone()).await?;
    yield_now().await;

    let first = scan_control
        .start_library_scan(library.id, None)
        .await
        .expect("first scan start");

    let _first_snapshot = loop {
        if let Some(snapshot) = scan_control.snapshot(&first.scan_id).await {
            break snapshot;
        }
        yield_now().await;
    };

    let events_bus = orchestrator.runtime().events();
    let carry_path = root.path().join("carry-over");
    std::fs::create_dir_all(&carry_path)?;
    let carry_norm = normalize_path(&carry_path);
    let idempotency = format!("scan:{}:{}", library.id, carry_norm);
    let job_id = JobId::new();

    events_bus
        .publish(job_event(
            first.correlation_id,
            library.id,
            &idempotency,
            &carry_norm,
            JobEventPayload::Enqueued {
                job_id,
                kind: JobKind::FolderScan,
                priority: JobPriority::P0,
            },
        ))
        .await?;

    yield_now().await;

    scan_control.cancel_scan(&first.scan_id).await?;
    sleep(Duration::from_millis(100)).await;

    let second = scan_control
        .start_library_scan(library.id, None)
        .await
        .expect("second scan start");

    let second_baseline = loop {
        if let Some(snapshot) = scan_control.snapshot(&second.scan_id).await {
            break snapshot;
        }
        yield_now().await;
    };

    events_bus
        .publish(job_event(
            first.correlation_id,
            library.id,
            &idempotency,
            &carry_norm,
            JobEventPayload::Completed {
                job_id,
                kind: JobKind::FolderScan,
                priority: JobPriority::P0,
            },
        ))
        .await?;

    yield_now().await;
    sleep(Duration::from_millis(200)).await;

    let refreshed = scan_control
        .snapshot(&second.scan_id)
        .await
        .expect("second scan snapshot");

    assert!(
        refreshed.total_items > second_baseline.total_items,
        "carry-over folder should be accounted for in restarted scan"
    );
    assert!(
        refreshed.completed_items > second_baseline.completed_items,
        "carry-over folder completion should advance progress in restarted scan"
    );

    orchestrator.shutdown().await?;
    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR", fixtures("test_libraries"))]
async fn fs_watch_produces_correlated_hot_scans(pool: PgPool) -> anyhow::Result<()> {
    let root = TempDir::new()?;
    let library = make_library(vec![root.path().to_path_buf()]);

    let (scan_control, orchestrator, _cache_dir, _image_cache_dir) =
        setup_scan_control(pool.clone(), library.clone()).await?;
    yield_now().await;

    let mut events_rx = orchestrator.subscribe_job_events();
    let run_id = Uuid::now_v7();

    let accepted = scan_control
        .start_library_scan(library.id, Some(run_id))
        .await?;
    assert_eq!(accepted.correlation_id, run_id);

    let mut sse_rx = scan_control.subscribe_scan(accepted.scan_id).await?;

    let baseline_snapshot = loop {
        if let Some(snapshot) = scan_control.snapshot(&accepted.scan_id).await {
            break snapshot;
        }
        yield_now().await;
    };
    let baseline_completed = baseline_snapshot.completed_items;

    let hot_folder = root.path().join("watch-hot");
    std::fs::create_dir_all(&hot_folder)?;
    let path_key = normalize_path(&hot_folder);

    let fs_event = FileSystemEvent {
        version: 1,
        correlation_id: Some(run_id),
        idempotency_key: format!("fs-hot-{}", Uuid::now_v7()),
        library_id: library.id,
        path_key: path_key.clone(),
        fingerprint: None,
        path: hot_folder.clone(),
        old_path: None,
        kind: FileSystemEventKind::Created,
        occurred_at: Utc::now(),
    };

    orchestrator
        .command_library(
            library.id,
            LibraryActorCommand::FsEvents {
                root: LibraryRootsId(0),
                events: vec![fs_event],
                correlation_id: Some(run_id),
            },
        )
        .await?;

    let mut job_id: Option<JobId> = None;
    let mut seen_enqueued = false;
    let mut seen_dequeued = false;
    let mut seen_completed = false;
    let job_deadline = Instant::now() + Duration::from_secs(10);

    while Instant::now() < job_deadline && !(seen_enqueued && seen_dequeued && seen_completed) {
        match timeout(Duration::from_millis(500), events_rx.recv()).await {
            Ok(Ok(event)) => {
                if event.meta.library_id != library.id {
                    continue;
                }
                if event.meta.path_key.as_deref() != Some(path_key.as_str()) {
                    continue;
                }
                assert_eq!(event.meta.correlation_id, run_id);
                match &event.payload {
                    JobEventPayload::Enqueued {
                        kind: JobKind::FolderScan,
                        job_id: id,
                        ..
                    } => {
                        job_id = Some(*id);
                        seen_enqueued = true;
                    }
                    JobEventPayload::Dequeued {
                        kind: JobKind::FolderScan,
                        job_id: id,
                        ..
                    } => {
                        if Some(*id) == job_id {
                            seen_dequeued = true;
                        }
                    }
                    JobEventPayload::Completed {
                        kind: JobKind::FolderScan,
                        job_id: id,
                        ..
                    } => {
                        if Some(*id) == job_id {
                            seen_completed = true;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Err(RecvError::Lagged(_))) => continue,
            Ok(Err(RecvError::Closed)) => break,
            Err(_) => {}
        }
    }

    assert!(seen_enqueued, "did not observe watcher enqueue");
    assert!(seen_dequeued, "did not observe watcher dequeue");
    assert!(seen_completed, "did not observe watcher completion");

    let sse_deadline = Instant::now() + Duration::from_secs(10);
    let mut watcher_progress = false;
    while Instant::now() < sse_deadline && !watcher_progress {
        match timeout(Duration::from_millis(500), sse_rx.recv()).await {
            Ok(Ok(frame)) => {
                if matches!(frame.event, ScanEventKind::Progress)
                    && frame.payload.correlation_id == run_id
                    && frame.payload.completed_items > baseline_completed
                    && frame.payload.path_key.as_deref() == Some(path_key.as_str())
                {
                    watcher_progress = true;
                }
            }
            Ok(Err(RecvError::Lagged(_))) => continue,
            Ok(Err(RecvError::Closed)) => break,
            Err(_) => {}
        }
    }

    assert!(
        watcher_progress,
        "scan SSE stream did not report watcher-driven progress"
    );

    orchestrator.shutdown().await?;
    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR", fixtures("test_libraries"))]
async fn scan_progress_sse_streams_progress_until_completion(pool: PgPool) -> anyhow::Result<()> {
    let root_one = TempDir::new()?;
    let root_two = TempDir::new()?;
    let library = make_library(vec![
        root_one.path().to_path_buf(),
        root_two.path().to_path_buf(),
    ]);

    let (scan_control, orchestrator, _cache_dir, _image_cache_dir) =
        setup_scan_control(pool.clone(), library.clone()).await?;
    yield_now().await;

    let accepted = scan_control
        .start_library_scan(library.id, None)
        .await
        .expect("scan start");
    yield_now().await;

    let mut receiver = scan_control
        .subscribe_scan(accepted.scan_id)
        .await
        .expect("subscribe");

    // Load history immediately to ensure the started frame is present.
    let mut frames: Vec<ScanBroadcastFrame> = scan_control
        .events(&accepted.scan_id)
        .await
        .expect("history");
    assert!(matches!(
        frames.first().map(|f| &f.event),
        Some(ScanEventKind::Started)
    ));

    let events_bus = orchestrator.runtime().events();

    // Emit folder scan lifecycle events.
    let job_id = JobId::new();
    let folder_key = format!("{}", root_one.path().join("folder_a").to_string_lossy());

    sleep(Duration::from_millis(50)).await;

    events_bus
        .publish(job_event(
            accepted.correlation_id,
            library.id,
            "folder-a",
            &folder_key,
            JobEventPayload::Enqueued {
                job_id,
                kind: JobKind::FolderScan,
                priority: JobPriority::P1,
            },
        ))
        .await
        .expect("publish enqueued");

    yield_now().await;

    events_bus
        .publish(job_event(
            accepted.correlation_id,
            library.id,
            "folder-a",
            &folder_key,
            JobEventPayload::Completed {
                job_id,
                kind: JobKind::FolderScan,
                priority: JobPriority::P1,
            },
        ))
        .await
        .expect("publish completed");

    // Allow the aggregator to process events and drive completion.
    sleep(Duration::from_millis(200)).await;

    // Drain live stream until we observe completion.
    let mut seen_progress = frames
        .iter()
        .any(|f| matches!(f.event, ScanEventKind::Progress));
    let mut seen_completed = frames
        .iter()
        .any(|f| matches!(f.event, ScanEventKind::Completed));
    let mut seen_quiescing = frames
        .iter()
        .any(|f| matches!(f.event, ScanEventKind::Quiescing));
    while !(seen_progress && seen_completed) {
        match timeout(Duration::from_secs(2), receiver.recv()).await {
            Ok(Ok(frame)) => {
                seen_progress |= matches!(frame.event, ScanEventKind::Progress);
                seen_quiescing |= matches!(frame.event, ScanEventKind::Quiescing);
                seen_completed |= matches!(frame.event, ScanEventKind::Completed);
                frames.push(frame);
            }
            Ok(Err(err)) => panic!("broadcast error: {err}"),
            Err(_) => panic!("timed out waiting for scan events"),
        }
    }

    let event_names: Vec<&str> = frames.iter().map(frame_label).collect();
    assert_eq!(event_names[0], "scan.started");
    assert!(event_names.iter().any(|name| *name == "scan.progress"));
    assert!(event_names.iter().any(|name| *name == "scan.quiescing"));
    assert!(event_names.last().copied() == Some("scan.completed"));
    assert!(seen_quiescing, "expected quiescing frame before completion");

    let history: Vec<ScanHistoryEntry> = scan_control.history(1).await;
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].status, ScanLifecycleStatus::Completed);

    orchestrator.shutdown().await?;
    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR", fixtures("test_libraries"))]
async fn quiescing_reopens_when_new_items_arrive(pool: PgPool) -> anyhow::Result<()> {
    let root = TempDir::new()?;
    let library = make_library(vec![root.path().to_path_buf()]);

    let (scan_control, orchestrator, _cache_dir, _image_cache_dir) =
        setup_scan_control(pool.clone(), library.clone()).await?;
    yield_now().await;

    let accepted = scan_control.start_library_scan(library.id, None).await?;
    let mut receiver = scan_control.subscribe_scan(accepted.scan_id).await?;

    // Drain the initial started notification.
    recv_matching_frame(&mut receiver, |frame| {
        matches!(frame.event, ScanEventKind::Started)
    })
    .await;

    let events_bus = orchestrator.runtime().events();

    // Drive the run into quiescing with a single completed folder.
    let first_path = root.path().join("folder_a");
    let first_norm = normalize_path(&first_path);
    let first_job = JobId::new();

    events_bus
        .publish(job_event(
            accepted.correlation_id,
            library.id,
            &format!("scan:{}:{}", library.id, first_norm),
            &first_norm,
            JobEventPayload::Enqueued {
                job_id: first_job,
                kind: JobKind::FolderScan,
                priority: JobPriority::P1,
            },
        ))
        .await?;

    events_bus
        .publish(job_event(
            accepted.correlation_id,
            library.id,
            &format!("scan:{}:{}", library.id, first_norm),
            &first_norm,
            JobEventPayload::Completed {
                job_id: first_job,
                kind: JobKind::FolderScan,
                priority: JobPriority::P1,
            },
        ))
        .await?;

    let quiescing_frame = recv_matching_frame(&mut receiver, |frame| {
        matches!(frame.event, ScanEventKind::Quiescing)
    })
    .await;
    assert_eq!(quiescing_frame.payload.status, "quiescing");

    // Enqueue new work while quiescing; the run should reopen and emit a processing update.
    let second_path = root.path().join("folder_b");
    let second_norm = normalize_path(&second_path);
    let second_job = JobId::new();

    events_bus
        .publish(job_event(
            accepted.correlation_id,
            library.id,
            &format!("scan:{}:{}", library.id, second_norm),
            &second_norm,
            JobEventPayload::Enqueued {
                job_id: second_job,
                kind: JobKind::FolderScan,
                priority: JobPriority::P1,
            },
        ))
        .await?;

    let reopened_progress = recv_matching_frame(&mut receiver, |frame| {
        matches!(frame.event, ScanEventKind::Progress)
            && frame.payload.current_path.as_deref() == Some(second_norm.as_str())
    })
    .await;
    assert_eq!(reopened_progress.payload.status, "processing");

    events_bus
        .publish(job_event(
            accepted.correlation_id,
            library.id,
            &format!("scan:{}:{}", library.id, second_norm),
            &second_norm,
            JobEventPayload::Completed {
                job_id: second_job,
                kind: JobKind::FolderScan,
                priority: JobPriority::P1,
            },
        ))
        .await?;

    let final_quiescing = recv_matching_frame(&mut receiver, |frame| {
        matches!(frame.event, ScanEventKind::Quiescing)
            && frame.payload.current_path.as_deref() == Some(second_norm.as_str())
    })
    .await;
    assert_eq!(final_quiescing.payload.status, "quiescing");

    let completed = recv_matching_frame(&mut receiver, |frame| {
        matches!(frame.event, ScanEventKind::Completed)
    })
    .await;
    assert_eq!(completed.payload.status, "completed");

    orchestrator.shutdown().await?;
    Ok(())
}

async fn recv_matching_frame<F>(
    receiver: &mut broadcast::Receiver<ScanBroadcastFrame>,
    predicate: F,
) -> ScanBroadcastFrame
where
    F: Fn(&ScanBroadcastFrame) -> bool,
{
    loop {
        match timeout(Duration::from_secs(2), receiver.recv()).await {
            Ok(Ok(frame)) => {
                if predicate(&frame) {
                    return frame;
                }
            }
            Ok(Err(RecvError::Lagged(_))) => continue,
            Ok(Err(RecvError::Closed)) => panic!("scan broadcast closed unexpectedly"),
            Err(_) => panic!("timed out waiting for scan frame"),
        }
    }
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR", fixtures("test_libraries"))]
async fn retryable_failures_do_not_fail_run(pool: PgPool) -> anyhow::Result<()> {
    let root = TempDir::new()?;
    let library = make_library(vec![root.path().to_path_buf()]);

    let (scan_control, orchestrator, _cache_dir, _image_cache_dir) =
        setup_scan_control(pool.clone(), library.clone()).await?;
    yield_now().await;

    let accepted = scan_control.start_library_scan(library.id, None).await?;
    yield_now().await;

    let mut receiver = scan_control
        .subscribe_scan(accepted.scan_id)
        .await
        .expect("subscribe");

    let events_bus = orchestrator.runtime().events();
    let job_id = JobId::new();
    let folder_key = format!("{}", root.path().join("retry_me").to_string_lossy());

    // Initial enqueue generates a progress frame.
    events_bus
        .publish(job_event(
            accepted.correlation_id,
            library.id,
            "retry-folder",
            &folder_key,
            JobEventPayload::Enqueued {
                job_id,
                kind: JobKind::FolderScan,
                priority: JobPriority::P1,
            },
        ))
        .await?;

    recv_matching_frame(&mut receiver, |frame| {
        matches!(frame.event, ScanEventKind::Progress)
    })
    .await;

    // Simulate a retryable failure.
    events_bus
        .publish(job_event(
            accepted.correlation_id,
            library.id,
            "retry-folder",
            &folder_key,
            JobEventPayload::Failed {
                job_id,
                kind: JobKind::FolderScan,
                priority: JobPriority::P1,
                retryable: true,
            },
        ))
        .await?;

    let retry_frame = recv_matching_frame(&mut receiver, |frame| {
        matches!(frame.event, ScanEventKind::Progress) && frame.payload.retrying_items == Some(1)
    })
    .await;
    assert_eq!(retry_frame.payload.status, "processing");

    // Re-enqueue the folder after retry.
    events_bus
        .publish(job_event(
            accepted.correlation_id,
            library.id,
            "retry-folder",
            &folder_key,
            JobEventPayload::Enqueued {
                job_id,
                kind: JobKind::FolderScan,
                priority: JobPriority::P1,
            },
        ))
        .await?;

    // Ensure retrying count clears on successful enqueue.
    recv_matching_frame(&mut receiver, |frame| {
        matches!(frame.event, ScanEventKind::Progress) && frame.payload.retrying_items.is_none()
    })
    .await;

    // Enqueue a fresh folder to ensure new work surfaces immediately after the retry loop.
    let new_job = JobId::new();
    let new_folder_key = format!("{}", root.path().join("fresh").to_string_lossy());
    events_bus
        .publish(job_event(
            accepted.correlation_id,
            library.id,
            "fresh-folder",
            &new_folder_key,
            JobEventPayload::Enqueued {
                job_id: new_job,
                kind: JobKind::FolderScan,
                priority: JobPriority::P1,
            },
        ))
        .await?;

    let fresh_enqueue = recv_matching_frame(&mut receiver, |frame| {
        matches!(frame.event, ScanEventKind::Progress)
            && frame.payload.path_key.as_deref() == Some(new_folder_key.as_str())
    })
    .await;
    assert_eq!(fresh_enqueue.payload.path_key, Some(new_folder_key.clone()));

    // Complete the original folder followed by the fresh one.
    events_bus
        .publish(job_event(
            accepted.correlation_id,
            library.id,
            "retry-folder",
            &folder_key,
            JobEventPayload::Completed {
                job_id,
                kind: JobKind::FolderScan,
                priority: JobPriority::P1,
            },
        ))
        .await?;

    events_bus
        .publish(job_event(
            accepted.correlation_id,
            library.id,
            "fresh-folder",
            &new_folder_key,
            JobEventPayload::Completed {
                job_id: new_job,
                kind: JobKind::FolderScan,
                priority: JobPriority::P1,
            },
        ))
        .await?;

    // Allow completion to process.
    sleep(Duration::from_millis(200)).await;

    let completion_frame = recv_matching_frame(&mut receiver, |frame| {
        matches!(frame.event, ScanEventKind::Completed)
    })
    .await;

    assert_eq!(completion_frame.payload.dead_lettered_items, None);
    assert_eq!(completion_frame.payload.retrying_items, None);
    assert!(completion_frame.payload.sequence > fresh_enqueue.payload.sequence);

    let history = scan_control.history(1).await;
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].status, ScanLifecycleStatus::Completed);

    orchestrator.shutdown().await?;
    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR", fixtures("test_libraries"))]
async fn dead_lettered_items_complete_without_failure(pool: PgPool) -> anyhow::Result<()> {
    let root = TempDir::new()?;
    let library = make_library(vec![root.path().to_path_buf()]);

    let (scan_control, orchestrator, _cache_dir, _image_cache_dir) =
        setup_scan_control(pool.clone(), library.clone()).await?;
    yield_now().await;

    let accepted = scan_control.start_library_scan(library.id, None).await?;
    yield_now().await;

    let mut receiver = scan_control
        .subscribe_scan(accepted.scan_id)
        .await
        .expect("subscribe");

    let events_bus = orchestrator.runtime().events();
    let job_id = JobId::new();
    let folder_key = format!("{}", root.path().join("dead_letter").to_string_lossy());

    events_bus
        .publish(job_event(
            accepted.correlation_id,
            library.id,
            "dead-letter",
            &folder_key,
            JobEventPayload::Enqueued {
                job_id,
                kind: JobKind::FolderScan,
                priority: JobPriority::P1,
            },
        ))
        .await?;

    recv_matching_frame(&mut receiver, |frame| {
        matches!(frame.event, ScanEventKind::Progress)
    })
    .await;

    events_bus
        .publish(job_event(
            accepted.correlation_id,
            library.id,
            "dead-letter",
            &folder_key,
            JobEventPayload::DeadLettered {
                job_id,
                kind: JobKind::FolderScan,
                priority: JobPriority::P1,
            },
        ))
        .await?;

    let dead_letter_frame = recv_matching_frame(&mut receiver, |frame| {
        matches!(frame.event, ScanEventKind::Progress)
            && frame.payload.dead_lettered_items == Some(1)
    })
    .await;
    assert_eq!(dead_letter_frame.payload.status, "processing");

    // Allow completion to trigger after dead-letter acknowledgement.
    sleep(Duration::from_millis(200)).await;

    let terminal_frame = recv_matching_frame(&mut receiver, |frame| {
        matches!(frame.event, ScanEventKind::Completed)
    })
    .await;

    assert_eq!(terminal_frame.payload.dead_lettered_items, Some(1));
    assert_eq!(terminal_frame.payload.status, "completed");

    let history = scan_control.history(1).await;
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].status, ScanLifecycleStatus::Completed);

    orchestrator.shutdown().await?;
    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR", fixtures("test_libraries"))]
async fn sse_resumes_from_last_event_id(pool: PgPool) -> anyhow::Result<()> {
    use axum::extract::{Path as AxumPath, State as AxumState};
    use axum::http::{HeaderName, HeaderValue};
    use axum::response::sse::{Event as SseEvent, KeepAlive};
    use axum::routing::get;
    use axum::{Router, response::Sse};

    let root = TempDir::new()?;
    let library = make_library(vec![root.path().to_path_buf()]);

    let (scan_control, orchestrator, _cache_dir, _image_cache_dir) =
        setup_scan_control(pool.clone(), library.clone()).await?;
    yield_now().await;

    let accepted = scan_control.start_library_scan(library.id, None).await?;
    yield_now().await;

    let events_bus = orchestrator.runtime().events();
    let job_id = JobId::new();
    let folder_key = format!("{}", root.path().join("resume").to_string_lossy());

    events_bus
        .publish(job_event(
            accepted.correlation_id,
            library.id,
            "resume",
            &folder_key,
            JobEventPayload::Enqueued {
                job_id,
                kind: JobKind::FolderScan,
                priority: JobPriority::P1,
            },
        ))
        .await?;

    // Spawn a canceller so the SSE stream completes after we connect.
    let cancel_control = Arc::clone(&scan_control);
    let cancel_scan_id = accepted.scan_id;
    tokio::spawn(async move {
        sleep(Duration::from_millis(200)).await;
        let _ = cancel_control.cancel_scan(&cancel_scan_id).await;
    });

    async fn test_sse_handler(
        AxumState(scan_control): AxumState<Arc<TestScanControlPlane>>,
        AxumPath(scan_id): AxumPath<Uuid>,
        headers: axum::http::HeaderMap,
    ) -> Sse<
        Pin<Box<dyn tokio_stream::Stream<Item = std::result::Result<SseEvent, Infallible>> + Send>>,
    > {
        let last_sequence = headers
            .get("last-event-id")
            .and_then(|value| value.to_str().ok())
            .and_then(|raw| raw.trim().parse::<u64>().ok());

        let stream = build_scan_progress_stream(scan_control, scan_id, last_sequence)
            .await
            .expect("stream");
        Sse::new(stream).keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(5))
                .text("keep-alive"),
        )
    }

    let router = Router::new()
        .route("/scan/{id}/progress", get(test_sse_handler))
        .with_state(Arc::clone(&scan_control));

    let server = TestServer::new(router)?;

    let response = server
        .get(&format!("/scan/{}/progress", accepted.scan_id))
        .add_header(
            HeaderName::from_static("last-event-id"),
            HeaderValue::from_static("1"),
        )
        .await;

    response.assert_status_success();
    let body = response.text();

    assert!(
        !body.contains("\nid: 1\n"),
        "resume stream repeated skipped id"
    );
    assert!(
        body.contains("\nid: 2\n"),
        "resume stream missed expected progress event"
    );

    orchestrator.shutdown().await?;
    Ok(())
}
