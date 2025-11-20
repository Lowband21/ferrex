use crate::domains::auth::dto::UserListItemDto;
use crate::domains::auth::errors::{
    AuthError, AuthResult, DeviceError, NetworkError, StorageError, TokenError,
};
use crate::domains::auth::state_types::{AuthState, AuthStateStore};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::{DateTime, Utc};
use directories::ProjectDirs;
use ed25519_dalek::{Signature, Signer, SigningKey};
use ferrex_core::api::routes::v1;
use ferrex_core::domain::users::auth::{
    device::DeviceInfo, domain::value_objects::SessionScope,
};
use ferrex_core::player_prelude::{
    ApiResponse, AuthToken, LoginRequest, Platform, RegisterRequest, User,
    UserPermissions,
};
use log::{error, info, warn};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, OnceCell};
use uuid::Uuid;

use crate::domains::auth::hardware_fingerprint::generate_hardware_fingerprint;
use crate::domains::auth::storage::{AuthStorage, StoredAuth};
use crate::infra::api_client::ApiClient;

#[derive(Debug, Serialize)]
struct RefreshTokenRequest {
    refresh_token: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoLoginScope {
    /// Only update device-local state (trust record, cache).
    DeviceOnly,
    /// Update both device-local state and the user-wide server preference.
    UserDefault,
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
            AuthError::Internal(format!(
                "Failed to serialize device identity: {}",
                e
            ))
        })?;
        tokio::fs::write(&path, content)
            .await
            .map_err(|e| AuthError::Storage(StorageError::WriteFailed(e)))?;
        Ok(())
    }

    fn config_path() -> AuthResult<PathBuf> {
        // Use a distinct app name under demo mode so device identity does not
        // collide with the production profile.
        let app_name = if is_demo_mode_enabled() {
            "ferrex-player-demo"
        } else {
            "ferrex-player"
        };
        let proj_dirs =
            ProjectDirs::from("", "ferrex", app_name).ok_or_else(|| {
                AuthError::Storage(StorageError::InitFailed(
                    "Unable to determine config directory".to_string(),
                ))
            })?;
        Ok(proj_dirs.config_dir().join("device.json"))
    }
}

fn is_demo_mode_enabled_env() -> bool {
    let env_value = std::env::var("FERREX_PLAYER_DEMO_MODE")
        .or_else(|_| std::env::var("FERREX_DEMO_MODE"))
        .unwrap_or_default();
    matches!(
        env_value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes"
    )
}

fn is_demo_mode_enabled() -> bool {
    if is_demo_mode_enabled_env() {
        return true;
    }
    std::env::args().any(|arg| arg == "--demo")
}

