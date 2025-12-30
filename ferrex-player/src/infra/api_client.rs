use ferrex_core::{
    api::routes::v1,
    player_prelude::{
        ApiResponse, AuthToken, AuthenticatedDevice, ConfirmClaimRequest,
        ConfirmClaimResponse, MediaQuery, MediaWithStatus, StartClaimRequest,
        StartClaimResponse, UpdateProgressRequest, UserWatchState,
    },
};

use anyhow::{Context, Result};
use ferrex_model::image::ImageQuery;
use log::{info, warn};
use reqwest::{Client, RequestBuilder, StatusCode};
use rkyv::util::AlignedVec;
use serde::{Serialize, de::DeserializeOwned};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};

use crate::infra::services::api::ImageFetchResult;

/// Callback for token refresh
pub type RefreshTokenCallback = Arc<
    Mutex<
        Option<
            Box<
                dyn Fn() -> std::pin::Pin<
                        Box<
                            dyn std::future::Future<Output = Result<AuthToken>>
                                + Send,
                        >,
                    > + Send
                    + Sync,
            >,
        >,
    >,
>;

/// API client with authentication support
#[derive(Clone)]
pub struct ApiClient {
    pub(crate) client: Client,
    base_url: String,
    api_version: String,
    token_store: Arc<RwLock<Option<AuthToken>>>,
    refresh_callback: RefreshTokenCallback,
}

impl std::fmt::Debug for ApiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiClient")
            .field("base_url", &self.base_url)
            .field("api_version", &self.api_version)
            .field(
                "has_token",
                &self
                    .token_store
                    .try_read()
                    .map(|t| t.is_some())
                    .unwrap_or(false),
            )
            .finish()
    }
}

impl ApiClient {
    /// Create a new API client
    pub fn new(base_url: String) -> Self {
        // Normalize the provided base URL so we don't trip over missing schemes
        // Rationale: many users will provide "localhost:3000" which reqwest rejects.
        // We add http:// if missing and trim a trailing slash to prevent double slashes.
        fn normalize(raw: String) -> String {
            let original = raw.clone();
            let trimmed = raw.trim().trim_end_matches('/').to_string();
            let with_scheme = if trimmed.starts_with("http://")
                || trimmed.starts_with("https://")
            {
                trimmed
            } else {
                format!("http://{}", trimmed)
            };
            if with_scheme != original {
                log::warn!(
                    "[ApiClient] Normalized base URL from '{}' to '{}'",
                    original,
                    with_scheme
                );
            }
            with_scheme
        }

        let base_url = normalize(base_url);
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            // In development, don't follow redirects to avoid HTTP->HTTPS issues
            .redirect(if cfg!(debug_assertions) {
                reqwest::redirect::Policy::none()
            } else {
                reqwest::redirect::Policy::default()
            })
            .danger_accept_invalid_certs(cfg!(debug_assertions)) // Accept self-signed certs in dev
            .build()
            .expect("Failed to create HTTP client");

        info!(
            "[ApiClient] Creating new API client with base URL: {}",
            base_url
        );

        Self {
            client,
            base_url,
            api_version: "v1".to_string(),
            token_store: Arc::new(RwLock::new(None)),
            refresh_callback: Arc::new(Mutex::new(None)),
        }
    }

    fn rkyv_timeout_for_url(url: &str) -> Duration {
        // The rkyv "snapshot" endpoints can be very large (libraries + media),
        // and can legitimately take longer than the default reqwest client
        // timeout under real-world libraries and slower disks/DBs.
        //
        // The regression observed in 2025-12-15 logs is consistent with the
        // global 30s client timeout being too low for `/api/v1/libraries`.
        //
        // Keep typical rkyv endpoints snappy, but allow library snapshots to
        // complete without spurious timeouts.
        let default = Duration::from_secs(30);
        let long_snapshot = Duration::from_secs(180);

        let Ok(parsed) = reqwest::Url::parse(url) else {
            return default;
        };
        let path = parsed.path();

        // Libraries collection snapshot: `/api/v1/libraries`
        if path.ends_with("/api/v1/libraries") {
            return long_snapshot;
        }

        // Per-library media snapshot: `/api/v1/libraries/{id}/media`
        if path.contains("/api/v1/libraries/") && path.ends_with("/media") {
            return long_snapshot;
        }

        // Movie batch snapshots can also be large, especially the bundle endpoint:
        // `/api/v1/libraries/{id}/movie-batches`.
        if path.contains("/api/v1/libraries/")
            && path.contains("/movie-batches")
        {
            return long_snapshot;
        }

        default
    }

