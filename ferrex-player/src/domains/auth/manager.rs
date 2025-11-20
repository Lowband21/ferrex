use crate::domains::auth::dto::UserListItemDto;
use crate::domains::auth::errors::{
    AuthError, AuthResult, DeviceError, NetworkError, StorageError, TokenError,
};
use crate::domains::auth::state_types::{AuthState, AuthStateStore};
use chrono::{DateTime, Utc};
use directories::ProjectDirs;
use ferrex_core::api_types::ApiResponse;
use ferrex_core::auth::{AuthResult as ServerAuthResult, DeviceInfo, Platform};
use ferrex_core::rbac::UserPermissions;
use ferrex_core::user::{AuthToken, LoginRequest, RegisterRequest, User};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use log::{debug, error, info, warn};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::OnceCell;
use uuid::Uuid;

use crate::domains::auth::hardware_fingerprint::generate_hardware_fingerprint;
use crate::domains::auth::storage::{AuthStorage, StoredAuth};
use crate::infrastructure::api_client::ApiClient;

const KEYCHAIN_SERVICE: &str = "ferrex-media-player";
const KEYCHAIN_ACCOUNT: &str = "auth-token";

/// JWT Token expiry buffer - refresh tokens 1 minute before they expire
/// This provides a reasonable buffer to prevent race conditions without being too aggressive
const TOKEN_EXPIRY_BUFFER_SECONDS: i64 = 60;

#[derive(Debug, Serialize)]
struct RefreshTokenRequest {
    refresh_token: String,
}

#[derive(Debug, Deserialize)]
struct JwtClaims {
    /// Token expiration time (Unix timestamp)
    exp: Option<i64>,
    /// Token issued at time (Unix timestamp)
    iat: Option<i64>,
    /// Subject (typically user ID)
    sub: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAuthStatus {
    pub device_registered: bool,
    pub has_pin: bool,
    pub remaining_attempts: Option<u8>,
}

#[derive(Debug, Clone)]
pub struct PlayerAuthResult {
    pub user: User,
    pub permissions: UserPermissions,
    pub device_has_pin: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceIdentity {
    pub id: Uuid,
    pub fingerprint: String,
    pub created_at: DateTime<Utc>,
    pub name: String,
}

impl DeviceIdentity {
    pub async fn load() -> AuthResult<Option<Self>> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(None);
        }

        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| AuthError::Storage(StorageError::ReadFailed(e)))?;
        let identity: DeviceIdentity = serde_json::from_str(&content)
            .map_err(|_| AuthError::Storage(StorageError::CorruptedData))?;
        Ok(Some(identity))
    }

    pub async fn save(&self) -> AuthResult<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let content = serde_json::to_string_pretty(self).map_err(|e| {
            AuthError::Internal(format!("Failed to serialize device identity: {}", e))
        })?;
        tokio::fs::write(&path, content)
            .await
            .map_err(|e| AuthError::Storage(StorageError::WriteFailed(e)))?;
        Ok(())
    }

    fn config_path() -> AuthResult<PathBuf> {
        let proj_dirs = ProjectDirs::from("", "ferrex", "media-player").ok_or_else(|| {
            AuthError::Storage(StorageError::InitFailed(
                "Unable to determine config directory".to_string(),
            ))
        })?;
        Ok(proj_dirs.config_dir().join("device.json"))
    }
}

#[derive(Debug, Serialize)]
pub struct DeviceLoginRequest {
    pub username: String,
    pub password: String,
    pub device_info: Option<DeviceInfo>,
    pub remember_device: bool,
}

#[derive(Debug, Serialize)]
pub struct PinLoginRequest {
    pub user_id: Uuid,
    pub device_id: Uuid,
    pub pin: String,
}

#[derive(Debug, Serialize)]
pub struct SetPinRequest {
    pub device_id: Uuid,
    pub pin: String,
}

/// Authentication state manager
///
/// ## Token Persistence Behavior
///
/// The AuthManager handles authentication token persistence across app restarts:
///
/// ### Token Storage
/// - Tokens are encrypted and stored locally using device-specific hardware fingerprints
/// - Only tokens from the same device can decrypt and use stored authentication
/// - Storage location: Platform-specific app data directory
///
/// ### Token Expiry Handling
/// - JWT tokens typically have 1-hour expiry times from the server
/// - A 60-second buffer (TOKEN_EXPIRY_BUFFER_SECONDS) is applied when loading tokens
/// - Tokens with less than 60 seconds remaining are considered expired and rejected
/// - This prevents race conditions where a token expires immediately after loading
///
/// ### App Restart Behavior
/// - On app start, `load_from_keychain()` attempts to restore previous authentication
/// - If a valid token is found (>60 seconds remaining), the user is auto-authenticated
/// - If token is expired or near expiry, the user must re-authenticate
///
/// ### Device Trust (Future Enhancement)
/// - Currently: Token persistence is based on JWT expiry (1 hour)
/// - Planned: Device trust for 30-day persistence independent of JWT expiry
/// - This would allow users to stay logged in for extended periods on trusted devices
///
/// ### Security Considerations
/// - Hardware fingerprint binding prevents token theft across devices
/// - Encrypted storage protects tokens at rest
/// - Short expiry buffer ensures tokens are refreshed before actual expiry
#[derive(Clone, Debug)]
pub struct AuthManager {
    api_client: ApiClient,
    auth_state: AuthStateStore,
    device_id: OnceCell<Uuid>,
    device_fingerprint: OnceCell<String>,
    auth_storage: Arc<AuthStorage>,
}

