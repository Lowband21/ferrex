use anyhow::Result;
use ferrex_core::api_types::{ApiResponse, MediaId};
use ferrex_core::user::AuthToken;
use ferrex_core::watch_status::{UpdateProgressRequest, UserWatchState};
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;
use tokio::sync::{RwLock, Mutex};
use log::{info, warn};

/// Callback for token refresh
pub type RefreshTokenCallback = Arc<Mutex<Option<Box<dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<AuthToken>> + Send>> + Send + Sync>>>>;

/// API client with authentication support
#[derive(Clone)]
pub struct ApiClient {
    client: Client,
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
            .field("has_token", &self.token_store.try_read().map(|t| t.is_some()).unwrap_or(false))
            .finish()
    }
}

impl ApiClient {
    /// Create a new API client
    pub fn new(base_url: String) -> Self {
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

        info!("[ApiClient] Creating new API client with base URL: {}", base_url);
        
        Self {
            client,
            base_url,
            api_version: "v1".to_string(),
            token_store: Arc::new(RwLock::new(None)),
            refresh_callback: Arc::new(Mutex::new(None)),
        }
    }

    /// Build a versioned API URL
    pub fn build_url(&self, path: &str) -> String {
        let path = path.trim_start_matches('/');
        format!("{}/api/{}/{}", self.base_url, self.api_version, path)
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
    async fn build_request(&self, builder: RequestBuilder) -> RequestBuilder {
        if let Some(token) = self.token_store.read().await.as_ref() {
            builder.header("Authorization", format!("Bearer {}", token.access_token))
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
    async fn execute_request<T: DeserializeOwned>(&self, request: RequestBuilder) -> Result<T> {
        // Clone the request for potential retry
        let request_clone = request.try_clone();
        let response = request.send().await?;

        match response.status() {
            StatusCode::OK => {
                let api_response: ApiResponse<T> = response.json().await?;
                match api_response.data {
                    Some(data) => Ok(data),
                    None => Err(anyhow::anyhow!("Empty response from server")),
                }
            }
            StatusCode::UNAUTHORIZED => {
                // Try to refresh token if we have a callback
                if let Some(request_retry) = request_clone {
                    if let Some(ref callback) = *self.refresh_callback.lock().await {
                        info!("[ApiClient] Token expired, attempting refresh");
                        match callback().await {
                            Ok(new_token) => {
                                info!("[ApiClient] Token refreshed successfully, retrying request");
                                self.set_token(Some(new_token.clone())).await;
                                
                                // Rebuild request with new token and execute without retry
                                let retry_request = self.build_request(request_retry).await;
                                return self.execute_request_without_retry(retry_request).await;
                            }
                            Err(e) => {
                                warn!("[ApiClient] Token refresh failed: {}", e);
                                // Fall through to clear token and return error
                            }
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
    async fn execute_request_without_retry<T: DeserializeOwned>(&self, request: RequestBuilder) -> Result<T> {
        let response = request.send().await?;

        match response.status() {
            StatusCode::OK => {
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

    /// Execute a request for setup status (handles different response format)
    async fn execute_setup_request(&self, request: RequestBuilder) -> Result<SetupStatus> {
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
        let url = format!("{}/api/setup/status", self.base_url);
        let request = self.client.get(&url);
        // Don't use auth for setup status check
        let status = self.execute_setup_request(request).await?;
        Ok(status.needs_setup)
    }

    /// POST request with authentication
    pub async fn post<T: Serialize, R: DeserializeOwned>(&self, path: &str, body: &T) -> Result<R> {
        let url = if path.starts_with("/api/") {
            // For paths that already include /api/, use versioned URL
            self.build_url(path.strip_prefix("/api/").unwrap())
        } else {
            // For other paths, use legacy URL
            format!("{}{}", self.base_url, path)
        };
        
        let request = self.client.post(&url).json(body);
        let request = self.build_request(request).await;
        self.execute_request(request).await
    }
    
    /// POST request for endpoints that return 204 No Content
    pub async fn post_no_content<T: Serialize>(&self, path: &str, body: &T) -> Result<()> {
        let url = if path.starts_with("/api/") {
            // For paths that already include /api/, use versioned URL
            self.build_url(path.strip_prefix("/api/").unwrap())
        } else {
            // For other paths, use legacy URL
            format!("{}{}", self.base_url, path)
        };
        
        let request = self.client.post(&url).json(body);
        let request = self.build_request(request).await;
        
        // Execute request with special handling for 204 No Content
        let request_clone = request.try_clone();
        let response = request.send().await?;
        
        match response.status() {
            StatusCode::OK | StatusCode::NO_CONTENT => Ok(()),
            StatusCode::UNAUTHORIZED => {
                // Try to refresh token if we have a callback
                if let Some(request_retry) = request_clone {
                    if let Some(ref callback) = *self.refresh_callback.lock().await {
                        info!("[ApiClient] Token expired, attempting refresh");
                        match callback().await {
                            Ok(new_token) => {
                                info!("[ApiClient] Token refreshed successfully, retrying request");
                                self.set_token(Some(new_token.clone())).await;
                                
                                // Rebuild request with new token and retry
                                let retry_request = self.build_request(request_retry).await;
                                let retry_response = retry_request.send().await?;
                                
                                match retry_response.status() {
                                    StatusCode::OK | StatusCode::NO_CONTENT => return Ok(()),
                                    _ => {
                                        let error_text = retry_response
                                            .text()
                                            .await
                                            .unwrap_or_else(|_| "Unknown error".to_string());
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

    /// GET request with authentication
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = if path.starts_with("/api/") {
            // For paths that already include /api/, use versioned URL
            self.build_url(path.strip_prefix("/api/").unwrap())
        } else {
            // For other paths, use legacy URL
            format!("{}{}", self.base_url, path)
        };
        
        // Debug logging
        log::debug!("[ApiClient] GET request to: {}", url);
        log::debug!("[ApiClient] Base URL: {}", self.base_url);
        
        let request = self.client.get(&url);
        let request = self.build_request(request).await;
        self.execute_request(request).await
    }

    /// GET request for public endpoints (no authentication)
    pub async fn get_public<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = if path.starts_with("/api/") {
            // For paths that already include /api/, use versioned URL
            self.build_url(path.strip_prefix("/api/").unwrap())
        } else {
            // For other paths, use legacy URL
            format!("{}{}", self.base_url, path)
        };
        
        log::debug!("[ApiClient] GET (public) request to: {}", url);
        
        let request = self.client.get(&url);
        let request = self.build_public_request(request);
        self.execute_request(request).await
    }

    /// PUT request
    pub async fn put<T: Serialize, R: DeserializeOwned>(&self, path: &str, body: &T) -> Result<R> {
        let url = if path.starts_with("/api/") {
            // For paths that already include /api/, use versioned URL
            self.build_url(path.strip_prefix("/api/").unwrap())
        } else {
            // For other paths, use legacy URL
            format!("{}{}", self.base_url, path)
        };
        
        let request = self.client.put(&url).json(body);
        let request = self.build_request(request).await;
        self.execute_request(request).await
    }

    /// DELETE request
    pub async fn delete<R: DeserializeOwned>(&self, path: &str) -> Result<R> {
        let url = if path.starts_with("/api/") {
            // For paths that already include /api/, use versioned URL
            self.build_url(path.strip_prefix("/api/").unwrap())
        } else {
            // For other paths, use legacy URL
            format!("{}{}", self.base_url, path)
        };
        
        let request = self.client.delete(&url);
        let request = self.build_request(request).await;
        self.execute_request(request).await
    }
}

impl ApiClient {
    /// Get watch state for the current user
    pub async fn get_watch_state(&self) -> Result<UserWatchState> {
        self.get("/api/watch/state").await
    }

    /// Update watch progress for a media item
    pub async fn update_progress(&self, request: &UpdateProgressRequest) -> Result<()> {
        // This endpoint returns 204 No Content, so we need special handling
        self.post_no_content("/api/watch/progress", request).await
    }

    /// Create initial admin user during setup
    pub async fn create_initial_admin(
        &self,
        username: String,
        password: String,
        display_name: Option<String>,
        setup_token: Option<String>,
    ) -> Result<AuthToken> {
        #[derive(Serialize)]
        struct AdminSetupRequest {
            username: String,
            password: String,
            display_name: Option<String>,
            setup_token: Option<String>,
        }

        let request = AdminSetupRequest {
            username,
            password,
            display_name,
            setup_token,
        };

        self.post("/api/setup/admin", &request).await
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
    pub async fn list_user_devices(&self) -> Result<Vec<ferrex_core::auth::device::AuthenticatedDevice>> {
        self.get("/api/users/me/devices").await
    }

    /// Revoke a device
    pub async fn revoke_device(&self, device_id: uuid::Uuid) -> Result<()> {
        let path = format!("/api/devices/{}/revoke", device_id);
        let _: serde_json::Value = self.delete(&path).await?;
        Ok(())
    }
    
    /// Execute a media query
    pub async fn query_media(&self, query: ferrex_core::query::MediaQuery) -> Result<Vec<ferrex_core::query::MediaReferenceWithStatus>> {
        // Server endpoint is at /media/query, not /api/v1/media/query
        self.post("/media/query", &query).await
    }
}

/// Server setup status
#[derive(Debug, Clone, serde::Deserialize)]
pub struct SetupStatus {
    pub needs_setup: bool,
    pub has_admin: bool,
    pub user_count: usize,
    pub library_count: usize,
}
