//! API service trait and implementations
//!
//! Provides abstraction over HTTP API operations,
//! replacing direct ApiClient access per RUS-136.

#[cfg(feature = "demo")]
use crate::infrastructure::api_types::{DemoResetRequest, DemoStatus};
use crate::infrastructure::repository::RepositoryResult;
use async_trait::async_trait;
use ferrex_core::{
    api_types::setup::{ConfirmClaimResponse, StartClaimResponse},
    player_prelude::{
        ActiveScansResponse, AuthToken, AuthenticatedDevice, CreateLibraryRequest,
        FilterIndicesRequest, LatestProgressResponse, Library, LibraryID, Media, MediaQuery,
        MediaWithStatus, ScanCommandAcceptedResponse, ScanCommandRequest, ScanConfig, ScanMetrics,
        StartScanRequest, UpdateLibraryRequest, UpdateProgressRequest, User, UserPermissions,
        UserWatchState,
    },
};
use rkyv::util::AlignedVec;
use std::fmt::Debug;
use uuid::Uuid;

/// Generic API service trait for server communication
#[async_trait]
pub trait ApiService: Send + Sync + Debug {
    async fn get_rkyv(
        &self,
        path: &str,
        query: Option<(&str, &str)>,
    ) -> RepositoryResult<AlignedVec>;

    /// Make a GET request that returns raw bytes (for images and binary content)
    async fn get_bytes(&self, path: &str, query: Option<(&str, &str)>)
        -> RepositoryResult<Vec<u8>>;

    // === Common API operations ===

    /// Fetch all libraries from the server
    async fn fetch_libraries(&self) -> RepositoryResult<Vec<Library>>;

    /// Fetch media for a specific library
    async fn fetch_library_media(&self, library_id: Uuid) -> RepositoryResult<Vec<Media>>;

    // === Library management ===
    /// Create a library on the server
    async fn create_library(&self, request: CreateLibraryRequest) -> RepositoryResult<LibraryID>;
    /// Update a library on the server
    async fn update_library(
        &self,
        id: LibraryID,
        request: UpdateLibraryRequest,
    ) -> RepositoryResult<()>;
    /// Delete a library on the server
    async fn delete_library(&self, id: LibraryID) -> RepositoryResult<()>;

    /// Start a library scan
    async fn start_library_scan(
        &self,
        library_id: LibraryID,
        request: StartScanRequest,
    ) -> RepositoryResult<ScanCommandAcceptedResponse>;

    /// Pause an active library scan
    async fn pause_library_scan(
        &self,
        library_id: LibraryID,
        request: ScanCommandRequest,
    ) -> RepositoryResult<ScanCommandAcceptedResponse>;

    /// Resume a paused library scan
    async fn resume_library_scan(
        &self,
        library_id: LibraryID,
        request: ScanCommandRequest,
    ) -> RepositoryResult<ScanCommandAcceptedResponse>;

    /// Cancel an active library scan
    async fn cancel_library_scan(
        &self,
        library_id: LibraryID,
        request: ScanCommandRequest,
    ) -> RepositoryResult<ScanCommandAcceptedResponse>;

    /// Fetch all active scans across libraries
    async fn fetch_active_scans(&self) -> RepositoryResult<ActiveScansResponse>;

    /// Fetch the latest progress frame for a scan
    async fn fetch_latest_scan_progress(
        &self,
        scan_id: uuid::Uuid,
    ) -> RepositoryResult<LatestProgressResponse>;

    /// Fetch scanner metrics (queue depths, active scan counts)
    async fn fetch_scan_metrics(&self) -> RepositoryResult<ScanMetrics>;

    /// Fetch orchestrator configuration currently in effect
    async fn fetch_scan_config(&self) -> RepositoryResult<ScanConfig>;

    /// Check server health
    async fn health_check(&self) -> RepositoryResult<bool>;

    #[cfg(feature = "demo")]
    async fn fetch_demo_status(&self) -> RepositoryResult<DemoStatus>;

    #[cfg(feature = "demo")]
    async fn reset_demo(&self, request: DemoResetRequest) -> RepositoryResult<DemoStatus>;

    // === Additional API operations ===

    /// Get watch status for all media
    async fn get_watch_state(&self) -> RepositoryResult<UserWatchState>;

    /// Update progress for a media item
    async fn update_progress(&self, request: &UpdateProgressRequest) -> RepositoryResult<()>;

    /// List authenticated devices for current user
    async fn list_user_devices(&self) -> RepositoryResult<Vec<AuthenticatedDevice>>;

    /// Revoke a device
    async fn revoke_device(&self, device_id: Uuid) -> RepositoryResult<()>;

    /// Query media with complex filters
    async fn query_media(&self, query: MediaQuery) -> RepositoryResult<Vec<MediaWithStatus>>;

    /// Fetch filtered index positions for a library based on the provided filter spec
    async fn fetch_filtered_indices(
        &self,
        library_id: Uuid,
        spec: &FilterIndicesRequest,
    ) -> RepositoryResult<Vec<u32>>;

    /// Check if setup is required
    async fn check_setup_status(
        &self,
    ) -> RepositoryResult<crate::infrastructure::api_client::SetupStatus>;

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
}

// Domain-specific scan API DTOs are defined in ferrex_core::api_scan
