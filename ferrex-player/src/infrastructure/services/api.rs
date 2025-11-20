//! API service trait and implementations
//!
//! Provides abstraction over HTTP API operations,
//! replacing direct ApiClient access per RUS-136.

use crate::infrastructure::repository::RepositoryResult;
use async_trait::async_trait;
use ferrex_core::auth::device::AuthenticatedDevice;
use ferrex_core::types::library::Library;
use ferrex_core::user::AuthToken;
use ferrex_core::watch_status::{UpdateProgressRequest, UserWatchState};
use ferrex_core::{LibraryID, Media, ScanResponse};
use rkyv::util::AlignedVec;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Generic API service trait for server communication
#[async_trait]
pub trait ApiService: Send + Sync {
    /// Make a GET request to the API
    async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str) -> RepositoryResult<T>;

    async fn get_rkyv(
        &self,
        path: &str,
        query: Option<(&str, &str)>,
    ) -> RepositoryResult<AlignedVec>;

    /// Make a GET request that returns raw bytes (for images and binary content)
    async fn get_bytes(&self, path: &str, query: Option<(&str, &str)>)
        -> RepositoryResult<Vec<u8>>;

    /// Make a POST request to the API
    async fn post<T: for<'de> Deserialize<'de>, B: Serialize + Send + Sync>(
        &self,
        path: &str,
        body: &B,
    ) -> RepositoryResult<T>;

    /// Make a PUT request to the API
    async fn put<T: for<'de> Deserialize<'de>, B: Serialize + Send + Sync>(
        &self,
        path: &str,
        body: &B,
    ) -> RepositoryResult<T>;

    /// Make a DELETE request to the API
    async fn delete<T: for<'de> Deserialize<'de>>(&self, path: &str) -> RepositoryResult<T>;

    // === Common API operations ===

    /// Fetch all libraries from the server
    async fn fetch_libraries(&self) -> RepositoryResult<Vec<Library>>;

    /// Fetch media for a specific library
    async fn fetch_library_media(&self, library_id: Uuid) -> RepositoryResult<Vec<Media>>;

    /// Start a library scan
    async fn scan_library(
        &self,
        library_id: LibraryID,
        force_refresh: bool,
    ) -> RepositoryResult<ScanResponse>;

    async fn scan_all_libraries(&self, force_refresh: bool) -> RepositoryResult<ScanResponse>;

    /// Check server health
    async fn health_check(&self) -> RepositoryResult<bool>;

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
    async fn query_media(
        &self,
        query: ferrex_core::query::MediaQuery,
    ) -> RepositoryResult<Vec<ferrex_core::query::MediaWithStatus>>;

    /// Check if setup is required
    async fn check_setup_status(
        &self,
    ) -> RepositoryResult<crate::infrastructure::api_client::SetupStatus>;

    /// Create initial admin user during setup
    async fn create_initial_admin(
        &self,
        username: String,
        password: String,
        pin: Option<String>,
    ) -> RepositoryResult<(ferrex_core::user::User, AuthToken)>;

    /// Make a GET request to a public endpoint (no auth)
    async fn get_public<T: for<'de> Deserialize<'de>>(&self, path: &str) -> RepositoryResult<T>;

    /// Build a full URL from a path
    fn build_url(&self, path: &str) -> String;

    /// Get the base URL
    fn base_url(&self) -> &str;

    /// Set the authentication token
    async fn set_token(&self, token: Option<AuthToken>);

    /// Get the current authentication token
    async fn get_token(&self) -> Option<AuthToken>;
}
