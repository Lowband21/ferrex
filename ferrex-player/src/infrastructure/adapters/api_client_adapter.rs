//! ApiClient adapter that implements ApiService trait
//!
//! Wraps the existing ApiClient to provide a trait-based interface

use async_trait::async_trait;
use rkyv::rancor::Error;
use rkyv::util::AlignedVec;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::infrastructure::ApiClient;
use crate::infrastructure::api_client::SetupStatus;
use crate::infrastructure::constants::routes;
use crate::infrastructure::constants::routes::utils::replace_param;
use crate::infrastructure::repository::{RepositoryError, RepositoryResult};
use crate::infrastructure::services::api::ApiService;
use ferrex_core::Media;
use ferrex_core::auth::device::AuthenticatedDevice;
use ferrex_core::types::library::Library;
use ferrex_core::user::{AuthToken, User};
use ferrex_core::watch_status::{UpdateProgressRequest, UserWatchState};

/// Adapter that implements ApiService using the existing ApiClient
#[derive(Debug, Clone)]
pub struct ApiClientAdapter {
    client: Arc<ApiClient>,
}

impl ApiClientAdapter {
    pub fn new(client: Arc<ApiClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ApiService for ApiClientAdapter {
    async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str) -> RepositoryResult<T> {
        self.client
            .get(path)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    async fn get_rkyv(
        &self,
        path: &str,
        query: Option<(&str, &str)>,
    ) -> RepositoryResult<AlignedVec> {
        let bytes = self.client.get_rkyv(path, query).await;
        bytes.map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    async fn get_bytes(
        &self,
        path: &str,
        query: Option<(&str, &str)>,
    ) -> RepositoryResult<Vec<u8>> {
        self.client
            .get_bytes(path, query)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    async fn post<T: for<'de> Deserialize<'de>, B: Serialize + Send + Sync>(
        &self,
        path: &str,
        body: &B,
    ) -> RepositoryResult<T> {
        self.client
            .post(path, body)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    async fn put<T: for<'de> Deserialize<'de>, B: Serialize + Send + Sync>(
        &self,
        path: &str,
        body: &B,
    ) -> RepositoryResult<T> {
        self.client
            .put(path, body)
            .await
            .map_err(|e| RepositoryError::UpdateFailed(e.to_string()))
    }

    async fn delete<T: for<'de> Deserialize<'de>>(&self, path: &str) -> RepositoryResult<T> {
        self.client
            .delete(path)
            .await
            .map_err(|e| RepositoryError::DeleteFailed(e.to_string()))
    }

    async fn fetch_libraries(&self) -> RepositoryResult<Vec<Library>> {
        self.get("/libraries").await
    }

    async fn fetch_library_media(&self, library_id: Uuid) -> RepositoryResult<Vec<Media>> {
        use ferrex_core::LibraryMediaResponse;

        // Build URL for the library media endpoint
        let url = self.client.build_url(
            &replace_param(routes::libraries::GET_MEDIA, ":id", library_id.to_string()),
            false,
        );
        log::info!("Fetching library media from {}", url);
        let request = self.client.client.get(&url);
        let request = self.client.build_request(request).await;

        // Use the rkyv request method to get binary response
        let bytes = self
            .client
            .execute_rkyv_request::<LibraryMediaResponse>(request)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))?;

        // Deserialize from rkyv bytes
        // For now, we deserialize to owned types for simplicity
        // Later we can optimize to work directly with archived data for zero-copy
        let response = rkyv::from_bytes::<LibraryMediaResponse, rkyv::rancor::Error>(&bytes)
            .map_err(|e| {
                RepositoryError::QueryFailed(format!("Failed to deserialize rkyv data: {:?}", e))
            })?;

        Ok(response.media)
    }

    async fn scan_library(&self, library_id: Uuid) -> RepositoryResult<()> {
        #[derive(Serialize)]
        struct ScanRequest {
            library_id: Uuid,
        }

        #[derive(Deserialize)]
        struct ScanResponse {
            message: String,
        }

        let _response: ScanResponse = self
            .post(
                &format!("/libraries/{}/scan", library_id),
                &ScanRequest { library_id },
            )
            .await?;

        Ok(())
    }

    async fn health_check(&self) -> RepositoryResult<bool> {
        #[derive(Deserialize)]
        struct HealthResponse {
            status: String,
        }

        match self.get::<HealthResponse>("/health").await {
            Ok(response) => Ok(response.status == "ok" || response.status == "healthy"),
            Err(_) => Ok(false),
        }
    }

    async fn get_watch_state(&self) -> RepositoryResult<UserWatchState> {
        self.client
            .get_watch_state()
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    async fn update_progress(&self, request: &UpdateProgressRequest) -> RepositoryResult<()> {
        self.client
            .update_progress(request)
            .await
            .map_err(|e| RepositoryError::UpdateFailed(e.to_string()))
    }

    async fn list_user_devices(&self) -> RepositoryResult<Vec<AuthenticatedDevice>> {
        self.client
            .list_user_devices()
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    async fn revoke_device(&self, device_id: Uuid) -> RepositoryResult<()> {
        self.client
            .revoke_device(device_id)
            .await
            .map_err(|e| RepositoryError::UpdateFailed(e.to_string()))
    }

    async fn query_media(
        &self,
        query: ferrex_core::query::MediaQuery,
    ) -> RepositoryResult<Vec<ferrex_core::query::MediaWithStatus>> {
        self.client
            .query_media(query)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    async fn check_setup_status(&self) -> RepositoryResult<SetupStatus> {
        // ApiClient's check_setup_status returns bool, but we need SetupStatus
        // Call the endpoint directly to get the full status
        self.get::<SetupStatus>("/setup/status").await
    }

    async fn create_initial_admin(
        &self,
        username: String,
        password: String,
        pin: Option<String>,
    ) -> RepositoryResult<(User, AuthToken)> {
        // The ApiClient method has 4 parameters, we only have 3
        // The fourth parameter is setup_token
        let token = self
            .client
            .create_initial_admin(username, password, pin, None)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))?;

        // Now get the user info
        let user: User = self.get("/users/me").await?;

        Ok((user, token))
    }

    async fn get_public<T: for<'de> Deserialize<'de>>(&self, path: &str) -> RepositoryResult<T> {
        self.client
            .get_public(path)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    fn build_url(&self, path: &str) -> String {
        self.client.build_url(path, false)
    }

    fn base_url(&self) -> &str {
        self.client.base_url()
    }

    async fn set_token(&self, token: Option<AuthToken>) {
        self.client.set_token(token).await
    }

    async fn get_token(&self) -> Option<AuthToken> {
        self.client.get_token().await
    }
}