    /// Build a versioned API URL
    pub fn build_url(&self, path: impl AsRef<str>) -> String {
        let p = path.as_ref();
        if p.starts_with("http://") || p.starts_with("https://") {
            return p.to_string();
        }
        if p.contains("api/v1/") {
            let path = p.trim_start_matches('/');
            format!("{}/{}", self.base_url, path)
        } else {
            let path = p.trim_start_matches('/');
            format!("{}/api/{}/{}", self.base_url, self.api_version, path)
        }
    }

    /// Get the base URL
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Set the authentication token
    pub async fn set_token(&self, token: Option<AuthToken>) {
        *self.token_store.write().await = token;
    }

    /// Get the current authentication token
    pub async fn get_token(&self) -> Option<AuthToken> {
        self.token_store.read().await.clone()
    }

    /// Set the token refresh callback
    pub async fn set_refresh_callback<F, Fut>(&self, callback: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<AuthToken>> + Send + 'static,
    {
        let boxed_callback = Box::new(move || -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<AuthToken>> + Send>> {
            Box::pin(callback())
        });
        *self.refresh_callback.lock().await = Some(boxed_callback);
    }

    /// Build a request with authentication headers
    pub async fn build_request(
        &self,
        builder: RequestBuilder,
    ) -> RequestBuilder {
        if let Some(token) = self.token_store.read().await.as_ref() {
            builder.header(
                "Authorization",
                format!("Bearer {}", token.access_token),
            )
        } else {
            builder
        }
    }

    /// Build a request WITHOUT authentication headers (for public endpoints)
    fn build_public_request(&self, builder: RequestBuilder) -> RequestBuilder {
        // Don't add any auth headers for public endpoints
        builder
    }

    /// Execute a request and handle common errors
    async fn execute_request<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
    ) -> Result<T> {
        // Clone the request for potential retry
        let request_clone = request.try_clone();
        let response = request.send().await?;

        match response.status() {
            status if status.is_success() => {
                if status == StatusCode::NO_CONTENT {
                    return Err(anyhow::anyhow!(
                        "Empty response from server (204 No Content)"
                    ));
                }
                let api_response: ApiResponse<T> = response.json().await?;
                match api_response.data {
                    Some(data) => Ok(data),
                    None => Err(anyhow::anyhow!("Empty response from server")),
                }
            }
            StatusCode::UNAUTHORIZED => {
                // Try to refresh token if we have a callback
                if let Some(request_retry) = request_clone
                    && let Some(ref callback) =
                        *self.refresh_callback.lock().await
                {
                    info!("[ApiClient] Token expired, attempting refresh");
                    match callback().await {
                        Ok(new_token) => {
                            info!(
                                "[ApiClient] Token refreshed successfully, retrying request"
                            );
                            self.set_token(Some(new_token.clone())).await;

                            // Rebuild request with new token and execute without retry
                            let retry_request =
                                self.build_request(request_retry).await;
                            return self
                                .execute_request_without_retry(retry_request)
                                .await;
                        }
                        Err(e) => {
                            warn!("[ApiClient] Token refresh failed: {}", e);
                            // Fall through to clear token and return error
                        }
                    }
                }

                // Token refresh failed or not available, clear token
                self.set_token(None).await;
                Err(anyhow::anyhow!("Unauthorized - please login again"))
            }
            status => {
                let error_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string());
                Err(anyhow::anyhow!(
                    "Request failed with status {}: {}",
                    status,
                    error_text
                ))
            }
        }
    }

    /// Execute a request without retry (to avoid recursion)
    async fn execute_request_without_retry<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
    ) -> Result<T> {
        let response = request.send().await?;

        match response.status() {
            status if status.is_success() => {
                if status == StatusCode::NO_CONTENT {
                    return Err(anyhow::anyhow!(
                        "Empty response from server (204 No Content)"
                    ));
                }
                let api_response: ApiResponse<T> = response.json().await?;
                match api_response.data {
                    Some(data) => Ok(data),
                    None => Err(anyhow::anyhow!("Empty response from server")),
                }
            }
            StatusCode::UNAUTHORIZED => {
                // Don't retry, just clear token and return error
                self.set_token(None).await;
                Err(anyhow::anyhow!("Unauthorized - please login again"))
            }
            status => {
                let error_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string());
                Err(anyhow::anyhow!(
                    "Request failed with status {}: {}",
                    status,
                    error_text
                ))
            }
        }
    }

    /// Execute a request that returns rkyv binary data
    pub async fn execute_rkyv_request(
        &self,
        request: RequestBuilder,
    ) -> Result<Vec<u8>> {
        // Add Accept header for rkyv format
        let request = request.header("Accept", "application/octet-stream");
        let response = request.send().await?;

        match response.status() {
            StatusCode::OK => {
                // Check content type
                let content_type = response
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");

                if content_type.contains("application/octet-stream") {
                    // Return raw bytes for the caller to deserialize
                    let bytes = response.bytes().await?;
                    Ok(bytes.to_vec())
                } else {
                    Err(anyhow::anyhow!(
                        "Expected octet-stream response but got {}",
                        content_type
                    ))
                }
            }
            StatusCode::UNAUTHORIZED => {
                self.set_token(None).await;
                Err(anyhow::anyhow!("Unauthorized - please login again"))
            }
            status => {
                let error_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string());
                Err(anyhow::anyhow!(
                    "Request failed with status {}: {}",
                    status,
                    error_text
                ))
            }
        }
    }

    /// Execute a request for setup status (handles different response format)
    async fn execute_setup_request(
        &self,
        request: RequestBuilder,
    ) -> Result<SetupStatus> {
        let response = request.send().await?;

        match response.status() {
            StatusCode::OK => {
                let status: SetupStatus = response.json().await?;
                Ok(status)
            }
            status => {
                let error_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string());
                Err(anyhow::anyhow!(
                    "Setup status request failed with status {}: {}",
                    status,
                    error_text
                ))
            }
        }
    }

    /// Check if initial setup is required
    pub async fn check_setup_status(&self) -> Result<bool> {
        let url = format!("{}/setup/status", self.base_url);
        let request = self.client.get(&url);
        // Don't use auth for setup status check
        let status = self.execute_setup_request(request).await?;
        Ok(status.needs_setup)
    }

    /// POST request with authentication
    pub async fn post<T: Serialize, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<R> {
        let url = self.build_url(path);

        let request = self.client.post(&url).json(body);
        let request = self.build_request(request).await;
        self.execute_request(request).await
    }

    /// POST request for endpoints that return 204 No Content
    pub async fn post_no_content<T: Serialize>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<()> {
        let url = self.build_url(path);

        let request = self.client.post(&url).json(body);
        let request = self.build_request(request).await;

        // Execute request with special handling for 204 No Content
        let request_clone = request.try_clone();
        let response = request.send().await?;

        match response.status() {
            StatusCode::OK | StatusCode::NO_CONTENT => Ok(()),
            StatusCode::UNAUTHORIZED => {
                // Try to refresh token if we have a callback
                if let Some(request_retry) = request_clone
                    && let Some(ref callback) =
                        *self.refresh_callback.lock().await
                {
                    info!("[ApiClient] Token expired, attempting refresh");
                    match callback().await {
                        Ok(new_token) => {
                            info!(
                                "[ApiClient] Token refreshed successfully, retrying request"
                            );
                            self.set_token(Some(new_token.clone())).await;

                            // Rebuild request with new token and retry
                            let retry_request =
                                self.build_request(request_retry).await;
                            let retry_response = retry_request.send().await?;

                            match retry_response.status() {
                                StatusCode::OK | StatusCode::NO_CONTENT => {
                                    return Ok(());
                                }
                                _ => {
                                    let error_text = retry_response
                                        .text()
                                        .await
                                        .unwrap_or_else(|_| {
                                            "Unknown error".to_string()
                                        });
                                    return Err(anyhow::anyhow!(
                                        "Request failed after retry: {}",
                                        error_text
                                    ));
                                }
                            }
                        }
                        Err(e) => {
                            warn!("[ApiClient] Token refresh failed: {}", e);
                        }
                    }
                }

                // Token refresh failed or not available
                self.set_token(None).await;
                Err(anyhow::anyhow!("Unauthorized - please login again"))
            }
            status => {
                let error_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string());
                Err(anyhow::anyhow!(
                    "Request failed with status {}: {}",
                    status,
                    error_text
                ))
            }
        }
    }

    /// GET request with authentication, returns raw rkyv bytes (structured data only)
    pub async fn get_rkyv(
        &self,
        path: &str,
        query: Option<(&str, &str)>,
    ) -> Result<AlignedVec> {
        let url = self.build_url(path);

        // Debug logging
        log::debug!("GET rkyv request to: {}", url);

        let request = self.client.get(&url);
        let request = if let Some(query) = query {
            request.query(&[query])
        } else {
            request
        };
        let request = self.build_request(request).await;

        //// Add Accept header for rkyv format
        let timeout = Self::rkyv_timeout_for_url(&url);
        if timeout > Duration::from_secs(30) {
            log::debug!(
                "[ApiClient] Using extended timeout {:?} for {}",
                timeout,
                url
            );
        }
        let request = request
            .header("Accept", "application/octet-stream")
            .timeout(timeout);

        let response = request.send().await.with_context(|| {
            format!("GET rkyv {} (timeout {:?})", url, timeout)
        })?;

        match response.status() {
            StatusCode::OK => {
                // Check content type
                let content_type = response
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");

                if content_type.contains("application/octet-stream") {
                    let size_hint =
                        response.content_length().unwrap_or(1024 * 1024)
                            as usize;
                    let mut aligned = AlignedVec::with_capacity(size_hint);
                    let bytes = response.bytes().await?;
                    aligned.extend_from_slice(&bytes);
                    if aligned.capacity() > aligned.len() * 2 {
                        aligned.shrink_to_fit();
                    }
                    Ok(aligned)
                } else {
                    Err(anyhow::anyhow!(
                        "Expected application/octet-stream from {} but got '{}'",
                        url,
                        content_type
                    ))
                }
            }
            StatusCode::UNAUTHORIZED => {
                self.set_token(None).await;
                Err(anyhow::anyhow!("Unauthorized - please login again"))
            }
            status => {
                let error_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string());
                Err(anyhow::anyhow!(
                    "Request failed with status {}: {}",
                    status,
                    error_text
                ))
            }
        }
    }

    /// GET request with authentication, returns raw bytes (for images)
    pub async fn get_bytes(
        &self,
        path: &str,
        query: Option<(&str, &str)>,
    ) -> Result<Vec<u8>> {
        let url = self.build_url(path);

        log::debug!("GET (bytes) request to: {}", url);

        let mut request = self.client.get(&url);
        if let Some((k, v)) = query {
            request = request.query(&[(k, v)]);
        }
        let request = self
            .build_request(request)
            .await
            .header("Accept", "image/jpeg,image/*");

        //;q=0.9,*/*;q=0.8
        // // Avoid compressed transfer encodings for ranged/partial hazards.
        // .header("Accept-Encoding", "identity");

        let response = request.send().await?;
        match response.status() {
            StatusCode::OK => {
                // Capture expected content length (if any) for diagnostics
                let cl = response
                    .headers()
                    .get(reqwest::header::CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<usize>().ok());
                let encoding = response
                    .headers()
                    .get(reqwest::header::CONTENT_ENCODING)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());

                let bytes = response.bytes().await?;
                if let Some(expected) = cl
                    && expected != bytes.len()
                {
                    // Treat mismatches as hard errors to avoid decoding partial/corrupt images.
                    return Err(anyhow::anyhow!(
                        "Content-Length mismatch for {}: header={} actual={} encoding={:?}",
                        url,
                        expected,
                        bytes.len(),
                        encoding
                    ));
                }
                Ok(bytes.to_vec())
            }
            StatusCode::UNAUTHORIZED => {
                self.set_token(None).await;
                Err(anyhow::anyhow!("Unauthorized - please login again"))
            }
            status => {
                let error_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string());
                Err(anyhow::anyhow!(
                    "Request failed with status {}: {}",
                    status,
                    error_text
                ))
            }
        }
    }

    /// GET request for images (size is carried via query params; no custom header)
    pub async fn get_image(
        &self,
        path: &str,
        image_query: ImageQuery,
    ) -> Result<ImageFetchResult> {
        let url = self.build_url(path);
        log::debug!("GET (image) request to: {}", url);

        let request = self.client.get(&url);
        let request = self
            .build_request(request)
            .await
            .header("Accept", "image/jpeg")
            .json(&image_query);

        let response = request.send().await?;

        match response.status() {
            StatusCode::OK => {
                let cl = response
                    .headers()
                    .get(reqwest::header::CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<usize>().ok());
                let encoding = response
                    .headers()
                    .get(reqwest::header::CONTENT_ENCODING)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());

                let bytes = response.bytes().await?;
                if let Some(expected) = cl
                    && expected != bytes.len()
                {
                    return Err(anyhow::anyhow!(
                        "Content-Length mismatch for {}: header={} actual={} encoding={:?}",
                        url,
                        expected,
                        bytes.len(),
                        encoding
                    ));
                }
                Ok(ImageFetchResult::Ready(bytes.to_vec()))
            }
            StatusCode::ACCEPTED => {
                let retry_after = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|raw| raw.parse::<u64>().ok())
                    .map(Duration::from_secs);
                Ok(ImageFetchResult::Pending { retry_after })
            }
            StatusCode::UNAUTHORIZED => {
                self.set_token(None).await;
                Err(anyhow::anyhow!("Unauthorized - please login again"))
            }
            status => {
                let error_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string());
                Err(anyhow::anyhow!(
                    "Request failed with status {}: {}",
                    status,
                    error_text
                ))
            }
        }
    }

    /// GET request with authentication
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = self.build_url(path);

        // Debug logging
        log::debug!("GET request to: {}", url);
        log::debug!("Base URL: {}", self.base_url);

        let request = self.client.get(&url);
        let request = self.build_request(request).await;
        self.execute_request(request).await
    }

    /// GET request for public endpoints (no authentication)
    pub async fn get_public<T: DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T> {
        let url = self.build_url(path);

        log::debug!("[ApiClient] GET (public) request to: {}", url);

        let request = self.client.get(&url);
        let request = self.build_public_request(request);
        self.execute_request(request).await
    }

    /// PUT request
    pub async fn put<T: Serialize, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<R> {
        let url = self.build_url(path);

        let request = self.client.put(&url).json(body);
        let request = self.build_request(request).await;
        self.execute_request(request).await
    }

    /// DELETE request
    pub async fn delete<R: DeserializeOwned>(&self, path: &str) -> Result<R> {
        let url = self.build_url(path);

        let request = self.client.delete(&url);
        let request = self.build_request(request).await;
        self.execute_request(request).await
    }
}