impl AuthManager {
    pub fn new(api_client: ApiClient) -> Self {
        let auth_storage = match AuthStorage::new() {
            Ok(storage) => Arc::new(storage),
            Err(e) => {
                warn!(
                    "Failed to create auth storage: {}. Auth persistence will be disabled.",
                    e
                );
                // TODO: Fix this panic
                panic!("Unable to create auth storage: {}", e);
            }
        };

        let manager = Self {
            api_client: api_client.clone(),
            auth_state: AuthStateStore::new(),
            device_id: OnceCell::new(),
            device_fingerprint: OnceCell::new(),
            auth_storage,
        };

        // Set up the refresh callback for automatic token refresh on 401
        let api_client_clone = api_client.clone();
        let auth_manager_for_callback = manager.clone();
        tokio::spawn(async move {
            api_client_clone
                .set_refresh_callback(move || {
                    let auth_manager = auth_manager_for_callback.clone();
                    async move {
                        auth_manager
                            .refresh_access_token_internal()
                            .await
                            .map_err(|e| anyhow::anyhow!("Token refresh failed: {}", e))
                    }
                })
                .await;
        });

        manager
    }

    pub fn auth_storage(&self) -> &Arc<AuthStorage> {
        &self.auth_storage
    }

    pub async fn load_from_keychain(&self) -> AuthResult<Option<StoredAuth>> {
        let hardware_fingerprint = generate_hardware_fingerprint().await.map_err(|e| {
            AuthError::Storage(StorageError::InitFailed(format!(
                "Failed to get hardware fingerprint: {}",
                e
            )))
        })?;

        match self.auth_storage.load_auth(&hardware_fingerprint).await {
            Ok(Some(stored_auth)) => {
                info!(
                    "Loaded authentication for user: {}",
                    stored_auth.user.username
                );

                info!(
                    "Loaded token (first 50 chars): {}...",
                    &stored_auth
                        .token
                        .access_token
                        .chars()
                        .take(50)
                        .collect::<String>()
                );
                info!(
                    "Token stored at: {}, expires_in: {} seconds",
                    stored_auth.stored_at, stored_auth.token.expires_in
                );

                // First check device trust expiry (30-day persistence)
                if let Some(device_trust_expires) = stored_auth.device_trust_expires_at {
                    let now = Utc::now();
                    if now > device_trust_expires {
                        warn!(
                            "Device trust expired (was valid until {})",
                            device_trust_expires
                        );
                        self.clear_keychain().await?;
                        return Ok(None);
                    }
                    info!("Device trust still valid until {}", device_trust_expires);
                }

                // Check if this is a non-JWT token and calculate actual expiry
                let parts: Vec<&str> = stored_auth.token.access_token.split('.').collect();
                if parts.len() != 3 {
                    // For non-JWT tokens, check if it's expired based on stored_at + expires_in
                    let elapsed = Utc::now().signed_duration_since(stored_auth.stored_at);
                    let elapsed_seconds = elapsed.num_seconds();
                    let remaining_seconds = stored_auth.token.expires_in as i64 - elapsed_seconds;

                    info!(
                        "Non-JWT token: elapsed {} seconds, remaining {} seconds",
                        elapsed_seconds, remaining_seconds
                    );

                    if remaining_seconds <= TOKEN_EXPIRY_BUFFER_SECONDS {
                        // Token expired but device trust still valid - we can refresh
                        if stored_auth.device_trust_expires_at.is_some()
                            && !stored_auth.token.refresh_token.is_empty()
                        {
                            info!("Token expired but device trust valid - attempting refresh");

                            // Set up auth state for refresh
                            let permissions =
                                stored_auth.permissions.clone().unwrap_or_else(|| {
                                    UserPermissions {
                                        user_id: stored_auth.user.id,
                                        roles: Vec::new(),
                                        permissions: std::collections::HashMap::new(),
                                        permission_details: None,
                                    }
                                });
                            self.auth_state.authenticate(
                                stored_auth.user.clone(),
                                stored_auth.token.clone(),
                                permissions,
                                stored_auth.server_url.clone(),
                            );

                            // Set token for API client
                            self.api_client
                                .set_token(Some(stored_auth.token.clone()))
                                .await;

                            // Try to refresh
                            match self.refresh_access_token().await {
                                Ok(()) => {
                                    // Reload the refreshed auth
                                    if let Ok(Some(refreshed)) =
                                        self.auth_storage.load_auth(&hardware_fingerprint).await
                                    {
                                        info!("[AuthManager] Successfully refreshed expired token");
                                        return Ok(Some(refreshed));
                                    }
                                }
                                Err(e) => {
                                    warn!("[AuthManager] Failed to refresh token: {}", e);
                                    // Fall through to clear auth
                                }
                            }
                        } else {
                            warn!(
                                "Non-JWT token expired: {} seconds remaining (buffer: {} seconds)",
                                remaining_seconds, TOKEN_EXPIRY_BUFFER_SECONDS
                            );
                            self.clear_keychain().await?;
                            return Ok(None);
                        }
                    }

                    info!(
                        "Non-JWT token still valid with {} seconds remaining",
                        remaining_seconds
                    );
                } else {
                    // For JWT tokens, use the standard expiry check
                    if is_token_expired(&stored_auth.token) {
                        // Token expired but device trust still valid - we can refresh
                        if stored_auth.device_trust_expires_at.is_some()
                            && !stored_auth.token.refresh_token.is_empty()
                        {
                            info!("JWT expired but device trust valid - attempting refresh");

                            // Set up auth state for refresh
                            let permissions =
                                stored_auth.permissions.clone().unwrap_or_else(|| {
                                    UserPermissions {
                                        user_id: stored_auth.user.id,
                                        roles: Vec::new(),
                                        permissions: std::collections::HashMap::new(),
                                        permission_details: None,
                                    }
                                });
                            self.auth_state.authenticate(
                                stored_auth.user.clone(),
                                stored_auth.token.clone(),
                                permissions,
                                stored_auth.server_url.clone(),
                            );

                            // Set token for API client
                            self.api_client
                                .set_token(Some(stored_auth.token.clone()))
                                .await;

                            // Try to refresh
                            match self.refresh_access_token().await {
                                Ok(()) => {
                                    // Reload the refreshed auth
                                    if let Ok(Some(refreshed)) =
                                        self.auth_storage.load_auth(&hardware_fingerprint).await
                                    {
                                        info!("[AuthManager] Successfully refreshed expired JWT");
                                        return Ok(Some(refreshed));
                                    }
                                }
                                Err(e) => {
                                    warn!("[AuthManager] Failed to refresh JWT: {}", e);
                                    // Fall through to clear auth
                                }
                            }
                        } else {
                            warn!("JWT token is expired");
                            self.clear_keychain().await?;
                            return Ok(None);
                        }
                    }
                }

                // Return the stored auth data without applying it
                Ok(Some(stored_auth))
            }
            Ok(None) => {
                info!("No stored authentication found");
                Ok(None)
            }
            Err(e) => {
                error!("Failed to load auth from storage: {}", e);

                // If decryption failed, it's likely due to hardware fingerprint change
                // Clear the old cache to allow fresh login
                if e.to_string().contains("Decryption failed") {
                    info!("Clearing invalid auth cache due to decryption failure");
                    if let Err(clear_err) = self.auth_storage.clear_auth().await {
                        error!("Failed to clear auth cache: {}", clear_err);
                    }
                }

                Ok(None)
            }
        }
    }

