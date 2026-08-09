//! Concrete HTTP client for Ferrex player API calls.
//!
//! The client owns base URL handling, authentication token refresh callbacks,
//! request/response decoding, and endpoint helpers used by higher-level service
//! adapters.

use ferrex_core::{
    api::{
        routes::{utils::replace_param, v1},
        types::collections::*,
    },
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

use crate::services::api::ImageFetchResult;

pub use ferrex_player_foundation::auth::SetupStatus;

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

fn decode_api_response<T>(response: ApiResponse<T>) -> Result<T> {
    let ApiResponse {
        status,
        data,
        error,
        message,
    } = response;

    if status != "success" || error.is_some() {
        let detail = error.or(message).unwrap_or_else(|| {
            format!("Server returned API status '{status}'")
        });
        return Err(anyhow::anyhow!(detail));
    }

    data.ok_or_else(|| anyhow::anyhow!("Empty response from server"))
}

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
            let mut normalized = with_scheme.trim_end_matches('/').to_string();

            // Back-compat: if a user pastes an API base like `http://host:port/api/v1`,
            // strip the version suffix so callers can pass `/api/v1/...` and `/api/v2/...`.
            for suffix in ["/api/v1", "/api/v2"] {
                if normalized.ends_with(suffix) {
                    normalized.truncate(normalized.len() - suffix.len());
                    normalized = normalized.trim_end_matches('/').to_string();
                    break;
                }
            }
            if normalized != original {
                log::warn!(
                    "[ApiClient] Normalized base URL from '{}' to '{}'",
                    original,
                    normalized
                );
            }
            normalized
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

        // If the caller provides an absolute API path (e.g. `/api/v1/...` or `/api/v2/...`),
        // treat it as already versioned and do not prepend `api_version`.
        let path = p.trim_start_matches('/');
        if path.starts_with("api/") {
            format!("{}/{}", self.base_url, path)
        } else {
            format!("{}/api/{}/{}", self.base_url, self.api_version, path)
        }
    }

    /// Get the base URL
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Access the underlying HTTP client for API calls that need custom headers.
    pub fn http_client(&self) -> &Client {
        &self.client
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
                decode_api_response(api_response)
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
                decode_api_response(api_response)
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

    /// POST request with authentication, returns raw rkyv bytes.
    pub async fn post_rkyv(
        &self,
        path: &str,
        body: Vec<u8>,
    ) -> Result<AlignedVec> {
        let url = self.build_url(path);
        log::debug!("POST rkyv request to: {}", url);

        let request = self.client.post(&url).body(body);
        let request = self
            .build_request(request)
            .await
            .header("Accept", "application/octet-stream")
            .header("Content-Type", "application/octet-stream");

        let timeout = Self::rkyv_timeout_for_url(&url);
        let request = request.timeout(timeout);

        let response = request.send().await.with_context(|| {
            format!("POST rkyv {} (timeout {:?})", url, timeout)
        })?;

        match response.status() {
            StatusCode::OK => {
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

    /// GET request with authentication and a serializable query string.
    pub async fn get_with_query<Q, R>(&self, path: &str, query: &Q) -> Result<R>
    where
        Q: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        let url = self.build_url(path);

        log::debug!("GET request to: {}", url);
        log::debug!("Base URL: {}", self.base_url);

        let request = self.client.get(&url).query(query);
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

    /// DELETE request with a JSON body.
    pub async fn delete_with_body<T: Serialize, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<R> {
        let url = self.build_url(path);

        let request = self.client.delete(&url).json(body);
        let request = self.build_request(request).await;
        self.execute_request(request).await
    }
}

impl ApiClient {
    fn collection_path(route: &str, collection_id: CollectionId) -> String {
        replace_param(route, "{collection_id}", collection_id.to_string())
    }

    /// List player collections with filtering and pagination.
    pub async fn list_collections(
        &self,
        request: &ListCollectionsRequest,
    ) -> Result<ListCollectionsResponse> {
        self.get_with_query(v1::collections::COLLECTION, request)
            .await
    }

    /// Fetch collection detail and optional rule/item/shelf expansions.
    pub async fn get_collection_detail(
        &self,
        collection_id: CollectionId,
        request: &GetCollectionDetailRequest,
    ) -> Result<GetCollectionDetailResponse> {
        let path = Self::collection_path(v1::collections::ITEM, collection_id);
        self.get_with_query(&path, request).await
    }

    /// List materialized collection members with pagination.
    pub async fn list_collection_items(
        &self,
        collection_id: CollectionId,
        request: &ListCollectionItemsRequest,
    ) -> Result<ListCollectionItemsResponse> {
        let path = Self::collection_path(v1::collections::ITEMS, collection_id);
        self.get_with_query(&path, request).await
    }

    /// Create a collection definition.
    pub async fn create_collection(
        &self,
        request: &CreateCollectionRequest,
    ) -> Result<CreateCollectionResponse> {
        self.post(v1::collections::COLLECTION, request).await
    }

    /// Update a collection definition.
    pub async fn update_collection(
        &self,
        collection_id: CollectionId,
        request: &UpdateCollectionRequest,
    ) -> Result<UpdateCollectionResponse> {
        let path = Self::collection_path(v1::collections::ITEM, collection_id);
        self.put(&path, request).await
    }

    /// Archive or unarchive a collection.
    pub async fn archive_collection(
        &self,
        collection_id: CollectionId,
        request: &ArchiveCollectionRequest,
    ) -> Result<ArchiveCollectionResponse> {
        let path =
            Self::collection_path(v1::collections::ARCHIVE, collection_id);
        self.post(&path, request).await
    }

    /// Delete a collection definition.
    pub async fn delete_collection(
        &self,
        collection_id: CollectionId,
        request: &DeleteCollectionRequest,
    ) -> Result<DeleteCollectionResponse> {
        let path = Self::collection_path(v1::collections::ITEM, collection_id);
        self.delete_with_body(&path, request).await
    }

    /// Add items to a manual collection.
    pub async fn manual_add_collection_items(
        &self,
        collection_id: CollectionId,
        request: &ManualAddCollectionItemsRequest,
    ) -> Result<ManualAddCollectionItemsResponse> {
        let path = Self::collection_path(
            v1::collections::MANUAL_ADD_ITEMS,
            collection_id,
        );
        self.post(&path, request).await
    }

    /// Remove items from a manual collection.
    pub async fn manual_remove_collection_items(
        &self,
        collection_id: CollectionId,
        request: &ManualRemoveCollectionItemsRequest,
    ) -> Result<ManualRemoveCollectionItemsResponse> {
        let path = Self::collection_path(
            v1::collections::MANUAL_REMOVE_ITEMS,
            collection_id,
        );
        self.post(&path, request).await
    }

    /// Reorder items in a manual collection.
    pub async fn manual_reorder_collection_items(
        &self,
        collection_id: CollectionId,
        request: &ManualReorderCollectionItemsRequest,
    ) -> Result<ManualReorderCollectionItemsResponse> {
        let path = Self::collection_path(
            v1::collections::MANUAL_REORDER_ITEMS,
            collection_id,
        );
        self.post(&path, request).await
    }

    /// Validate a dynamic collection rule.
    pub async fn validate_collection_rule(
        &self,
        request: &ValidateCollectionRuleRequest,
    ) -> Result<ValidateCollectionRuleResponse> {
        self.post(v1::collections::RULE_VALIDATE, request).await
    }

    /// Preview dynamic collection rule results.
    pub async fn preview_collection_rule(
        &self,
        request: &PreviewCollectionRuleRequest,
    ) -> Result<PreviewCollectionRuleResponse> {
        self.post(v1::collections::RULE_PREVIEW, request).await
    }

    /// Refresh a collection's dynamic rule materialization.
    pub async fn refresh_collection_rule(
        &self,
        collection_id: CollectionId,
        request: &RefreshCollectionRuleRequest,
    ) -> Result<RefreshCollectionRuleResponse> {
        let path =
            Self::collection_path(v1::collections::RULE_REFRESH, collection_id);
        self.post(&path, request).await
    }

    /// List shelf placements.
    pub async fn list_shelf_placements(
        &self,
        request: &ListShelfPlacementsRequest,
    ) -> Result<ListShelfPlacementsResponse> {
        self.get_with_query(v1::shelves::PLACEMENTS, request).await
    }

    /// Pin or unpin a collection on a shelf.
    pub async fn pin_shelf_placement(
        &self,
        request: &PinShelfPlacementRequest,
    ) -> Result<PinShelfPlacementResponse> {
        self.post(v1::shelves::PIN_PLACEMENT, request).await
    }

    /// Reorder shelf placements.
    pub async fn reorder_shelf_placements(
        &self,
        request: &ReorderShelfPlacementsRequest,
    ) -> Result<ReorderShelfPlacementsResponse> {
        self.post(v1::shelves::REORDER_PLACEMENTS, request).await
    }

    /// List TMDB collections available for import.
    pub async fn list_tmdb_collections(
        &self,
        request: &TmdbListCollectionsRequest,
    ) -> Result<TmdbListCollectionsResponse> {
        self.get_with_query(v1::collections::tmdb::LIST, request)
            .await
    }

    /// Import a TMDB collection/list or refresh an existing imported collection.
    pub async fn import_tmdb_collection(
        &self,
        request: &TmdbImportCollectionRequest,
    ) -> Result<TmdbImportCollectionResponse> {
        self.post(v1::collections::tmdb::IMPORT, request).await
    }

    /// Refresh an existing TMDB-backed collection using the import contract.
    pub async fn refresh_tmdb_collection(
        &self,
        request: &TmdbImportCollectionRequest,
    ) -> Result<TmdbImportCollectionResponse> {
        let mut request = request.clone();
        request.refresh_existing = true;
        self.import_tmdb_collection(&request).await
    }

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

        self.post_no_content(v1::auth::device::REVOKE, &payload)
            .await
    }

    /// Execute a media query
    pub async fn query_media(
        &self,
        query: MediaQuery,
    ) -> Result<Vec<MediaWithStatus>> {
        self.post(v1::media::QUERY, &query).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrex_model::{MediaID, MovieID};
    use uuid::Uuid;

    fn uuid(value: &str) -> Uuid {
        Uuid::parse_str(value).expect("valid uuid")
    }

    #[test]
    fn api_error_envelope_on_http_success_surfaces_server_detail() {
        let error = decode_api_response::<String>(ApiResponse::error(
            "library deletion blocked by provenance".to_string(),
        ))
        .expect_err(
            "error envelope must not be treated as a successful request",
        );

        assert_eq!(error.to_string(), "library deletion blocked by provenance");
    }

    #[test]
    fn api_success_envelope_returns_payload() {
        assert_eq!(
            decode_api_response(ApiResponse::success("deleted".to_string()))
                .expect("success envelope should return data"),
            "deleted"
        );
    }

    #[test]
    fn collection_paths_use_shared_route_constants() {
        let collection_id =
            CollectionId::from(uuid("018f0c8a-2eab-7f03-a989-1fd8f8f03a11"));

        assert_eq!(
            ApiClient::collection_path(v1::collections::ITEM, collection_id),
            "/api/v1/collections/018f0c8a-2eab-7f03-a989-1fd8f8f03a11"
        );
        assert_eq!(
            ApiClient::collection_path(v1::collections::ITEMS, collection_id),
            "/api/v1/collections/018f0c8a-2eab-7f03-a989-1fd8f8f03a11/items"
        );
        assert_eq!(
            ApiClient::collection_path(
                v1::collections::MANUAL_REORDER_ITEMS,
                collection_id,
            ),
            "/api/v1/collections/018f0c8a-2eab-7f03-a989-1fd8f8f03a11/items:reorder"
        );
        assert_eq!(
            ApiClient::collection_path(
                v1::collections::RULE_REFRESH,
                collection_id,
            ),
            "/api/v1/collections/018f0c8a-2eab-7f03-a989-1fd8f8f03a11/rule:refresh"
        );

        let client = ApiClient::new("https://ferrex.example/api/v1".into());
        assert_eq!(
            client.build_url(v1::shelves::PIN_PLACEMENT),
            "https://ferrex.example/api/v1/shelves/placements:pin"
        );
        assert_eq!(
            client.build_url(v1::collections::tmdb::LIST),
            "https://ferrex.example/api/v1/collections/tmdb/lists"
        );
    }

    #[test]
    fn collection_contract_dtos_round_trip_through_json() {
        let media_id = MediaID::Movie(MovieID(uuid(
            "018f0c8a-2eab-7f03-a989-1fd8f8f03a12",
        )));
        let create = CreateCollectionRequest {
            title: "Favorites".into(),
            description: Some("Movies to revisit".into()),
            kind: CollectionKind::Manual,
            source: CollectionSource::Manual,
            owner: CollectionOwner::default(),
            scope: CollectionScope::User,
            visibility: CollectionVisibility::Private,
            presentation: CollectionPresentationMode::Playlist,
            media_scope: CollectionMediaScope::ExplicitItems {
                item_keys: vec![CollectionMemberKey::for_media(&media_id)],
            },
            duplicate_policy: CollectionDuplicatePolicy::RejectDuplicates,
            artwork: CollectionArtwork::default(),
            theme: CollectionTheme::default(),
            provenance: None,
            rule: None,
        };
        let decoded: CreateCollectionRequest = serde_json::from_str(
            &serde_json::to_string(&create).expect("serialize create request"),
        )
        .expect("deserialize create request");
        assert_eq!(decoded, create);

        let add = ManualAddCollectionItemsRequest {
            items: vec![CollectionManualAddItem {
                media_id,
                title_override: Some("Arrival".into()),
                position: Some(7),
            }],
            duplicate_policy: Some(CollectionDuplicatePolicy::RejectDuplicates),
            expected_revision: Some(3),
        };
        let decoded: ManualAddCollectionItemsRequest = serde_json::from_str(
            &serde_json::to_string(&add).expect("serialize add request"),
        )
        .expect("deserialize add request");
        assert_eq!(decoded, add);

        let shelf = PinShelfPlacementRequest {
            collection_id: CollectionId::from(uuid(
                "018f0c8a-2eab-7f03-a989-1fd8f8f03a13",
            )),
            surface: ShelfSurface::Home,
            shelf_key: "home.hero".into(),
            pinned: true,
            position: Some(1),
            presentation: Some(CollectionPresentationMode::Hero),
        };
        let decoded: PinShelfPlacementRequest = serde_json::from_str(
            &serde_json::to_string(&shelf).expect("serialize shelf request"),
        )
        .expect("deserialize shelf request");
        assert_eq!(decoded, shelf);
    }
}