impl ApiClient {
    /// Get watch state for the current user
    pub async fn get_watch_state(&self) -> Result<UserWatchState> {
        self.get(v1::watch::STATE).await
    }

    /// Update watch progress for a media item
    pub async fn update_progress(
        &self,
        request: &UpdateProgressRequest,
    ) -> Result<()> {
        // This endpoint returns 204 No Content, so we need special handling
        self.post_no_content(v1::watch::UPDATE_PROGRESS, request)
            .await
    }

    /// Create initial admin user during setup
    pub async fn create_initial_admin(
        &self,
        username: String,
        password: String,
        display_name: Option<String>,
        setup_token: Option<String>,
        claim_token: Option<String>,
    ) -> Result<AuthToken> {
        #[derive(Serialize)]
        struct AdminSetupRequest {
            username: String,
            password: String,
            display_name: Option<String>,
            setup_token: Option<String>,
            claim_token: Option<String>,
        }

        let request = AdminSetupRequest {
            username,
            password,
            display_name,
            setup_token,
            claim_token,
        };

        self.post(v1::setup::CREATE_ADMIN, &request).await
    }

    /// Start the secure claim flow for first-run binding
    pub async fn start_setup_claim(
        &self,
        device_name: Option<String>,
    ) -> Result<StartClaimResponse> {
        let request = StartClaimRequest { device_name };
        self.post(v1::setup::CLAIM_START, &request).await
    }