    /// Apply stored authentication (actually authenticate the user)
    pub async fn apply_stored_auth(&self, stored_auth: StoredAuth) -> AuthResult<()> {
        info!(
            "Applying stored authentication for user: {}",
            stored_auth.user.username
        );

        // Set token in API client
        self.api_client
            .set_token(Some(stored_auth.token.clone()))
            .await;

        match self.fetch_user_and_permissions().await {
            Ok((user, permissions)) => {
                self.auth_state.authenticate(
                    user.clone(),
                    stored_auth.token.clone(),
                    permissions.clone(),
                    stored_auth.server_url.clone(),
                );

                // Persist refreshed auth snapshot for future startups
                if let Err(err) = self.save_current_auth().await {
                    warn!("Failed to persist refreshed auth: {}", err);
                }

                Ok(())
            }
            Err(err) => {
                self.api_client.set_token(None).await;
                self.auth_state.logout();

                if matches!(&err, AuthError::Network(NetworkError::InvalidCredentials)) {
                    if let Err(clear_err) = self.clear_keychain().await {
                        warn!("Failed to clear invalid auth cache: {}", clear_err);
                    }
                }

                Err(err)
            }
        }
    }

    /// Validate that the currently configured session is still authorized
    pub async fn validate_session(&self) -> AuthResult<(User, UserPermissions)> {
        let (token, server_url) = self
            .auth_state
            .with_state(|state| match state {
                AuthState::Authenticated {
                    token, server_url, ..
                } => Some((token.clone(), server_url.clone())),
                _ => None,
            })
            .ok_or(AuthError::Token(TokenError::NotAuthenticated))?;

        self.api_client.set_token(Some(token.clone())).await;

        match self.fetch_user_and_permissions().await {
            Ok((user, permissions)) => {
                self.auth_state
                    .authenticate(user.clone(), token, permissions.clone(), server_url);

                if let Err(err) = self.save_current_auth().await {
                    warn!("Failed to persist refreshed auth: {}", err);
                }

                Ok((user, permissions))
            }
            Err(err) => {
                self.api_client.set_token(None).await;
                self.auth_state.logout();

                if matches!(&err, AuthError::Network(NetworkError::InvalidCredentials)) {
                    if let Err(clear_err) = self.clear_keychain().await {
                        warn!("Failed to clear invalid auth cache: {}", clear_err);
                    }
                }

                Err(err)
            }
        }
    }

    async fn fetch_user_and_permissions(&self) -> AuthResult<(User, UserPermissions)> {
        let user: User = self.fetch_api_data("/users/me").await?;
        let permissions: UserPermissions = self.fetch_api_data("/users/me/permissions").await?;
        Ok((user, permissions))
    }

