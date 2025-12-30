//! API service trait and implementations
//!
//! Provides abstraction over HTTP API operations,
//! replacing direct ApiClient access per RUS-136.

#[cfg(feature = "demo")]
use crate::infra::api_types::{DemoResetRequest, DemoStatus};
use crate::infra::repository::RepositoryResult;
use async_trait::async_trait;
use ferrex_core::{
    api::types::setup::{ConfirmClaimResponse, StartClaimResponse},
    player_prelude::{
        ActiveScansResponse, AuthToken, AuthenticatedDevice,
        CreateLibraryRequest, FilterIndicesRequest, LatestProgressResponse,
        Library, LibraryId, Media, MediaQuery, MediaRootBrowseResponse,
        MediaWithStatus, MovieBatchFetchRequest, MovieBatchId,
        MovieBatchSyncRequest, MovieBatchSyncResponse, NextEpisode,
        ScanCommandAcceptedResponse, ScanCommandRequest, ScanConfig,
        ScanMetrics, SeasonWatchStatus, SeriesBundleFetchRequest,
        SeriesBundleSyncRequest, SeriesBundleSyncResponse, SeriesID,
        SeriesWatchStatus, StartScanRequest, UpdateLibraryRequest,
        UpdateProgressRequest, User, UserPermissions, UserWatchState,
    },
};
use ferrex_model::image::ImageQuery;
use rkyv::util::AlignedVec;
use std::fmt::Debug;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum ImageFetchResult {
    Ready(Vec<u8>),
    Pending { retry_after: Option<Duration> },
}

/// Generic API service trait for server communication
#[async_trait]
pub trait ApiService: Send + Sync + Debug {
    async fn get_rkyv(
        &self,
        path: &str,
        query: Option<(&str, &str)>,
    ) -> RepositoryResult<AlignedVec>;

    /// Make a GET request that returns raw bytes (for images and binary content)
    async fn get_bytes(
        &self,
        path: &str,
        query: Option<(&str, &str)>,
    ) -> RepositoryResult<Vec<u8>>;

    /// Make a GET request that returns raw bytes with typed ImageSize
    async fn get_image(
        &self,
        path: &str,
        size: ImageQuery,
    ) -> RepositoryResult<ImageFetchResult>;

    // === Common API operations ===

    /// Fetch all libraries from the server
    async fn fetch_libraries(&self) -> RepositoryResult<Vec<Library>>;

    /// Fetch media for a specific library
    async fn fetch_library_media(
        &self,
        library_id: Uuid,
    ) -> RepositoryResult<Vec<Media>>;

    /// Fetch a finalized movie reference batch by (library_id, batch_id).
    ///
    /// Returns raw rkyv bytes so the player can keep this batch zero-copy.
    async fn fetch_movie_reference_batch(
        &self,
        library_id: LibraryId,
        batch_id: MovieBatchId,
    ) -> RepositoryResult<AlignedVec>;

    /// Fetch all finalized movie reference batches for a library as a single bundle.
    ///
    /// Returns raw rkyv bytes so the player can keep per-batch payloads zero-copy.
    async fn fetch_movie_reference_batch_bundle(
        &self,
        library_id: LibraryId,
    ) -> RepositoryResult<AlignedVec>;

    /// Compare cached movie batch versions against the server and get a list of updates.
    async fn sync_movie_reference_batches(
        &self,
        library_id: LibraryId,
        request: MovieBatchSyncRequest,
    ) -> RepositoryResult<MovieBatchSyncResponse>;

    /// Fetch a subset of movie batches for a library as a single rkyv bundle.
    async fn fetch_movie_reference_batches(
        &self,
        library_id: LibraryId,
        request: MovieBatchFetchRequest,
    ) -> RepositoryResult<AlignedVec>;

    /// Fetch a single series bundle by (library_id, series_id).
    ///
    /// Returns raw rkyv bytes so the player can keep this series archive zero-copy.
    async fn fetch_series_bundle(
        &self,
        library_id: LibraryId,
        series_id: SeriesID,
    ) -> RepositoryResult<AlignedVec>;

    /// Fetch all series bundles for a library as a single bundle.
    ///
    /// Returns raw rkyv bytes so the player can keep per-series payloads isolated.
    async fn fetch_series_bundle_bundle(
        &self,
        library_id: LibraryId,
    ) -> RepositoryResult<AlignedVec>;

    /// Compare cached series bundle versions against the server and get a list of updates.
    async fn sync_series_bundles(
        &self,
        library_id: LibraryId,
        request: SeriesBundleSyncRequest,
    ) -> RepositoryResult<SeriesBundleSyncResponse>;

    /// Fetch a subset of series bundles for a library as a single rkyv bundle.
    async fn fetch_series_bundles(
        &self,
        library_id: LibraryId,
        request: SeriesBundleFetchRequest,
    ) -> RepositoryResult<AlignedVec>;

    // === Library management ===
    /// Create a library on the server
    async fn create_library(
        &self,
        request: CreateLibraryRequest,
    ) -> RepositoryResult<LibraryId>;
    /// Update a library on the server
    async fn update_library(
        &self,
        id: LibraryId,
        request: UpdateLibraryRequest,
    ) -> RepositoryResult<()>;
    /// Delete a library on the server
    async fn delete_library(&self, id: LibraryId) -> RepositoryResult<()>;