    /// Confirm a secure claim using the provided claim code
    pub async fn confirm_setup_claim(
        &self,
        claim_code: &str,
    ) -> Result<ConfirmClaimResponse> {
        let request = ConfirmClaimRequest {
            claim_code: claim_code.to_string(),
        };
        self.post(v1::setup::CLAIM_CONFIRM, &request).await
    }

    /// Get auth header for the current session
    pub async fn get_auth_header(&self) -> Option<String> {
        self.token_store
            .read()
            .await
            .as_ref()
            .map(|token| format!("Bearer {}", token.access_token))
    }

    /// List user devices
    pub async fn list_user_devices(&self) -> Result<Vec<AuthenticatedDevice>> {
        self.get(v1::auth::device::LIST).await
    }

    /// Revoke a device
    pub async fn revoke_device(&self, device_id: uuid::Uuid) -> Result<()> {
        #[derive(Serialize)]
        struct RevokeDeviceRequest {
            device_id: uuid::Uuid,
        }

        let payload = RevokeDeviceRequest { device_id };

        self.post::<_, ()>(v1::auth::device::REVOKE, &payload)
            .await?;
        Ok(())
    }

    /// Execute a media query
    pub async fn query_media(
        &self,
        query: MediaQuery,
    ) -> Result<Vec<MediaWithStatus>> {
        self.post(v1::media::QUERY, &query).await
    }
}

/// Server setup status
#[derive(Debug, Clone, serde::Deserialize)]
pub struct SetupStatus {
    pub needs_setup: bool,
    pub has_admin: bool,
    #[serde(default)]
    pub requires_setup_token: bool,
    pub user_count: usize,
    pub library_count: usize,
}
