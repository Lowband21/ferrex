use anyhow::Result;
use ferrex_core::api_types::{ApiResponse, MediaId};
use ferrex_core::user::AuthToken;
use ferrex_core::watch_status::{UpdateProgressRequest, UserWatchState};
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;
use std::sync::Arc;
use tokio::sync::RwLock;

/// API client with authentication support
#[derive(Clone, Debug)]
pub struct ApiClient {
    client: Client,
    base_url: String,
    api_version: String,
    token_store: Arc<RwLock<Option<AuthToken>>>,
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

        log::info!("[ApiClient] Creating new API client with base URL: {}", base_url);
        
        Self {
            client,
            base_url,
            api_version: "v1".to_string(),
            token_store: Arc::new(RwLock::new(None)),
        }
    }

    /// Build a versioned API URL
    pub fn build_url(&self, path: &str) -> String {
        let path = path.trim_start_matches('/');
        format!("{}/api/{}/{}", self.base_url, self.api_version, path)
    }

    /// Set the authentication token
    pub async fn set_token(&self, token: Option<AuthToken>) {
        *self.token_store.write().await = token;
    }

    /// Get the current authentication token
    pub async fn get_token(&self) -> Option<AuthToken> {
        self.token_store.read().await.clone()
    }

    /// Build a request with authentication headers
    async fn build_request(&self, builder: RequestBuilder) -> RequestBuilder {
        if let Some(token) = self.token_store.read().await.as_ref() {
            builder.header("Authorization", format!("Bearer {}", token.access_token))
        } else {
            builder
        }
    }

    /// Execute a request and handle common errors
    async fn execute_request<T: DeserializeOwned>(&self, request: RequestBuilder) -> Result<T> {
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
                // Token might be expired, clear it
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

    /// Execute a request that returns raw response (for non-JSON endpoints)
    async fn execute_raw(&self, request: RequestBuilder) -> Result<Response> {
        let response = request.send().await?;

        match response.status() {
            StatusCode::OK => Ok(response),
            StatusCode::UNAUTHORIZED => {
                // Token might be expired, clear it
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

    /// Execute a request that returns no content (204)
    async fn execute_no_content(&self, request: RequestBuilder) -> Result<()> {
        let response = request.send().await?;

        match response.status() {
            StatusCode::OK | StatusCode::NO_CONTENT => Ok(()),
            StatusCode::UNAUTHORIZED => {
                // Token might be expired, clear it
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

    // Public API methods

    /// GET request
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

    /// POST request
    pub async fn post<B: serde::Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let url = if path.starts_with("/api/") {
            self.build_url(path.strip_prefix("/api/").unwrap())
        } else {
            format!("{}{}", self.base_url, path)
        };
        let request = self.client.post(&url).json(body);
        let request = self.build_request(request).await;
        self.execute_request(request).await
    }

    /// PUT request
    pub async fn put<B: serde::Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let url = if path.starts_with("/api/") {
            self.build_url(path.strip_prefix("/api/").unwrap())
        } else {
            format!("{}{}", self.base_url, path)
        };
        let request = self.client.put(&url).json(body);
        let request = self.build_request(request).await;
        self.execute_request(request).await
    }

    /// DELETE request
    pub async fn delete<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = if path.starts_with("/api/") {
            self.build_url(path.strip_prefix("/api/").unwrap())
        } else {
            format!("{}{}", self.base_url, path)
        };
        let request = self.client.delete(&url);
        let request = self.build_request(request).await;
        self.execute_request(request).await
    }

    /// GET request for raw bytes (e.g., images)
    pub async fn get_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let url = if path.starts_with("/api/") {
            self.build_url(path.strip_prefix("/api/").unwrap())
        } else {
            format!("{}{}", self.base_url, path)
        };
        let request = self.client.get(&url);
        let request = self.build_request(request).await;
        let response = self.execute_raw(request).await?;
        Ok(response.bytes().await?.to_vec())
    }

    /// Build a full URL for a given path (legacy - non-versioned)
    pub fn build_legacy_url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// Get authorization header value if authenticated
    pub async fn get_auth_header(&self) -> Option<String> {
        self.token_store
            .read()
            .await
            .as_ref()
            .map(|token| format!("Bearer {}", token.access_token))
    }

    // Watch status methods

    /// Get the user's complete watch state
    pub async fn get_watch_state(&self) -> Result<UserWatchState> {
        self.get("/api/watch/state").await
    }

    /// Update watch progress for a media item
    pub async fn update_watch_progress(&self, request: &UpdateProgressRequest) -> Result<()> {
        let url = self.build_url("watch/progress");
        let request = self.client.post(&url).json(request);
        let request = self.build_request(request).await;
        self.execute_no_content(request).await
    }

    /// Get watch progress for a specific media item
    pub async fn get_media_progress(&self, media_id: &MediaId) -> Result<Option<f32>> {
        let serialized_id = serde_json::to_string(media_id)?;
        let encoded_id = urlencoding::encode(&serialized_id);
        let path = format!("/api/media/{}/progress", encoded_id);

        #[derive(serde::Deserialize)]
        struct ProgressResponse {
            progress: Option<f32>,
        }

        let response: ProgressResponse = self.get(&path).await?;
        Ok(response.progress)
    }

    /// Mark a media item as watched
    pub async fn mark_as_watched(&self, media_id: &MediaId) -> Result<()> {
        let serialized_id = serde_json::to_string(media_id)?;
        let encoded_id = urlencoding::encode(&serialized_id);
        let url = format!("{}/api/media/{}/complete", self.base_url, encoded_id);
        let request = self.client.post(&url).json(&serde_json::json!({}));
        let request = self.build_request(request).await;
        self.execute_no_content(request).await
    }

    /// Remove watch status for a media item
    pub async fn remove_watch_status(&self, media_id: &MediaId) -> Result<()> {
        let serialized_id = serde_json::to_string(media_id)?;
        let encoded_id = urlencoding::encode(&serialized_id);
        let url = format!("{}/api/watch/progress/{}", self.base_url, encoded_id);
        let request = self.client.delete(&url);
        let request = self.build_request(request).await;
        self.execute_no_content(request).await
    }

    /// Change user password
    pub async fn change_password(&self, current_password: String, new_password: String) -> Result<()> {
        #[derive(serde::Serialize)]
        struct ChangePasswordRequest {
            current_password: String,
            new_password: String,
        }

        let request = ChangePasswordRequest {
            current_password,
            new_password,
        };

        let url = self.build_url("user/password");
        let request = self.client.put(&url).json(&request);
        let request = self.build_request(request).await;
        self.execute_no_content(request).await
    }

    // Device management methods

    /// List user's authenticated devices
    pub async fn list_user_devices(&self) -> Result<Vec<ferrex_core::auth::AuthenticatedDevice>> {
        self.get("/api/auth/device/list").await
    }

    /// Revoke a device
    pub async fn revoke_device(&self, device_id: uuid::Uuid) -> Result<()> {
        #[derive(serde::Serialize)]
        struct RevokeDeviceRequest {
            device_id: uuid::Uuid,
        }

        let request = RevokeDeviceRequest { device_id };
        let url = self.build_url("auth/device/revoke");
        let request = self.client.post(&url).json(&request);
        let request = self.build_request(request).await;
        self.execute_no_content(request).await
    }

    /// Check server setup status
    pub async fn check_setup_status(&self) -> Result<SetupStatus> {
        self.get("/api/setup/status").await
    }

    /// Create initial admin user during first-run setup
    pub async fn create_initial_admin(&self, username: String, password: String, display_name: Option<String>, setup_token: Option<String>) -> Result<AuthToken> {
        #[derive(serde::Serialize)]
        struct CreateAdminRequest {
            username: String,
            password: String,
            display_name: Option<String>,
            setup_token: Option<String>,
        }

        let request = CreateAdminRequest {
            username,
            password,
            display_name,
            setup_token,
        };

        self.post("/api/setup/admin", &request).await
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