    /// Start a library scan
    async fn start_library_scan(
        &self,
        library_id: LibraryId,
        request: StartScanRequest,
    ) -> RepositoryResult<ScanCommandAcceptedResponse>;

    /// Pause an active library scan
    async fn pause_library_scan(
        &self,
        library_id: LibraryId,
        request: ScanCommandRequest,
    ) -> RepositoryResult<ScanCommandAcceptedResponse>;

    /// Resume a paused library scan
    async fn resume_library_scan(
        &self,
        library_id: LibraryId,
        request: ScanCommandRequest,
    ) -> RepositoryResult<ScanCommandAcceptedResponse>;

    /// Cancel an active library scan
    async fn cancel_library_scan(
        &self,
        library_id: LibraryId,
        request: ScanCommandRequest,
    ) -> RepositoryResult<ScanCommandAcceptedResponse>;

    /// Fetch all active scans across libraries
    async fn fetch_active_scans(&self)
    -> RepositoryResult<ActiveScansResponse>;

    /// Fetch the latest progress frame for a scan
    async fn fetch_latest_scan_progress(
        &self,
        scan_id: uuid::Uuid,
    ) -> RepositoryResult<LatestProgressResponse>;

    /// Fetch scanner metrics (queue depths, active scan counts)
    async fn fetch_scan_metrics(&self) -> RepositoryResult<ScanMetrics>;

    /// Fetch orchestrator configuration currently in effect
    async fn fetch_scan_config(&self) -> RepositoryResult<ScanConfig>;

    /// Browse the server's media root (relative paths) to help admins pick folders.
    async fn browse_media_root(
        &self,
        path: Option<&str>,
    ) -> RepositoryResult<MediaRootBrowseResponse>;

    /// Check server health
    async fn health_check(&self) -> RepositoryResult<bool>;

    #[cfg(feature = "demo")]
    async fn fetch_demo_status(&self) -> RepositoryResult<DemoStatus>;

    #[cfg(feature = "demo")]
    async fn reset_demo(
        &self,
        request: DemoResetRequest,
    ) -> RepositoryResult<DemoStatus>;

    #[cfg(feature = "demo")]
    async fn resize_demo(
        &self,
        request: DemoResetRequest,
    ) -> RepositoryResult<DemoStatus>;

    // === Additional API operations ===

    /// Get watch status for all media
    async fn get_watch_state(&self) -> RepositoryResult<UserWatchState>;

    /// Update progress for a media item
    async fn update_progress(
        &self,
        request: &UpdateProgressRequest,
    ) -> RepositoryResult<()>;

    /// Get series watch state
    async fn get_series_watch_state(
        &self,
        tmdb_series_id: u64,
    ) -> RepositoryResult<SeriesWatchStatus>;

    /// Get season watch state
    async fn get_season_watch_state(
        &self,
        tmdb_series_id: u64,
        season_number: u16,
    ) -> RepositoryResult<SeasonWatchStatus>;

    /// Get next episode for a series
    async fn get_series_next_episode(
        &self,
        tmdb_series_id: u64,
    ) -> RepositoryResult<Option<NextEpisode>>;

    /// List authenticated devices for current user
    async fn list_user_devices(
        &self,
    ) -> RepositoryResult<Vec<AuthenticatedDevice>>;

    /// Revoke a device
    async fn revoke_device(&self, device_id: Uuid) -> RepositoryResult<()>;

    /// Query media with complex filters
    async fn query_media(
        &self,
        query: MediaQuery,
    ) -> RepositoryResult<Vec<MediaWithStatus>>;

    /// Fetch filtered index positions for a library based on the provided filter spec
    async fn fetch_filtered_indices(
        &self,
        library_id: Uuid,
        spec: &FilterIndicesRequest,
    ) -> RepositoryResult<Vec<u32>>;

    /// Check if setup is required
    async fn check_setup_status(
        &self,
    ) -> RepositoryResult<crate::infra::api_client::SetupStatus>;

    /// Create initial admin user during setup
    async fn create_initial_admin(
        &self,
        username: String,
        password: String,
        display_name: Option<String>,
        setup_token: Option<String>,
        claim_token: Option<String>,
    ) -> RepositoryResult<(User, AuthToken)>;

    /// Fetch the currently authenticated user profile
    async fn fetch_current_user(&self) -> RepositoryResult<User>;

    /// Fetch the current user's permissions
    async fn fetch_my_permissions(&self) -> RepositoryResult<UserPermissions>;

    /// Start the secure setup claim workflow and retrieve a claim code.
    async fn start_setup_claim(
        &self,
        device_name: Option<String>,
    ) -> RepositoryResult<StartClaimResponse>;

    /// Confirm an existing setup claim and receive the setup token.
    async fn confirm_setup_claim(
        &self,
        claim_code: String,
    ) -> RepositoryResult<ConfirmClaimResponse>;

    /// Build a full URL from a path
    fn build_url(&self, path: &str) -> String;

    /// Get the base URL
    fn base_url(&self) -> &str;

    /// Set the authentication token
    async fn set_token(&self, token: Option<AuthToken>);

    /// Get the current authentication token
    async fn get_token(&self) -> Option<AuthToken>;

    /// Fetch a short-lived playback ticket (scoped token) for a media item
    async fn fetch_playback_ticket(
        &self,
        media_id: &str,
    ) -> RepositoryResult<String>;
}

// Domain-specific scan API DTOs are defined in ferrex_core::api::scan