    async fn fetch_api_data<T>(&self, path: &str) -> AuthResult<T>
    where
        T: DeserializeOwned,
    {
        let url = self.api_client.build_url(path, false);
        let request = self.api_client.client.get(&url);
        let request = self.api_client.build_request(request).await;
        let response = request
            .send()
            .await
            .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?;

        match response.status() {
            StatusCode::OK => {
                let api_response: ApiResponse<T> = response.json().await.map_err(|e| {
                    AuthError::Network(NetworkError::InvalidResponse(e.to_string()))
                })?;

                api_response.data.ok_or_else(|| {
                    AuthError::Network(NetworkError::InvalidResponse(format!(
                        "No data returned for {}",
                        path
                    )))
                })
            }
            StatusCode::UNAUTHORIZED => Err(AuthError::Network(NetworkError::InvalidCredentials)),
            status => {
                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "<unavailable>".to_string());
                Err(AuthError::Network(NetworkError::RequestFailed(format!(
                    "{} {}",
                    status, body
                ))))
            }
        }
    }

    /// Save authentication to encrypted storage
    async fn save_to_keychain(&self, auth: &StoredAuth) -> AuthResult<()> {
        let hardware_fingerprint = generate_hardware_fingerprint().await.map_err(|e| {
            AuthError::Storage(StorageError::InitFailed(format!(
                "Failed to get hardware fingerprint: {}",
                e
            )))
        })?;

        // Log what we're saving for debugging
        info!(
            "Saving token (first 50 chars): {}...",
            &auth.token.access_token.chars().take(50).collect::<String>()
        );
        info!("Token expires_in: {} seconds", auth.token.expires_in);

        self.auth_storage
            .save_auth(auth, &hardware_fingerprint)
            .await
            .map_err(|e| {
                AuthError::Storage(StorageError::WriteFailed(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to save auth: {}", e),
                )))
            })?;

        info!("Saved authentication to encrypted storage");
        Ok(())
    }

    /// Refresh the access token using the refresh token (public API)
    pub async fn refresh_access_token(&self) -> AuthResult<()> {
        self.refresh_access_token_internal().await.map(|_| ())
    }

    /// Internal refresh method that returns the new token for API client callback
    async fn refresh_access_token_internal(&self) -> AuthResult<AuthToken> {
        // Get current refresh token
        let refresh_token = self.auth_state.with_state(|state| match state {
            AuthState::Authenticated { token, .. } => Some(token.refresh_token.clone()),
            _ => None,
        });

        let refresh_token =
            refresh_token.ok_or_else(|| AuthError::Token(TokenError::NotAuthenticated))?;

        if refresh_token.is_empty() {
            return Err(AuthError::Token(TokenError::RefreshTokenMissing));
        }

        info!("[AuthManager] Attempting to refresh access token");

        // Temporarily disable the refresh callback to avoid infinite recursion
        let response: AuthToken = {
            // Create a new client without callback for this request
            let temp_client = ApiClient::new(self.api_client.base_url().to_string());
            temp_client
                .set_token(Some(AuthToken {
                    access_token: String::new(),
                    refresh_token: refresh_token.clone(),
                    expires_in: 0,
                }))
                .await;

            temp_client
                .post("/auth/refresh", &RefreshTokenRequest { refresh_token })
                .await
                .map_err(|e| {
                    warn!("[AuthManager] Token refresh failed: {}", e);
                    AuthError::Network(NetworkError::RequestFailed(e.to_string()))
                })?
        };

        // Get current state details
        let (user, permissions, server_url) = self
            .auth_state
            .with_state(|state| match state {
                AuthState::Authenticated {
                    user,
                    permissions,
                    server_url,
                    ..
                } => Some((user.clone(), permissions.clone(), server_url.clone())),
                _ => None,
            })
            .ok_or_else(|| AuthError::Token(TokenError::NotAuthenticated))?;

        // Update auth state with new token
        self.auth_state.authenticate(
            user.clone(),
            response.clone(),
            permissions.clone(),
            server_url.clone(),
        );

        // Update API client token
        self.api_client.set_token(Some(response.clone())).await;

        // Save to storage
        self.save_current_auth().await?;

        info!("[AuthManager] Successfully refreshed access token");
        Ok(response)
    }

    /// Save current auth state to encrypted storage
    pub async fn save_current_auth(&self) -> AuthResult<()> {
        // Get current state from AuthStateStore
        let stored_auth = self.auth_state.with_state(|state| match state {
            AuthState::Authenticated {
                user,
                token,
                permissions,
                server_url,
            } => {
                info!(
                    "Saving auth with token expiring in {} seconds",
                    token.expires_in
                );
                let now = Utc::now();
                Some(StoredAuth {
                    token: token.clone(),
                    user: user.clone(),
                    server_url: server_url.clone(),
                    permissions: Some(permissions.clone()),
                    stored_at: now,
                    // Set device trust to expire in 30 days
                    device_trust_expires_at: Some(now + chrono::Duration::days(30)),
                    refresh_token: Some(token.refresh_token.clone()),
                })
            }
            _ => None,
        });

        match stored_auth {
            Some(auth) => self.save_to_keychain(&auth).await,
            None => Err(AuthError::NotAuthenticated),
        }
    }

    /// Clear stored authentication from encrypted storage
    pub async fn clear_keychain(&self) -> AuthResult<()> {
        self.auth_storage.clear_auth().await.map_err(|e| {
            AuthError::Storage(StorageError::WriteFailed(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to clear auth: {}", e),
            )))
        })?;

        info!("Cleared authentication from storage");
        Ok(())
    }

    /// Login with username and PIN
    pub async fn login(
        &self,
        username: String,
        pin: String,
        server_url: String,
    ) -> AuthResult<(User, UserPermissions)> {
        let request = LoginRequest {
            username,
            password: pin, // Using PIN as password
            device_name: Some("Ferrex Media Player".to_string()),
        };

        // Call login endpoint
        let token: AuthToken = self
            .api_client
            .post("/auth/login", &request)
            .await
            .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?;

        info!(
            "Login received token with expires_in: {} seconds ({} minutes)",
            token.expires_in,
            token.expires_in / 60
        );

        // Set token in API client
        self.api_client.set_token(Some(token.clone())).await;

        // Get user profile
        let user: User = self
            .api_client
            .get("/users/me")
            .await
            .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?;

        // Get user permissions
        let permissions: UserPermissions = self
            .api_client
            .get("/users/me/permissions")
            .await
            .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?;

        // Update auth state using AuthStateStore
        self.auth_state.authenticate(
            user.clone(),
            token.clone(),
            permissions.clone(),
            server_url.clone(),
        );

        // Save to keychain
        if let Err(e) = self.save_current_auth().await {
            warn!("Failed to save to keychain: {}", e);
        }

        Ok((user, permissions))
    }

    /// Register a new user
    pub async fn register(
        &self,
        username: String,
        pin: String,
        display_name: String,
        server_url: String,
    ) -> AuthResult<(User, UserPermissions)> {
        let request = RegisterRequest {
            username,
            password: pin, // Using PIN as password
            display_name,
        };

        // Call register endpoint
        let token: AuthToken = self
            .api_client
            .post("/auth/register", &request)
            .await
            .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?;

        // Set token in API client
        self.api_client.set_token(Some(token.clone())).await;

        // Get user profile
        let user: User = self
            .api_client
            .get("/users/me")
            .await
            .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?;

        // Get user permissions
        let permissions: UserPermissions = self
            .api_client
            .get("/users/me/permissions")
            .await
            .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?;

        // Update auth state using AuthStateStore
        self.auth_state.authenticate(
            user.clone(),
            token.clone(),
            permissions.clone(),
            server_url.clone(),
        );

        // Save to keychain
        if let Err(e) = self.save_current_auth().await {
            warn!("Failed to save to keychain: {}", e);
        }

        Ok((user, permissions))
    }

    /// Logout current user
    pub async fn logout(&self) -> AuthResult<()> {
        // Fire and forget logout request with short timeout
        // We don't wait for the response since the token might already be invalid
        let api_client = self.api_client.clone();
        tokio::spawn(async move {
            #[derive(serde::Serialize)]
            struct EmptyRequest {}

            // Use a short timeout for logout
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                api_client
                    .post::<EmptyRequest, serde_json::Value>("/auth/logout", &EmptyRequest {}),
            )
            .await;
        });

        // Clear token from API client immediately
        self.api_client.set_token(None).await;

        // Clear auth state using AuthStateStore
        self.auth_state.logout();

        // Clear keychain
        self.clear_keychain().await?;

        Ok(())
    }

    /// Switch to a different user account without app restart
    /// This will log out the current user and prompt for authentication
    pub async fn switch_user(&self) -> AuthResult<()> {
        // Log out current user
        self.logout().await?;
        Ok(())
    }

    /// Set PIN for current device
    pub async fn set_device_pin(&self, pin: String) -> AuthResult<()> {
        let device_id = self.get_or_create_device_id().await?;

        let request = SetPinRequest { device_id, pin };

        self.api_client
            .post::<_, serde_json::Value>("/auth/device/pin/set", &request)
            .await
            .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?;

        Ok(())
    }

    /// Change PIN for current device
    pub async fn change_device_pin(&self, current_pin: String, new_pin: String) -> AuthResult<()> {
        let device_id = self.get_or_create_device_id().await?;

        #[derive(serde::Serialize)]
        struct ChangePinRequest {
            device_id: String,
            current_pin: String,
            new_pin: String,
        }

        let request = ChangePinRequest {
            device_id: device_id.to_string(),
            current_pin,
            new_pin,
        };

        self.api_client
            .post::<_, serde_json::Value>("/auth/device/pin/change", &request)
            .await
            .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?;

        Ok(())
    }

    /// Check if user has PIN on this device
    pub async fn check_device_auth(&self, user_id: Uuid) -> AuthResult<DeviceAuthStatus> {
        // Try offline check first
        if let Some(status) = self.check_cached_device_status(user_id).await {
            log::info!(
                "[Auth] Using cached device status for user {}: registered={}, has_pin={}",
                user_id,
                status.device_registered,
                status.has_pin
            );
            return Ok(status);
        }

        // Online check
        let device_id = self.get_or_create_device_id().await?;
        log::info!(
            "[Auth] Checking device status online for user {} on device {}",
            user_id,
            device_id
        );

        // Note: This endpoint doesn't require authentication - it's checking if a device can use PIN
        let status: DeviceAuthStatus = self
            .api_client
            .get(&format!(
                "/auth/device/status?user_id={}&device_id={}",
                user_id, device_id
            ))
            .await
            .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?;

        log::info!(
            "[Auth] Device status for user {}: registered={}, has_pin={}, attempts_remaining={:?}",
            user_id,
            status.device_registered,
            status.has_pin,
            status.remaining_attempts
        );

        // Cache the result
        self.cache_device_status(user_id, &status).await;
        Ok(status)
    }

    /// Handle authentication result
    async fn handle_auth_result(&self, result: ServerAuthResult) -> AuthResult<()> {
        // Extract actual expiry from the JWT token
        let expires_in = extract_token_expiry(&result.session_token)
            .and_then(|secs| {
                info!(
                    "JWT token expires in {} seconds ({} minutes)",
                    secs,
                    secs / 60
                );
                u32::try_from(secs).ok()
            })
            .unwrap_or_else(|| {
                warn!("Could not extract token expiry, using default 1 hour");
                3600
            });

        // Set token in API client
        let token = AuthToken {
            access_token: result.session_token.clone(),
            refresh_token: String::new(), // TODO: Get from server response
            expires_in,
        };
        self.api_client.set_token(Some(token.clone())).await;

        // Get user details
        let user: User = self
            .api_client
            .get("/users/me")
            .await
            .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?;

        // Get user permissions
        let permissions: UserPermissions = self
            .api_client
            .get("/users/me/permissions")
            .await
            .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?;

        // Get server URL
        let server_url = self.api_client.build_url("", false);

        // Update auth state using AuthStateStore
        self.auth_state.authenticate(
            user.clone(),
            token.clone(),
            permissions.clone(),
            server_url.clone(),
        );

        // Save to keychain
        if let Err(e) = self.save_current_auth().await {
            warn!("Failed to save to keychain: {}", e);
        }

        Ok(())
    }

    /// Check cached device status (stub for now)
    async fn check_cached_device_status(&self, _user_id: Uuid) -> Option<DeviceAuthStatus> {
        // TODO: Implement offline cache
        None
    }

    /// Cache device status (stub for now)
    async fn cache_device_status(&self, _user_id: Uuid, _status: &DeviceAuthStatus) {
        // TODO: Implement offline cache
    }

    /// Get or create device ID
    async fn get_or_create_device_id(&self) -> AuthResult<Uuid> {
        if let Some(id) = self.device_id.get() {
            return Ok(*id);
        }

        // Try to load existing device identity
        if let Some(identity) = DeviceIdentity::load().await? {
            let _ = self.device_id.set(identity.id);
            let _ = self.device_fingerprint.set(identity.fingerprint);
            return Ok(identity.id);
        }

        // Create new device identity
        let id = Uuid::new_v4();
        let fingerprint = generate_hardware_fingerprint().await.map_err(|e| {
            AuthError::Storage(StorageError::InitFailed(format!(
                "Failed to get hardware fingerprint: {}",
                e
            )))
        })?;
        let identity = DeviceIdentity {
            id,
            fingerprint: fingerprint.clone(),
            created_at: Utc::now(),
            name: get_device_name(),
        };

        identity.save().await?;
        let _ = self.device_id.set(id);
        let _ = self.device_fingerprint.set(fingerprint);

        Ok(id)
    }

    /// Get current authenticated user
    pub async fn get_current_user(&self) -> Option<User> {
        self.auth_state.with_state(|state| match state {
            AuthState::Authenticated { user, .. } => Some(user.clone()),
            _ => None,
        })
    }

    /// Get current user permissions
    pub async fn get_current_permissions(&self) -> Option<UserPermissions> {
        self.auth_state.with_state(|state| match state {
            AuthState::Authenticated { permissions, .. } => Some(permissions.clone()),
            _ => None,
        })
    }

    /// Check if auto-login is enabled for current user
    pub async fn is_auto_login_enabled(&self) -> bool {
        if let Some(user) = self.get_current_user().await {
            // Check both user preference and device-specific setting
            let device_auto_login = self
                .auth_storage
                .is_auto_login_enabled(&user.id)
                .await
                .unwrap_or(false);
            user.preferences.auto_login_enabled && device_auto_login
        } else {
            false
        }
    }

    /// Set auto-login preference for current user and device
    pub async fn set_auto_login(&self, enabled: bool) -> AuthResult<()> {
        let user = self
            .get_current_user()
            .await
            .ok_or(AuthError::NotAuthenticated)?;

        // Set device-specific auto-login
        self.auth_storage
            .set_auto_login(&user.id, enabled)
            .await
            .map_err(|e| {
                AuthError::Storage(StorageError::WriteFailed(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to set auto-login: {}", e),
                )))
            })?;

        Ok(())
    }

    /// Get all users (for user selection screen)
    ///
    /// This method sends the device fingerprint to get appropriate user information
    /// based on whether the device is known/trusted.
    pub async fn get_all_users(&self) -> AuthResult<Vec<UserListItemDto>> {
        // Generate device fingerprint
        let fingerprint =
            crate::domains::auth::hardware_fingerprint::generate_hardware_fingerprint()
                .await
                .map_err(|e| {
                    AuthError::Device(DeviceError::FingerprintGeneration(e.to_string()))
                })?;

        // Check if we have an active auth token
        let has_auth = self
            .auth_state
            .with_state(|state| matches!(state, AuthState::Authenticated { .. }));

        let users: Vec<UserListItemDto> = if has_auth {
            // Use authenticated endpoint for better information
            self.api_client
                .get("/v1/users/list")
                .await
                .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?
        } else {
            // TODO: Use ApiClient trait instance
            // Use public endpoint with device fingerprint
            // Build request with custom header
            let client = reqwest::Client::new();
            let url = format!("{}/api/v1/users/public", self.api_client.base_url());

            let response = client
                .get(&url)
                .header("X-Device-Fingerprint", fingerprint)
                .send()
                .await
                .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?;

            if !response.status().is_success() {
                let status = response.status();
                let error_text = response.text().await.unwrap_or_else(|_| status.to_string());
                return Err(AuthError::Network(NetworkError::RequestFailed(format!(
                    "Failed to get users: {}",
                    error_text
                ))));
            }

            response
                .json::<ferrex_core::api_types::ApiResponse<Vec<UserListItemDto>>>()
                .await
                .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?
                .data
                .ok_or_else(|| {
                    AuthError::Network(NetworkError::RequestFailed(
                        "No data in response".to_string(),
                    ))
                })?
        };

        Ok(users)
    }

    /// Check setup status
    pub async fn check_setup_status(&self) -> AuthResult<bool> {
        #[derive(Debug, Deserialize)]
        struct SetupStatus {
            needs_setup: bool,
            has_admin: bool,
            user_count: usize,
            library_count: usize,
        }

        let status: SetupStatus = self
            .api_client
            .get("/setup/status")
            .await
            .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?;
        Ok(status.needs_setup)
    }

    /// Authenticate with username/password and device info
    pub async fn authenticate_device(
        &self,
        username: String,
        password: String,
        remember_device: bool,
    ) -> AuthResult<PlayerAuthResult> {
        let (device_id, _) = self
            .get_or_create_device_id()
            .await
            .map(|id| (id, ()))
            .unwrap_or((Uuid::new_v4(), ()));

        let device_info = DeviceInfo {
            device_id,
            device_name: get_device_name(),
            platform: get_current_platform(),
            app_version: "1.0.0".to_string(),
            hardware_id: Some(generate_hardware_fingerprint().await.map_err(|e| {
                AuthError::Storage(StorageError::InitFailed(format!(
                    "Failed to get hardware fingerprint: {}",
                    e
                )))
            })?),
        };

        let request = DeviceLoginRequest {
            username,
            password,
            device_info: Some(device_info),
            remember_device,
        };

        let result: ServerAuthResult =
            self.api_client
                .post("/auth/device/login", &request)
                .await
                .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?;

        // Log the received token for debugging
        info!(
            "Device login received token (first 50 chars): {}...",
            &result.session_token.chars().take(50).collect::<String>()
        );

        self.handle_auth_result(result.clone()).await?;

        // Get user and permissions
        let user: User = self
            .api_client
            .get("/users/me")
            .await
            .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?;
        let permissions: UserPermissions = self
            .api_client
            .get("/users/me/permissions")
            .await
            .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?;

        Ok(PlayerAuthResult {
            user,
            permissions,
            device_has_pin: result.requires_pin_setup,
        })
    }

    /// Authenticate with PIN
    pub async fn authenticate_pin(
        &self,
        user_id: Uuid,
        pin: String,
    ) -> AuthResult<PlayerAuthResult> {
        let device_id = self.get_or_create_device_id().await?;

        let request = PinLoginRequest {
            user_id,
            device_id,
            pin,
        };

        let result: ServerAuthResult = self
            .api_client
            .post("/auth/device/pin", &request)
            .await
            .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?;

        self.handle_auth_result(result.clone()).await?;

        // Get user and permissions
        let user: User = self
            .api_client
            .get("/users/me")
            .await
            .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?;
        let permissions: UserPermissions = self
            .api_client
            .get("/users/me/permissions")
            .await
            .map_err(|e| AuthError::Network(NetworkError::RequestFailed(e.to_string())))?;

        Ok(PlayerAuthResult {
            user,
            permissions,
            device_has_pin: true, // If PIN login succeeded, they have a PIN
        })
    }

    /// Enable admin PIN unlock (stub for now)
    pub async fn enable_admin_pin_unlock(&self) -> AuthResult<()> {
        // TODO: Implement admin PIN unlock
        Ok(())
    }

    /// Disable admin PIN unlock (stub for now)
    pub async fn disable_admin_pin_unlock(&self) -> AuthResult<()> {
        // TODO: Implement admin PIN unlock
        Ok(())
    }

    /// Get access to auth state store (for subscriptions)
    pub fn auth_state(&self) -> &AuthStateStore {
        &self.auth_state
    }
}