#[derive(Debug, Serialize)]
pub struct DeviceLoginRequest {
    pub username: String,
    pub password: String,
    pub device_info: Option<DeviceInfo>,
    pub remember_device: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_public_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_key_alg: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PinLoginRequest {
    pub device_id: Uuid,
    /// Client-derived PIN proof (PHC string)
    pub client_proof: String,
    pub challenge_id: Uuid,
    pub device_signature: String,
}

#[derive(Debug, Serialize)]
pub struct SetPinRequest {
    pub device_id: Uuid,
    /// Client-derived PIN proof (PHC string)
    pub client_proof: String,
    pub challenge_id: Uuid,
    pub device_signature: String,
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
/// - A 60-second buffer is applied when loading tokens
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
    device_trust_expires_at: Arc<Mutex<Option<DateTime<Utc>>>>,
}

impl AuthManager {
    pub fn new(api_client: ApiClient) -> Self {
        let auth_storage = match AuthStorage::new() {
            Ok(storage) => Arc::new(storage),
            Err(e) => {
                // Rationale: Do not crash the application if the platform config dir is unavailable.
                // Instead, fall back to a temp-file path, effectively disabling persistence across restarts
                // while allowing the app to run. This is safer for public release.
                warn!(
                    "Failed to create auth storage at platform path: {}. Falling back to temp path (persistence disabled for this run).",
                    e
                );
                let fallback = std::env::temp_dir()
                    .join("ferrex-player")
                    .join("auth_cache.disabled.enc");
                Arc::new(AuthStorage::with_cache_path(fallback))
            }
        };

        let manager = Self {
            api_client: api_client.clone(),
            auth_state: AuthStateStore::new(),
            device_id: OnceCell::new(),
            device_fingerprint: OnceCell::new(),
            auth_storage,
            device_trust_expires_at: Arc::new(Mutex::new(None)),
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
                            .map_err(|e| {
                                anyhow::anyhow!("Token refresh failed: {}", e)
                            })
                    }
                })
                .await;
        });

        manager
    }

    pub fn auth_storage(&self) -> &Arc<AuthStorage> {
        &self.auth_storage
    }

    pub async fn apply_stored_auth(
        &self,
        stored_auth: StoredAuth,
    ) -> AuthResult<()> {
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

                if matches!(
                    &err,
                    AuthError::Network(NetworkError::InvalidCredentials)
                ) && let Err(clear_err) = self.clear_keychain().await
                {
                    warn!("Failed to clear invalid auth cache: {}", clear_err);
                }

                Err(err)
            }
        }
    }

    /// Validate that the currently configured session is still authorized
    pub async fn validate_session(
        &self,
    ) -> AuthResult<(User, UserPermissions)> {
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
                self.auth_state.authenticate(
                    user.clone(),
                    token,
                    permissions.clone(),
                    server_url,
                );

                if let Err(err) = self.save_current_auth().await {
                    warn!("Failed to persist refreshed auth: {}", err);
                }

                Ok((user, permissions))
            }
            Err(err) => {
                self.api_client.set_token(None).await;
                self.auth_state.logout();

                if matches!(
                    &err,
                    AuthError::Network(NetworkError::InvalidCredentials)
                ) && let Err(clear_err) = self.clear_keychain().await
                {
                    warn!("Failed to clear invalid auth cache: {}", clear_err);
                }

                Err(err)
            }
        }
    }

    async fn fetch_user_and_permissions(
        &self,
    ) -> AuthResult<(User, UserPermissions)> {
        let user: User = self.fetch_api_data(v1::users::CURRENT).await?;
        let permissions: UserPermissions =
            self.fetch_api_data(v1::roles::MY_PERMISSIONS).await?;
        Ok((user, permissions))
    }

    async fn fetch_api_data<T>(&self, path: &str) -> AuthResult<T>
    where
        T: DeserializeOwned,
    {
        let url = self.api_client.build_url(path);
        let request = self.api_client.client.get(&url);
        let request = self.api_client.build_request(request).await;
        let response = request.send().await.map_err(|e| {
            AuthError::Network(NetworkError::RequestFailed(e.to_string()))
        })?;

        match response.status() {
            StatusCode::OK => {
                let api_response: ApiResponse<T> =
                    response.json().await.map_err(|e| {
                        AuthError::Network(NetworkError::InvalidResponse(
                            e.to_string(),
                        ))
                    })?;

                api_response.data.ok_or_else(|| {
                    AuthError::Network(NetworkError::InvalidResponse(format!(
                        "No data returned for {}",
                        path
                    )))
                })
            }
            StatusCode::UNAUTHORIZED => {
                Err(AuthError::Network(NetworkError::InvalidCredentials))
            }
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
        let hardware_fingerprint =
            generate_hardware_fingerprint().await.map_err(|e| {
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
                AuthError::Storage(StorageError::WriteFailed(
                    std::io::Error::other(format!(
                        "Failed to save auth: {}",
                        e
                    )),
                ))
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
            AuthState::Authenticated { token, .. } => {
                Some(token.refresh_token.clone())
            }
            _ => None,
        });

        let refresh_token = refresh_token
            .ok_or_else(|| AuthError::Token(TokenError::NotAuthenticated))?;

        if refresh_token.is_empty() {
            return Err(AuthError::Token(TokenError::RefreshTokenMissing));
        }

        info!("[AuthManager] Attempting to refresh access token");

        // Temporarily disable the refresh callback to avoid infinite recursion
        let response: AuthToken = {
            // Create a new client without callback for this request
            let temp_client =
                ApiClient::new(self.api_client.base_url().to_string());
            temp_client
                .set_token(Some(AuthToken {
                    access_token: String::new(),
                    refresh_token: refresh_token.clone(),
                    expires_in: 0,
                    session_id: None,
                    device_session_id: None,
                    user_id: None,
                    scope: SessionScope::Full,
                }))
                .await;

            temp_client
                .post(v1::auth::REFRESH, &RefreshTokenRequest { refresh_token })
                .await
                .map_err(|e| {
                    warn!("[AuthManager] Token refresh failed: {}", e);
                    AuthError::Network(NetworkError::RequestFailed(
                        e.to_string(),
                    ))
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
                } => Some((
                    user.clone(),
                    permissions.clone(),
                    server_url.clone(),
                )),
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
        let state_snapshot = self.auth_state.current();
        let stored_auth = if let AuthState::Authenticated {
            user,
            token,
            permissions,
            server_url,
        } = state_snapshot
        {
            info!(
                "Saving auth with token expiring in {} seconds",
                token.expires_in
            );
            let now = Utc::now();
            let trust_expires_at = {
                let guard = self.device_trust_expires_at.lock().await;
                *guard
            };

            Some(StoredAuth {
                token: token.clone(),
                user: user.clone(),
                server_url,
                permissions: Some(permissions.clone()),
                stored_at: now,
                device_trust_expires_at: trust_expires_at,
                refresh_token: Some(token.refresh_token.clone()),
            })
        } else {
            None
        };

        match stored_auth {
            Some(auth) => self.save_to_keychain(&auth).await,
            None => Err(AuthError::NotAuthenticated),
        }
    }

    /// Clear stored authentication from encrypted storage
    pub async fn clear_keychain(&self) -> AuthResult<()> {
        self.auth_storage.clear_auth().await.map_err(|e| {
            AuthError::Storage(StorageError::WriteFailed(
                std::io::Error::other(format!("Failed to clear auth: {}", e)),
            ))
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
            .post(v1::auth::LOGIN, &request)
            .await
            .map_err(|e| {
                AuthError::Network(NetworkError::RequestFailed(e.to_string()))
            })?;

        info!(
            "Login received token with expires_in: {} seconds ({} minutes)",
            token.expires_in,
            token.expires_in / 60
        );

        // Set token in API client
        self.api_client.set_token(Some(token.clone())).await;

        // Get user profile
        let user: User =
            self.api_client.get(v1::users::CURRENT).await.map_err(|e| {
                AuthError::Network(NetworkError::RequestFailed(e.to_string()))
            })?;

        // Get user permissions
        let permissions: UserPermissions = self
            .api_client
            .get(v1::roles::MY_PERMISSIONS)
            .await
            .map_err(|e| {
                AuthError::Network(NetworkError::RequestFailed(e.to_string()))
            })?;

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
            .post(v1::auth::REGISTER, &request)
            .await
            .map_err(|e| {
                AuthError::Network(NetworkError::RequestFailed(e.to_string()))
            })?;

        // Set token in API client
        self.api_client.set_token(Some(token.clone())).await;

        // Get user profile
        let user: User =
            self.api_client.get(v1::users::CURRENT).await.map_err(|e| {
                AuthError::Network(NetworkError::RequestFailed(e.to_string()))
            })?;

        // Get user permissions
        let permissions: UserPermissions = self
            .api_client
            .get(v1::roles::MY_PERMISSIONS)
            .await
            .map_err(|e| {
                AuthError::Network(NetworkError::RequestFailed(e.to_string()))
            })?;

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
                api_client.post::<EmptyRequest, serde_json::Value>(
                    v1::auth::LOGOUT,
                    &EmptyRequest {},
                ),
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
        let user = self
            .get_current_user()
            .await
            .ok_or(AuthError::NotAuthenticated)?;

        // Load signing key
        let signing_key_bytes = self
            .auth_storage
            .load_device_key()
            .await
            .map_err(|e| {
                error!("failed to load device key: {e}");
                AuthError::Storage(StorageError::InitFailed(
                    "device key unavailable".to_string(),
                ))
            })?
            .ok_or_else(|| {
                AuthError::Storage(StorageError::InitFailed(
                    "device key not found".to_string(),
                ))
            })?;
        let signing_key = SigningKey::from_bytes(
            signing_key_bytes.as_slice().try_into().map_err(|_| {
                AuthError::Storage(StorageError::InitFailed(
                    "invalid device key".to_string(),
                ))
            })?,
        );

        // Request a challenge
        #[derive(serde::Serialize)]
        struct ChallengeReq {
            device_id: Uuid,
        }
        #[derive(serde::Deserialize)]
        struct ChallengeResp {
            challenge_id: Uuid,
            nonce: String,
            _expires_in_secs: i64,
            pin_salt: String,
        }

        let challenge: ChallengeResp = self
            .api_client
            .post(v1::auth::device::PIN_CHALLENGE, &ChallengeReq { device_id })
            .await
            .map_err(|e| {
                AuthError::Network(NetworkError::RequestFailed(e.to_string()))
            })?;

        let nonce =
            BASE64.decode(challenge.nonce.as_bytes()).map_err(|_| {
                AuthError::Storage(StorageError::InitFailed(
                    "invalid nonce".to_string(),
                ))
            })?;
        let pin_salt =
            BASE64.decode(challenge.pin_salt.as_bytes()).map_err(|_| {
                AuthError::Internal("invalid PIN salt from server".to_string())
            })?;
        // Build message v1: "Ferrex-PIN-v1" || challenge_id || nonce || user_id
        const CTX: &[u8] = b"Ferrex-PIN-v1";
        let mut msg = Vec::with_capacity(CTX.len() + 16 + nonce.len() + 16);
        msg.extend_from_slice(CTX);
        msg.extend_from_slice(challenge.challenge_id.as_bytes());
        msg.extend_from_slice(&nonce);
        msg.extend_from_slice(user.id.as_bytes());
        let sig: Signature = signing_key.sign(&msg);
        let sig_b64 = BASE64.encode(sig.to_bytes());
        let client_proof = Self::derive_client_pin_proof(&pin, &pin_salt)?;
        let request = SetPinRequest {
            device_id,
            client_proof,
            challenge_id: challenge.challenge_id,
            device_signature: sig_b64,
        };

        self.api_client
            .post::<_, serde_json::Value>(v1::auth::device::SET_PIN, &request)
            .await
            .map_err(|e| {
                AuthError::Network(NetworkError::RequestFailed(e.to_string()))
            })?;

        Ok(())
    }

    /// Change PIN for current device
    pub async fn change_device_pin(
        &self,
        current_pin: String,
        new_pin: String,
    ) -> AuthResult<()> {
        let device_id = self.get_or_create_device_id().await?;

        #[derive(serde::Serialize)]
        struct ChangePinRequest {
            device_id: Uuid,
            current_proof: String,
            new_proof: String,
            challenge_id: Uuid,
            device_signature: String,
        }

        let user = self
            .get_current_user()
            .await
            .ok_or(AuthError::NotAuthenticated)?;
        // Load signing key
        let signing_key_bytes = self
            .auth_storage
            .load_device_key()
            .await
            .map_err(|e| {
                error!("failed to load device key: {e}");
                AuthError::Storage(StorageError::InitFailed(
                    "device key unavailable".to_string(),
                ))
            })?
            .ok_or_else(|| {
                AuthError::Storage(StorageError::InitFailed(
                    "device key not found".to_string(),
                ))
            })?;
        let signing_key = SigningKey::from_bytes(
            signing_key_bytes.as_slice().try_into().map_err(|_| {
                AuthError::Storage(StorageError::InitFailed(
                    "invalid device key".to_string(),
                ))
            })?,
        );

        // Request a challenge
        #[derive(serde::Serialize)]
        struct ChallengeReq {
            device_id: Uuid,
        }
        #[derive(serde::Deserialize)]
        struct ChallengeResp {
            challenge_id: Uuid,
            nonce: String,
            _expires_in_secs: i64,
            pin_salt: String,
        }
        let challenge: ChallengeResp = self
            .api_client
            .post(v1::auth::device::PIN_CHALLENGE, &ChallengeReq { device_id })
            .await
            .map_err(|e| {
                AuthError::Network(NetworkError::RequestFailed(e.to_string()))
            })?;
        let nonce =
            BASE64.decode(challenge.nonce.as_bytes()).map_err(|_| {
                AuthError::Storage(StorageError::InitFailed(
                    "invalid nonce".to_string(),
                ))
            })?;
        let pin_salt =
            BASE64.decode(challenge.pin_salt.as_bytes()).map_err(|_| {
                AuthError::Internal("invalid PIN salt from server".to_string())
            })?;
        let current_proof =
            Self::derive_client_pin_proof(&current_pin, &pin_salt)?;
        let new_proof = Self::derive_client_pin_proof(&new_pin, &pin_salt)?;
        // Build message v1: "Ferrex-PIN-v1" || challenge_id || nonce || user_id
        const CTX: &[u8] = b"Ferrex-PIN-v1";
        let mut msg = Vec::with_capacity(CTX.len() + 16 + nonce.len() + 16);
        msg.extend_from_slice(CTX);
        msg.extend_from_slice(challenge.challenge_id.as_bytes());
        msg.extend_from_slice(&nonce);
        msg.extend_from_slice(user.id.as_bytes());
        let sig: Signature = signing_key.sign(&msg);
        let sig_b64 = BASE64.encode(sig.to_bytes());

        let request = ChangePinRequest {
            device_id,
            current_proof,
            new_proof,
            challenge_id: challenge.challenge_id,
            device_signature: sig_b64,
        };

        self.api_client
            .post::<_, serde_json::Value>(
                v1::auth::device::CHANGE_PIN,
                &request,
            )
            .await
            .map_err(|e| {
                AuthError::Network(NetworkError::RequestFailed(e.to_string()))
            })?;

        Ok(())
    }

    async fn perform_login(
        &self,
        username: String,
        secret: String,
        remember_device: Option<bool>,
    ) -> AuthResult<PlayerAuthResult> {
        let server_url = self.api_client.base_url().to_string();
        let (user, permissions) =
            self.login(username, secret, server_url).await?;

        if let Some(remember) = remember_device
            && let Err(err) = self
                .set_auto_login_scope(remember, AutoLoginScope::DeviceOnly)
                .await
        {
            warn!("Failed to update auto-login preference: {}", err);
        }

        let device_status = self.check_device_auth(user.id).await?;

        // Persist lightweight user summary locally to enable offline user cards
        let summary = crate::domains::auth::dto::UserListItemDto {
            id: user.id,
            username: user.username.clone(),
            display_name: user.display_name.clone(),
            avatar_url: user.avatar_url.clone(),
            has_pin: device_status.has_pin,
            last_login: Some(chrono::Utc::now()),
        };
        if let Err(e) = self
            .auth_storage
            .upsert_user_summary_for_server(
                self.api_client.base_url(),
                &summary,
            )
            .await
        {
            warn!("Failed to persist user summary: {}", e);
        }

        Ok(PlayerAuthResult {
            user,
            permissions,
            device_has_pin: device_status.has_pin,
        })
    }

    /// Authenticate using username/password and optionally remember this device.
    pub async fn authenticate_device(
        &self,
        username: String,
        password: String,
        remember_device: bool,
    ) -> AuthResult<PlayerAuthResult> {
        self.perform_login(username, password, Some(remember_device))
            .await
    }

    /// Authenticate using a stored PIN for the selected user.
    pub async fn authenticate_pin(
        &self,
        user_id: Uuid,
        pin: String,
    ) -> AuthResult<PlayerAuthResult> {
        let users = self.get_all_users().await?;
        let username = users
            .into_iter()
            .find(|candidate| candidate.id == user_id)
            .map(|user| user.username)
            .ok_or(AuthError::UserNotFound(user_id))?;

        self.perform_login(username, pin, None).await
    }

    /// Check if user has PIN on this device
    pub async fn check_device_auth(
        &self,
        user_id: Uuid,
    ) -> AuthResult<DeviceAuthStatus> {
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

        // If not authenticated, avoid online probing; treat as needs login
        let is_authed = self.auth_state.with_state(|state| {
            matches!(state, AuthState::Authenticated { .. })
        });
        if !is_authed {
            log::info!(
                "[Auth] Not authenticated; deferring device status check for user {}",
                user_id
            );
            return Ok(DeviceAuthStatus {
                device_registered: false,
                has_pin: false,
                remaining_attempts: None,
            });
        }

        // Online check (requires authenticated session)
        let device_id = self.get_or_create_device_id().await?;
        log::info!(
            "[Auth] Checking device status online for user {} on device {}",
            user_id,
            device_id
        );

        // Authenticated endpoint; user is inferred from the session
        let status_path =
            format!("{}?device_id={}", v1::auth::device::STATUS, device_id);

        let status: DeviceAuthStatus =
            self.api_client.get(&status_path).await.map_err(|e| {
                AuthError::Network(NetworkError::RequestFailed(e.to_string()))
            })?;

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

    /// Check cached device status using locally stored user summaries
    async fn check_cached_device_status(
        &self,
        user_id: Uuid,
    ) -> Option<DeviceAuthStatus> {
        if let Ok(users) = self
            .auth_storage
            .load_user_summaries_for_server(self.api_client.base_url())
            .await
        {
            if let Some(u) = users.into_iter().find(|u| u.id == user_id) {
                return Some(DeviceAuthStatus {
                    device_registered: true,
                    has_pin: u.has_pin,
                    remaining_attempts: None,
                });
            }
        }
        None
    }

    /// Cache device status by updating user summary
    async fn cache_device_status(
        &self,
        user_id: Uuid,
        status: &DeviceAuthStatus,
    ) {
        if let Ok(mut users) = self
            .auth_storage
            .load_user_summaries_for_server(self.api_client.base_url())
            .await
        {
            let mut updated = false;
            for u in users.iter_mut() {
                if u.id == user_id {
                    u.has_pin = status.has_pin;
                    updated = true;
                    break;
                }
            }
            if updated {
                if let Err(e) = self
                    .auth_storage
                    .save_user_summaries_for_server(
                        self.api_client.base_url(),
                        &users,
                    )
                    .await
                {
                    warn!("Failed to update cached user summaries: {}", e);
                }
            }
        }
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
        let id = Uuid::now_v7();
        let fingerprint =
            generate_hardware_fingerprint().await.map_err(|e| {
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

    /// Expose the current device identifier to callers that need to identify themselves
    pub async fn current_device_id(&self) -> AuthResult<Uuid> {
        self.get_or_create_device_id().await
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
            AuthState::Authenticated { permissions, .. } => {
                Some(permissions.clone())
            }
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

    /// Set auto-login preference scoped to either the device or the user default.
    pub async fn set_auto_login_scope(
        &self,
        enabled: bool,
        scope: AutoLoginScope,
    ) -> AuthResult<()> {
        let user = self
            .get_current_user()
            .await
            .ok_or(AuthError::NotAuthenticated)?;

        // Set device-specific auto-login
        self.auth_storage
            .set_auto_login(&user.id, enabled)
            .await
            .map_err(|e| {
                AuthError::Storage(StorageError::WriteFailed(
                    std::io::Error::other(format!(
                        "Failed to set auto-login: {}",
                        e
                    )),
                ))
            })?;

        if !enabled {
            let mut guard = self.device_trust_expires_at.lock().await;
            *guard = None;
        }

        if matches!(scope, AutoLoginScope::UserDefault) {
            // Update server-side preference so settings stay in sync across devices
            let request = json!({ "auto_login_enabled": enabled });
            self.api_client
                .put::<_, serde_json::Value>(
                    v1::users::CURRENT_PREFERENCES,
                    &request,
                )
                .await
                .map_err(|e| {
                    AuthError::Network(NetworkError::RequestFailed(
                        e.to_string(),
                    ))
                })?;

            // Update in-memory auth state with the new preference so UI stays consistent
            if let AuthState::Authenticated {
                token,
                permissions,
                server_url,
                ..
            } = self.auth_state.current()
            {
                let mut updated_user = user.clone();
                updated_user.preferences.auto_login_enabled = enabled;
                self.auth_state.authenticate(
                    updated_user,
                    token.clone(),
                    permissions.clone(),
                    server_url,
                );
            }
        }

        // Persist the updated preference to storage so auto-login reflects user intent
        if let Err(err) = self.save_current_auth().await {
            warn!("Failed to persist auto-login preference: {}", err);
        }

        Ok(())
    }

    /// Backwards-compatible helper that applies the user-default scope.
    pub async fn set_auto_login(&self, enabled: bool) -> AuthResult<()> {
        self.set_auto_login_scope(enabled, AutoLoginScope::UserDefault)
            .await
    }

    /// Get all users (for user selection screen)
    ///
    /// This method sends the device fingerprint to get appropriate user information
    /// based on whether the device is known/trusted.
    pub async fn get_all_users(&self) -> AuthResult<Vec<UserListItemDto>> {
        // Check if we have an active auth token
        let has_auth = self.auth_state.with_state(|state| {
            matches!(state, AuthState::Authenticated { .. })
        });

        let users: Vec<UserListItemDto> = if has_auth {
            // Use authenticated endpoint and update local cache
            let fetched: Vec<UserListItemDto> =
                self.api_client.get(v1::users::LIST_AUTH).await.map_err(
                    |e| {
                        AuthError::Network(NetworkError::RequestFailed(
                            e.to_string(),
                        ))
                    },
                )?;
            if let Err(e) = self
                .auth_storage
                .save_user_summaries_for_server(
                    self.api_client.base_url(),
                    &fetched,
                )
                .await
            {
                warn!("Failed to save user summaries: {}", e);
            }
            fetched
        } else {
            // When unauthenticated, proactively check server setup status.
            // If the server needs setup, cached users are certainly stale; clear and return empty.
            // If the check fails with an authorization/HTTP error (common on fresh servers that
            // restrict the setup endpoint), also clear cache to avoid showing users from a previous
            // database instance. Only fall back to cached users when the error strongly suggests a
            // connectivity problem (offline/timeout/connection refused), where cached users act as
            // an offline hint.
            match self.check_setup_status().await {
                Ok(true) => {
                    if let Err(e) = self
                        .auth_storage
                        .clear_user_summaries_for_server(
                            self.api_client.base_url(),
                        )
                        .await
                    {
                        warn!("Failed to clear cached user summaries: {}", e);
                    }
                    Vec::new()
                }
                Ok(false) => Vec::new(),
                Err(err) => {
                    let msg = err.to_string().to_ascii_lowercase();
                    let looks_like_connectivity = msg.contains("timeout")
                        || msg.contains("timed out")
                        || msg.contains("dns")
                        || msg.contains("failed to resolve")
                        || msg.contains("connection refused")
                        || msg.contains("connection reset")
                        || msg.contains("no route to host")
                        || msg.contains("network unreachable")
                        || msg.contains("host unreachable");

                    if looks_like_connectivity {
                        match self
                            .auth_storage
                            .load_user_summaries_for_server(
                                self.api_client.base_url(),
                            )
                            .await
                        {
                            Ok(users) => users,
                            Err(e) => {
                                warn!(
                                    "Failed to load cached user summaries during offline fallback: {}",
                                    e
                                );
                                Vec::new()
                            }
                        }
                    } else {
                        // Not a connectivity error: treat this as a hard failure and clear any stale cache
                        if let Err(e) = self
                            .auth_storage
                            .clear_user_summaries_for_server(
                                self.api_client.base_url(),
                            )
                            .await
                        {
                            warn!(
                                "Failed to clear cached user summaries after setup-status error: {}",
                                e
                            );
                        }
                        Vec::new()
                    }
                }
            }
        };

        Ok(users)
    }

    /// Check setup status
    pub async fn check_setup_status(&self) -> AuthResult<bool> {
        // TODO: Utilize setup statistics
        #[derive(Debug, Deserialize)]
        struct SetupStatus {
            needs_setup: bool,
            _has_admin: bool,
            _user_count: usize,
            _library_count: usize,
        }

        let status: SetupStatus =
            self.api_client.get(v1::setup::STATUS).await.map_err(|e| {
                AuthError::Network(NetworkError::RequestFailed(e.to_string()))
            })?;
        Ok(status.needs_setup)
    }

    /// Clear the user cache for the current server base URL
    pub async fn clear_current_server_user_cache(&self) -> AuthResult<()> {
        self.auth_storage
            .clear_user_summaries_for_server(self.api_client.base_url())
            .await
            .map_err(|e| {
                AuthError::Storage(StorageError::WriteFailed(
                    std::io::Error::other(format!(
                        "Failed to clear server-scoped user cache: {}",
                        e
                    )),
                ))
            })
    }

    /// Derive a deterministic client-side PIN proof (PHC string) scoped to the provided salt.
    ///
    /// Construction:
    /// - password material = pin || user_salt (server-managed)
    /// - Argon2id params: m=64MiB, t=3, p=1, outlen=32
    /// - Argon2 salt = user_salt
    fn derive_client_pin_proof(
        pin: &str,
        user_salt: &[u8],
    ) -> AuthResult<String> {
        use argon2::password_hash::{PasswordHasher, SaltString};
        use argon2::{Algorithm, Argon2, Params, ParamsBuilder, Version};

        if user_salt.is_empty() {
            return Err(AuthError::Internal("missing PIN salt".to_string()));
        }

        // Password material: pin || user_salt bytes
        let mut material = Vec::with_capacity(pin.len() + user_salt.len());
        material.extend_from_slice(pin.as_bytes());
        material.extend_from_slice(user_salt);

        let salt = SaltString::encode_b64(user_salt).map_err(|_| {
            AuthError::Internal("invalid PIN salt payload".to_string())
        })?;

        let params: Params = ParamsBuilder::new()
            .m_cost(64 * 1024)
            .t_cost(3)
            .p_cost(1)
            .output_len(32)
            .build()
            .map_err(|_| {
                AuthError::Internal("invalid Argon2 parameters".to_string())
            })?;
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

        let phc = argon2
            .hash_password(&material, &salt)
            .map_err(|_| {
                AuthError::Internal("failed to derive PIN proof".to_string())
            })?
            .to_string();
        Ok(phc)
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

/// Get the current platform
fn get_current_platform() -> Platform {
    #[cfg(target_os = "macos")]
    return Platform::MacOS;

    #[cfg(target_os = "linux")]
    return Platform::Linux;

    #[cfg(target_os = "windows")]
    return Platform::Windows;

    #[cfg(not(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows"
    )))]
    return Platform::Unknown;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::auth::dto::UserListItemDto;
    use crate::domains::auth::storage::AUTH_CACHE_FILE;
    use sha2::{Digest, Sha256};
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn derive_pin_proof_requires_non_empty_salt() {
        let result = AuthManager::derive_client_pin_proof("1234", &[]);
        assert!(result.is_err(), "empty salt should be rejected");
    }

    #[test]
    fn derive_pin_proof_varies_with_salt() {
        let salt_a = vec![0xAA; 16];
        let salt_b = vec![0xBB; 16];

        let proof_a = AuthManager::derive_client_pin_proof("2580", &salt_a)
            .expect("proof for salt A");
        let proof_a_repeat =
            AuthManager::derive_client_pin_proof("2580", &salt_a)
                .expect("repeat proof for salt A");
        let proof_b = AuthManager::derive_client_pin_proof("2580", &salt_b)
            .expect("proof for salt B");

        assert_eq!(
            proof_a, proof_a_repeat,
            "same salt should yield deterministic proof"
        );
        assert_ne!(
            proof_a, proof_b,
            "different salts must produce distinct proofs"
        );
    }

    // Minimal HTTP 401 responder for a single request
    async fn spawn_unauthorized_server() -> (String, tokio::task::JoinHandle<()>)
    {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            if let Ok((mut socket, _peer)) = listener.accept().await {
                // Read and discard request
                let mut buf = [0u8; 1024];
                let _ = socket.read(&mut buf).await;
                // Respond with 401 Unauthorized and minimal body
                let resp = b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = socket.write_all(resp).await;
                let _ = socket.shutdown().await;
            }
        });
        (format!("http://{}", addr), handle)
    }

    fn server_hash(base_url: &str) -> String {
        let normalized = base_url.trim().trim_end_matches('/').to_lowercase();
        let mut hasher = Sha256::new();
        hasher.update(normalized.as_bytes());
        let digest = hasher.finalize();
        let mut out = String::with_capacity(digest.len() * 2);
        for b in digest {
            use std::fmt::Write as _;
            let _ = write!(&mut out, "{:02x}", b);
        }
        out
    }

    #[tokio::test]
    async fn unauthenticated_401_clears_server_scoped_user_cache() {
        let (base_url, _server_handle) = spawn_unauthorized_server().await;

        // Create auth storage in a temp directory
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join(AUTH_CACHE_FILE);
        let storage = AuthStorage::with_cache_path(cache_path);

        // Pre-seed server-scoped user cache with one user
        let seed = vec![UserListItemDto {
            id: Uuid::now_v7(),
            username: "cached".into(),
            display_name: "Cached User".into(),
            avatar_url: None,
            has_pin: true,
            last_login: Some(Utc::now()),
        }];
        storage
            .save_user_summaries_for_server(&base_url, &seed)
            .await
            .unwrap();

        // Sanity: cache file exists
        let expected_cache = storage
            .cache_path()
            .parent()
            .unwrap()
            .join("servers")
            .join(server_hash(&base_url))
            .join("users_cache.json");
        assert!(expected_cache.exists());

        // Build ApiClient pointing to the unauthorized server
        let client = ApiClient::new(base_url.clone());
        let mut manager = AuthManager::new(client);
        // Inject our temp storage
        manager.auth_storage = Arc::new(storage);

        // Call get_all_users while unauthenticated (default state)
        let users = manager.get_all_users().await.unwrap();
        assert!(users.is_empty(), "expected empty list after HTTP 401");

        // The server-scoped cache file should have been cleared
        assert!(
            !expected_cache.exists(),
            "server-scoped cache file should be removed on 401"
        );
    }
}