/// Get device name from system
fn get_device_name() -> String {
    #[cfg(target_os = "macos")]
    {
        // Try to get computer name on macOS
        if let Ok(output) = std::process::Command::new("scutil")
            .arg("--get")
            .arg("ComputerName")
            .output()
        {
            if let Ok(name) = String::from_utf8(output.stdout) {
                let name = name.trim();
                if !name.is_empty() {
                    return name.to_string();
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        // Try to get hostname on Linux
        if let Ok(hostname) = std::fs::read_to_string("/etc/hostname") {
            let hostname = hostname.trim();
            if !hostname.is_empty() {
                return hostname.to_string();
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Try to get computer name on Windows
        if let Ok(output) = std::process::Command::new("hostname").output() {
            if let Ok(name) = String::from_utf8(output.stdout) {
                let name = name.trim();
                if !name.is_empty() {
                    return name.to_string();
                }
            }
        }
    }

    // Fallback to generic name
    format!("{} Device", get_current_platform().as_ref())
}

/// Check if a token is expired
#[cfg(any(test, feature = "testing"))]
pub fn is_token_expired(token: &AuthToken) -> bool {
    is_token_expired_impl(token)
}

/// Check if a token is expired (internal implementation)
#[cfg(not(any(test, feature = "testing")))]
fn is_token_expired(token: &AuthToken) -> bool {
    is_token_expired_impl(token)
}

/// Actual implementation of token expiry check
fn is_token_expired_impl(token: &AuthToken) -> bool {
    // First check if this looks like a JWT token (has 3 parts separated by dots)
    let parts: Vec<&str> = token.access_token.split('.').collect();
    if parts.len() != 3 {
        // Not a JWT token - likely a simple session token
        // For non-JWT tokens, use the expires_in field and stored_at time
        info!(
            "Token is not JWT format (parts: {}), using expires_in field",
            parts.len()
        );

        // We can't check expiry for non-JWT tokens without stored_at time
        // So we'll consider them valid if expires_in > buffer
        if token.expires_in > TOKEN_EXPIRY_BUFFER_SECONDS as u32 {
            info!(
                "Non-JWT token still valid based on expires_in: {} seconds",
                token.expires_in
            );
            return false;
        } else {
            warn!(
                "Non-JWT token expired based on expires_in: {} seconds",
                token.expires_in
            );
            return true;
        }
    }

    // Try to decode the JWT header without validation to check expiry
    match decode_header(&token.access_token) {
        Ok(_) => {
            // Create a validation that only checks expiry, not signature
            let mut validation = Validation::new(Algorithm::default());
            validation.insecure_disable_signature_validation();
            validation.validate_exp = true;
            validation.leeway = 0;

            // Try to decode with a dummy key (signature validation is disabled)
            match decode::<JwtClaims>(
                &token.access_token,
                &DecodingKey::from_secret(b"dummy"),
                &validation,
            ) {
                Ok(token_data) => {
                    // Check if token has expired
                    if let Some(exp) = token_data.claims.exp {
                        let now = Utc::now().timestamp();
                        let seconds_until_expiry = exp - now;
                        let is_expired = now >= exp - TOKEN_EXPIRY_BUFFER_SECONDS;

                        if is_expired {
                            info!(
                                "JWT token considered expired: {} seconds until actual expiry (buffer: {} seconds)",
                                seconds_until_expiry, TOKEN_EXPIRY_BUFFER_SECONDS
                            );
                        } else {
                            debug!(
                                "JWT token still valid: {} seconds until expiry",
                                seconds_until_expiry
                            );
                        }

                        is_expired
                    } else {
                        // No expiry claim, consider it expired
                        warn!("JWT token has no expiry claim, considering it expired");
                        true
                    }
                }
                Err(e) => {
                    // Failed to decode, consider it expired
                    warn!("Failed to decode JWT token for expiry check: {}", e);
                    true
                }
            }
        }
        Err(e) => {
            // Not a valid JWT, consider it expired
            warn!("Not a valid JWT token header: {}", e);
            true
        }
    }
}

/// Get the current platform
fn get_current_platform() -> Platform {
    #[cfg(target_os = "macos")]
    return Platform::MacOS;

    #[cfg(target_os = "linux")]
    return Platform::Linux;

    #[cfg(target_os = "windows")]
    return Platform::Windows;

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    return Platform::Unknown;
}

/// Extract expiry time from JWT token (returns seconds until expiry)
fn extract_token_expiry(token_str: &str) -> Option<i64> {
    // First check if this looks like a JWT token (has 3 parts separated by dots)
    let parts: Vec<&str> = token_str.split('.').collect();
    if parts.len() != 3 {
        // Not a JWT token - likely a simple session token
        info!(
            "Token is not JWT format (parts: {}), cannot extract expiry",
            parts.len()
        );
        return None;
    }

    // Try to decode the JWT header without validation to check expiry
    match decode_header(token_str) {
        Ok(_) => {
            // Create a validation that only checks expiry, not signature
            let mut validation = Validation::new(Algorithm::default());
            validation.insecure_disable_signature_validation();
            validation.validate_exp = false; // Don't validate expiry, we just want to read it
            validation.leeway = 0;

            // Try to decode with a dummy key (signature validation is disabled)
            match decode::<JwtClaims>(token_str, &DecodingKey::from_secret(b"dummy"), &validation) {
                Ok(token_data) => {
                    // Calculate seconds until expiry
                    if let Some(exp) = token_data.claims.exp {
                        let now = Utc::now().timestamp();
                        let seconds_until_expiry = exp - now;
                        if seconds_until_expiry > 0 {
                            info!(
                                "Extracted JWT expiry: {} seconds from now",
                                seconds_until_expiry
                            );
                            Some(seconds_until_expiry)
                        } else {
                            warn!("JWT token already expired");
                            None // Already expired
                        }
                    } else {
                        warn!("JWT has no expiry claim");
                        None // No expiry claim
                    }
                }
                Err(e) => {
                    warn!("Failed to decode JWT for expiry extraction: {}", e);
                    None
                }
            }
        }
        Err(e) => {
            warn!("Failed to decode JWT header: {}", e);
            None
        }
    }
}
