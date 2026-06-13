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
    device::{DeviceInfo, DeviceRegistration},
    domain::value_objects::SessionScope,
};
use ferrex_core::player_prelude::{
    ApiResponse, AuthToken, Platform, RegisterRequest, User, UserPermissions,
};
use log::{info, warn};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, OnceCell};
use uuid::Uuid;

use crate::domains::auth::hardware_fingerprint::generate_hardware_fingerprint;
use crate::domains::auth::pin_policy::{
    PIN_MAX_LENGTH, PIN_MIN_LENGTH, PinPolicyRules,
};
use crate::domains::auth::storage::{AuthStorage, StoredAuth};
use crate::infra::api_client::ApiClient;

#[derive(Debug, Serialize)]
struct RefreshTokenRequest {
    refresh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinPolicyResponse {
    #[serde(default = "default_pin_min_length")]
    pub min_length: u16,
    #[serde(default = "default_pin_max_length")]
    pub max_length: u16,
    #[serde(default = "default_true")]
    pub require_numeric: bool,
    #[serde(default = "default_true")]
    pub reject_repeated_digits: bool,
    #[serde(default = "default_max_consecutive_identical")]
    pub max_consecutive_identical: u16,
    #[serde(default = "default_true")]
    pub reject_sequential_digits: bool,
}

impl Default for PinPolicyResponse {
    fn default() -> Self {
        Self {
            min_length: default_pin_min_length(),
            max_length: default_pin_max_length(),
            require_numeric: true,
            reject_repeated_digits: true,
            max_consecutive_identical: default_max_consecutive_identical(),
            reject_sequential_digits: true,
        }
    }
}

impl From<&PinPolicyResponse> for PinPolicyRules {
    fn from(value: &PinPolicyResponse) -> Self {
        Self {
            min_length: usize::from(value.min_length),
            max_length: usize::from(value.max_length),
            require_numeric: value.require_numeric,
            reject_repeated_digits: value.reject_repeated_digits,
            max_consecutive_identical: usize::from(
                value.max_consecutive_identical,
            ),
            reject_sequential_digits: value.reject_sequential_digits,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTrustPolicyResponse {
    #[serde(default)]
    pub remember_device_default: bool,
    #[serde(default = "default_trust_duration_days")]
    pub trust_duration_days: u16,
    #[serde(default = "default_pin_max_attempts")]
    pub pin_max_attempts: u8,
    #[serde(default = "default_pin_lockout_minutes")]
    pub pin_lockout_minutes: u16,
    #[serde(default)]
    pub admin_pin_unlock_enabled: bool,
}

impl Default for DeviceTrustPolicyResponse {
    fn default() -> Self {
        Self {
            remember_device_default: false,
            trust_duration_days: default_trust_duration_days(),
            pin_max_attempts: default_pin_max_attempts(),
            pin_lockout_minutes: default_pin_lockout_minutes(),
            admin_pin_unlock_enabled: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAuthStatus {
    pub device_registered: bool,
    pub has_pin: bool,
    pub remaining_attempts: Option<u8>,
    #[serde(default)]
    pub pin_policy: PinPolicyResponse,
    #[serde(default)]
    pub device_trust_policy: DeviceTrustPolicyResponse,
}

fn default_pin_min_length() -> u16 {
    PIN_MIN_LENGTH as u16
}

fn default_pin_max_length() -> u16 {
    PIN_MAX_LENGTH as u16
}

fn default_max_consecutive_identical() -> u16 {
    2
}

fn default_trust_duration_days() -> u16 {
    30
}

fn default_pin_max_attempts() -> u8 {
    3
}

fn default_pin_lockout_minutes() -> u16 {
    5
}

fn default_true() -> bool {
    true
}

impl Default for DeviceAuthStatus {
    fn default() -> Self {
        Self {
            device_registered: false,
            has_pin: false,
            remaining_attempts: None,
            pin_policy: PinPolicyResponse::default(),
            device_trust_policy: DeviceTrustPolicyResponse::default(),
        }
    }
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

    pub async fn reset() -> AuthResult<()> {
        let path = Self::config_path()?;
        if path.exists() {
            tokio::fs::remove_file(&path).await.map_err(|e| {
                AuthError::Storage(StorageError::WriteFailed(e))
            })?;
        }
        Ok(())
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
    /// Server-side device session id returned by `/auth/device/login`.
    pub device_id: Uuid,
    /// Client-derived PIN proof (PHC string)
    pub client_proof: String,
    pub challenge_id: Uuid,
    pub device_signature: String,
}

#[derive(Debug, Serialize)]
pub struct SetPinRequest {
    /// Server-side device session id returned by `/auth/device/login`.
    pub device_id: Uuid,
    /// Client-derived PIN proof (PHC string)
    pub client_proof: String,
    pub challenge_id: Uuid,
    pub device_signature: String,
}

#[derive(Debug, Deserialize)]
struct DeviceAuthResponse {
    #[serde(flatten)]
    token: AuthToken,
    #[serde(default)]
    device_registration: Option<DeviceRegistration>,
    #[serde(default)]
    requires_pin_setup: bool,
}

#[derive(Debug, Serialize)]
struct KnownDeviceProfilesRequest {
    device_info: Option<DeviceInfo>,
}

#[derive(Debug, Deserialize)]
struct KnownDeviceProfilesResponse {
    known_device: bool,
    users: Vec<KnownDeviceUserCard>,
}

#[derive(Debug, Deserialize)]
struct KnownDeviceUserCard {
    id: Uuid,
    username: String,
    display_name: String,
    avatar_url: Option<String>,
    has_pin: bool,
}

#[derive(Debug, Serialize)]
struct PinChallengeRequest {
    /// Server-side device session id.
    device_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct PinChallengeResponse {
    challenge_id: Uuid,
    nonce: String,
    #[serde(rename = "expires_in_secs")]
    _expires_in_secs: i64,
    pin_salt: String,
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

    /// Stable local identifier used to encrypt/decrypt the remembered-auth cache.
    ///
    /// This intentionally uses the persisted `DeviceIdentity` fingerprint rather
    /// than recalculating the hardware fingerprint on every startup. The hardware
    /// fingerprint includes best-effort system details that can drift (for
    /// example filesystem sizing on some Linux mounts), while the auth cache key
    /// must remain stable for "remember me" to work.
    pub async fn device_auth_cache_fingerprint(&self) -> AuthResult<String> {
        Ok(self.get_or_create_device_identity().await?.fingerprint)
    }

    async fn get_or_create_device_identity(
        &self,
    ) -> AuthResult<DeviceIdentity> {
        if let (Some(id), Some(fingerprint)) =
            (self.device_id.get(), self.device_fingerprint.get())
        {
            return Ok(DeviceIdentity {
                id: *id,
                fingerprint: fingerprint.clone(),
                created_at: Utc::now(),
                name: get_device_name(),
            });
        }

        match DeviceIdentity::load().await {
            Ok(Some(identity)) => {
                let _ = self.device_id.set(identity.id);
                let _ =
                    self.device_fingerprint.set(identity.fingerprint.clone());
                Ok(identity)
            }
            Ok(None) => self.create_device_identity().await,
            Err(err) => {
                warn!(
                    "Failed to load device identity; resetting local identity: {}",
                    err
                );
                if let Err(reset_err) = DeviceIdentity::reset().await {
                    warn!(
                        "Failed to reset corrupt device identity: {}",
                        reset_err
                    );
                }
                self.create_device_identity().await
            }
        }
    }

    async fn create_device_identity(&self) -> AuthResult<DeviceIdentity> {
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
        Ok(identity)
    }

    fn device_info_for_identity(identity: &DeviceIdentity) -> DeviceInfo {
        DeviceInfo {
            device_id: identity.id,
            device_name: identity.name.clone(),
            platform: get_current_platform(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            hardware_id: Some(identity.fingerprint.clone()),
        }
    }

    async fn load_existing_device_signing_key(
        &self,
    ) -> AuthResult<Option<SigningKey>> {
        let key_bytes = match self.auth_storage.load_device_key().await {
            Ok(key_bytes) => key_bytes,
            Err(err) => {
                warn!(
                    "Failed to load device signing key; clearing stale key material: {}",
                    err
                );
                if let Err(clear_err) =
                    self.auth_storage.clear_device_key().await
                {
                    warn!("Failed to clear stale device key: {}", clear_err);
                }
                return Ok(None);
            }
        };

        let Some(key_bytes) = key_bytes else {
            return Ok(None);
        };

        let key_array: [u8; 32] = match key_bytes.as_slice().try_into() {
            Ok(key_array) => key_array,
            Err(_) => {
                warn!(
                    "Stored device signing key had invalid length; clearing stale key material"
                );
                if let Err(clear_err) =
                    self.auth_storage.clear_device_key().await
                {
                    warn!("Failed to clear invalid device key: {}", clear_err);
                }
                return Ok(None);
            }
        };

        Ok(Some(SigningKey::from_bytes(&key_array)))
    }

    async fn ensure_device_signing_key(&self) -> AuthResult<SigningKey> {
        if let Some(signing_key) =
            self.load_existing_device_signing_key().await?
        {
            return Ok(signing_key);
        }

        let mut key_bytes = [0u8; 32];
        getrandom::fill(&mut key_bytes).map_err(|e| {
            AuthError::Storage(StorageError::InitFailed(format!(
                "Failed to generate device signing key: {}",
                e
            )))
        })?;
        let signing_key = SigningKey::from_bytes(&key_bytes);

        if let Err(first_err) =
            self.auth_storage.save_device_key(&key_bytes).await
        {
            warn!(
                "Failed to save generated device signing key; resetting key storage: {}",
                first_err
            );
            if let Err(clear_err) = self.auth_storage.clear_device_key().await {
                warn!("Failed to reset device key storage: {}", clear_err);
            }
            self.auth_storage
                .save_device_key(&key_bytes)
                .await
                .map_err(|e| {
                    AuthError::Storage(StorageError::WriteFailed(
                        std::io::Error::other(format!(
                            "Failed to save device signing key: {}",
                            e
                        )),
                    ))
                })?;
        }

        Ok(signing_key)
    }

    async fn post_device_json<T, R>(
        &self,
        path: &str,
        body: &T,
        authenticated: bool,
        client_device_id: Option<Uuid>,
    ) -> AuthResult<R>
    where
        T: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        let url = self.api_client.build_url(path);
        let mut request = self.api_client.client.post(&url).json(body);
        if let Some(device_id) = client_device_id {
            request = request.header("X-Device-ID", device_id.to_string());
        }
        if authenticated {
            request = self.api_client.build_request(request).await;
        }

        let response = request.send().await.map_err(|e| {
            AuthError::Network(NetworkError::RequestFailed(e.to_string()))
        })?;
        self.parse_api_response(path, response).await
    }

    async fn post_device_json_no_data<T>(
        &self,
        path: &str,
        body: &T,
        authenticated: bool,
        client_device_id: Option<Uuid>,
    ) -> AuthResult<()>
    where
        T: Serialize + ?Sized,
    {
        let url = self.api_client.build_url(path);
        let mut request = self.api_client.client.post(&url).json(body);
        if let Some(device_id) = client_device_id {
            request = request.header("X-Device-ID", device_id.to_string());
        }
        if authenticated {
            request = self.api_client.build_request(request).await;
        }

        let response = request.send().await.map_err(|e| {
            AuthError::Network(NetworkError::RequestFailed(e.to_string()))
        })?;

        match response.status() {
            status if status.is_success() => Ok(()),
            StatusCode::UNAUTHORIZED => {
                Err(AuthError::Network(NetworkError::InvalidCredentials))
            }
            StatusCode::TOO_MANY_REQUESTS => {
                Err(AuthError::Network(NetworkError::RateLimited))
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

    async fn parse_api_response<R>(
        &self,
        path: &str,
        response: reqwest::Response,
    ) -> AuthResult<R>
    where
        R: DeserializeOwned,
    {
        match response.status() {
            status if status.is_success() => {
                let api_response: ApiResponse<R> =
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
            StatusCode::TOO_MANY_REQUESTS => {
                Err(AuthError::Network(NetworkError::RateLimited))
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

    async fn remember_server_device_session(
        &self,
        user_id: Uuid,
        device_session_id: Uuid,
    ) -> AuthResult<()> {
        let client_device_id = self.get_or_create_device_id().await?;
        self.auth_storage
            .save_device_session_for_server(
                self.api_client.base_url(),
                user_id,
                client_device_id,
                device_session_id,
            )
            .await
            .map_err(|e| {
                AuthError::Storage(StorageError::WriteFailed(
                    std::io::Error::other(format!(
                        "Failed to save device session: {}",
                        e
                    )),
                ))
            })
    }

    async fn stored_device_session_id_for_user(
        &self,
        user_id: Uuid,
    ) -> AuthResult<Option<Uuid>> {
        let client_device_id = self.get_or_create_device_id().await?;
        let stored = self
            .auth_storage
            .load_device_session_for_server(self.api_client.base_url(), user_id)
            .await
            .map_err(|e| {
                AuthError::Storage(StorageError::ReadFailed(
                    std::io::Error::other(format!(
                        "Failed to load device session: {}",
                        e
                    )),
                ))
            })?;

        let Some(stored) = stored else {
            return Ok(None);
        };

        if stored.client_device_id == client_device_id {
            Ok(Some(stored.device_session_id))
        } else {
            warn!(
                "Ignoring stale server device session {} for user {} because local device id changed",
                stored.device_session_id, user_id
            );
            if let Err(err) = self
                .auth_storage
                .clear_device_session_for_server(
                    self.api_client.base_url(),
                    user_id,
                )
                .await
            {
                warn!("Failed to clear stale device session mapping: {}", err);
            }
            Ok(None)
        }
    }

    pub async fn current_device_session_id(&self) -> AuthResult<Option<Uuid>> {
        let token_device_session = self.auth_state.with_state(|state| {
            if let AuthState::Authenticated { token, .. } = state {
                token.device_session_id
            } else {
                None
            }
        });
        if token_device_session.is_some() {
            return Ok(token_device_session);
        }

        let current_user = self.get_current_user().await;
        match current_user {
            Some(user) => self.stored_device_session_id_for_user(user.id).await,
            None => Ok(None),
        }
    }

    async fn request_signed_pin_challenge(
        &self,
        device_session_id: Uuid,
        user_id: Uuid,
    ) -> AuthResult<(PinChallengeResponse, Vec<u8>, String)> {
        let signing_key = self
            .load_existing_device_signing_key()
            .await?
            .ok_or_else(|| {
                AuthError::Storage(StorageError::InitFailed(
                    "device signing key unavailable; sign in with password to remember this device again"
                        .to_string(),
                ))
            })?;
        let client_device_id = self.get_or_create_device_id().await?;
        let challenge: PinChallengeResponse = self
            .post_device_json(
                v1::auth::device::PIN_CHALLENGE,
                &PinChallengeRequest {
                    device_id: device_session_id,
                },
                false,
                Some(client_device_id),
            )
            .await?;

        let nonce =
            BASE64.decode(challenge.nonce.as_bytes()).map_err(|_| {
                AuthError::Storage(StorageError::InitFailed(
                    "invalid nonce".to_string(),
                ))
            })?;

        // Build message v1: "Ferrex-PIN-v1" || challenge_id || nonce || user_id
        const CTX: &[u8] = b"Ferrex-PIN-v1";
        let mut msg = Vec::with_capacity(CTX.len() + 16 + nonce.len() + 16);
        msg.extend_from_slice(CTX);
        msg.extend_from_slice(challenge.challenge_id.as_bytes());
        msg.extend_from_slice(&nonce);
        msg.extend_from_slice(user_id.as_bytes());
        let sig: Signature = signing_key.sign(&msg);
        let sig_b64 = BASE64.encode(sig.to_bytes());

        let pin_salt =
            BASE64.decode(challenge.pin_salt.as_bytes()).map_err(|_| {
                AuthError::Internal("invalid PIN salt from server".to_string())
            })?;

        Ok((challenge, pin_salt, sig_b64))
    }

    async fn complete_token_login(
        &self,
        token: AuthToken,
        requested_auto_login: Option<bool>,
    ) -> AuthResult<(User, UserPermissions)> {
        self.api_client.set_token(Some(token.clone())).await;
        let (mut user, permissions) = self.fetch_user_and_permissions().await?;

        if let Some(device_session_id) = token.device_session_id
            && let Err(err) = self
                .remember_server_device_session(user.id, device_session_id)
                .await
        {
            warn!("Failed to remember server device session id: {}", err);
        }

        let mut effective_auto_login = match requested_auto_login {
            Some(enabled) => enabled,
            None => self
                .auth_storage
                .is_auto_login_enabled(&user.id)
                .await
                .unwrap_or(false),
        };

        if let Some(enabled) = requested_auto_login
            && let Err(err) =
                self.auth_storage.set_auto_login(&user.id, enabled).await
        {
            warn!(
                "Failed to update device-local auto-login preference: {}",
                err
            );
            effective_auto_login = false;
        }

        user.preferences.auto_login_enabled = effective_auto_login;

        self.auth_state.authenticate(
            user.clone(),
            token.clone(),
            permissions.clone(),
            self.api_client.base_url().to_string(),
        );

        if effective_auto_login {
            if let Err(err) = self.save_current_auth().await {
                warn!(
                    "Failed to persist remembered authentication; disabling auto-login for this device: {}",
                    err
                );
                if let Err(pref_err) =
                    self.auth_storage.set_auto_login(&user.id, false).await
                {
                    warn!(
                        "Failed to clear auto-login preference after persistence failure: {}",
                        pref_err
                    );
                }
                let mut updated_user = user.clone();
                updated_user.preferences.auto_login_enabled = false;
                self.auth_state.authenticate(
                    updated_user.clone(),
                    token,
                    permissions.clone(),
                    self.api_client.base_url().to_string(),
                );
                user = updated_user;
            }
        } else if let Err(err) = self.clear_keychain().await {
            warn!("Failed to clear unremembered auth cache: {}", err);
        }

        Ok((user, permissions))
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
            Ok((mut user, permissions)) => {
                let device_auto_login = self
                    .auth_storage
                    .is_auto_login_enabled(&user.id)
                    .await
                    .unwrap_or(false);
                user.preferences.auto_login_enabled = device_auto_login;

                if let Some(device_session_id) =
                    stored_auth.token.device_session_id
                    && let Err(err) = self
                        .remember_server_device_session(
                            user.id,
                            device_session_id,
                        )
                        .await
                {
                    warn!(
                        "Failed to refresh stored device session mapping: {}",
                        err
                    );
                }

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
            Ok((mut user, permissions)) => {
                let device_auto_login = self
                    .auth_storage
                    .is_auto_login_enabled(&user.id)
                    .await
                    .unwrap_or(false);
                user.preferences.auto_login_enabled = device_auto_login;

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
        let auth_cache_fingerprint =
            self.device_auth_cache_fingerprint().await?;

        info!(
            "Saving auth token with expires_in: {} seconds",
            auth.token.expires_in
        );

        self.auth_storage
            .save_auth(auth, &auth_cache_fingerprint)
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
        // Get current refresh token and device-session binding
        let current_token = self.auth_state.with_state(|state| match state {
            AuthState::Authenticated { token, .. } => Some(token.clone()),
            _ => None,
        });

        let current_token = current_token
            .ok_or_else(|| AuthError::Token(TokenError::NotAuthenticated))?;
        let refresh_token = current_token.refresh_token.clone();
        let previous_device_session_id = current_token.device_session_id;

        if refresh_token.is_empty() {
            return Err(AuthError::Token(TokenError::RefreshTokenMissing));
        }

        info!("[AuthManager] Attempting to refresh access token");

        // Temporarily disable the refresh callback to avoid infinite recursion
        let mut response: AuthToken = {
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

        if response.device_session_id.is_none() {
            response.device_session_id = previous_device_session_id;
        }
        if response.user_id.is_none() {
            response.user_id = Some(user.id);
        }
        if let Some(device_session_id) = response.device_session_id
            && let Err(err) = self
                .remember_server_device_session(user.id, device_session_id)
                .await
        {
            warn!("Failed to persist refreshed device session id: {}", err);
        }

        // Update auth state with new token
        self.auth_state.authenticate(
            *user,
            response.clone(),
            *permissions,
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
                user: *user,
                server_url,
                permissions: Some(*permissions),
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

    /// Login with username/password without remembering this device.
    pub async fn login(
        &self,
        username: String,
        password: String,
        _server_url: String,
    ) -> AuthResult<(User, UserPermissions)> {
        let result =
            self.authenticate_device(username, password, false).await?;
        Ok((result.user, result.permissions))
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

    /// Change the current user's password.
    pub async fn change_password(
        &self,
        current_password: String,
        new_password: String,
    ) -> AuthResult<()> {
        #[derive(serde::Serialize)]
        struct ChangePasswordRequest {
            current_password: String,
            new_password: String,
        }

        let url = self.api_client.build_url(v1::users::CHANGE_PASSWORD);
        let request =
            self.api_client
                .client
                .put(&url)
                .json(&ChangePasswordRequest {
                    current_password,
                    new_password,
                });
        let request = self.api_client.build_request(request).await;
        let response = request.send().await.map_err(|e| {
            AuthError::Network(NetworkError::RequestFailed(e.to_string()))
        })?;

        match response.status() {
            status if status.is_success() => Ok(()),
            StatusCode::UNAUTHORIZED => {
                Err(AuthError::Network(NetworkError::InvalidCredentials))
            }
            StatusCode::TOO_MANY_REQUESTS => {
                Err(AuthError::Network(NetworkError::RateLimited))
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

    /// Set PIN for current remembered device session.
    pub async fn set_device_pin(&self, pin: String) -> AuthResult<()> {
        let user = self
            .get_current_user()
            .await
            .ok_or(AuthError::NotAuthenticated)?;
        let device_session_id = self
            .current_device_session_id()
            .await?
            .ok_or(AuthError::Device(DeviceError::NotRegistered))?;
        let (challenge, pin_salt, device_signature) = self
            .request_signed_pin_challenge(device_session_id, user.id)
            .await?;
        let client_proof = Self::derive_client_pin_proof(&pin, &pin_salt)?;
        let request = SetPinRequest {
            device_id: device_session_id,
            client_proof,
            challenge_id: challenge.challenge_id,
            device_signature,
        };

        let client_device_id = self.get_or_create_device_id().await?;
        self.post_device_json_no_data(
            v1::auth::device::SET_PIN,
            &request,
            true,
            Some(client_device_id),
        )
        .await?;

        self.cache_device_status(
            user.id,
            &DeviceAuthStatus {
                device_registered: true,
                has_pin: true,
                remaining_attempts: None,
                ..DeviceAuthStatus::default()
            },
        )
        .await;

        Ok(())
    }

    /// Change PIN for current remembered device session.
    pub async fn change_device_pin(
        &self,
        current_pin: String,
        new_pin: String,
    ) -> AuthResult<()> {
        #[derive(serde::Serialize)]
        struct ChangePinRequest {
            /// Server-side device session id.
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
        let device_session_id = self
            .current_device_session_id()
            .await?
            .ok_or(AuthError::Device(DeviceError::NotRegistered))?;
        let (challenge, pin_salt, device_signature) = self
            .request_signed_pin_challenge(device_session_id, user.id)
            .await?;
        let current_proof =
            Self::derive_client_pin_proof(&current_pin, &pin_salt)?;
        let new_proof = Self::derive_client_pin_proof(&new_pin, &pin_salt)?;

        let request = ChangePinRequest {
            device_id: device_session_id,
            current_proof,
            new_proof,
            challenge_id: challenge.challenge_id,
            device_signature,
        };

        let client_device_id = self.get_or_create_device_id().await?;
        self.post_device_json_no_data(
            v1::auth::device::CHANGE_PIN,
            &request,
            true,
            Some(client_device_id),
        )
        .await?;

        self.cache_device_status(
            user.id,
            &DeviceAuthStatus {
                device_registered: true,
                has_pin: true,
                remaining_attempts: None,
                ..DeviceAuthStatus::default()
            },
        )
        .await;

        Ok(())
    }

    /// Remove PIN for current remembered device session.
    pub async fn remove_device_pin(
        &self,
        current_pin: String,
    ) -> AuthResult<()> {
        #[derive(serde::Serialize)]
        struct RemovePinRequest {
            /// Server-side device session id.
            device_id: Uuid,
            current_proof: String,
            challenge_id: Uuid,
            device_signature: String,
        }

        let user = self
            .get_current_user()
            .await
            .ok_or(AuthError::NotAuthenticated)?;
        let device_session_id = self
            .current_device_session_id()
            .await?
            .ok_or(AuthError::Device(DeviceError::NotRegistered))?;
        let (challenge, pin_salt, device_signature) = self
            .request_signed_pin_challenge(device_session_id, user.id)
            .await?;
        let current_proof =
            Self::derive_client_pin_proof(&current_pin, &pin_salt)?;

        let request = RemovePinRequest {
            device_id: device_session_id,
            current_proof,
            challenge_id: challenge.challenge_id,
            device_signature,
        };

        let client_device_id = self.get_or_create_device_id().await?;
        self.post_device_json_no_data(
            v1::auth::device::REMOVE_PIN,
            &request,
            true,
            Some(client_device_id),
        )
        .await?;

        self.cache_device_status(
            user.id,
            &DeviceAuthStatus {
                device_registered: true,
                has_pin: false,
                remaining_attempts: None,
                ..DeviceAuthStatus::default()
            },
        )
        .await;

        Ok(())
    }

    /// Authenticate using username/password and optionally remember this device.
    pub async fn authenticate_device(
        &self,
        username: String,
        password: String,
        remember_device: bool,
    ) -> AuthResult<PlayerAuthResult> {
        let identity = self.get_or_create_device_identity().await?;
        let device_info = Self::device_info_for_identity(&identity);
        let signing_key = if remember_device {
            Some(self.ensure_device_signing_key().await?)
        } else {
            None
        };
        let (device_public_key, device_key_alg) = signing_key
            .as_ref()
            .map(|key| {
                (
                    Some(BASE64.encode(key.verifying_key().to_bytes())),
                    Some("ed25519".to_string()),
                )
            })
            .unwrap_or((None, None));

        let request = DeviceLoginRequest {
            username,
            password,
            device_info: Some(device_info),
            remember_device,
            device_public_key,
            device_key_alg,
        };

        let response: DeviceAuthResponse = self
            .post_device_json(
                v1::auth::device::LOGIN,
                &request,
                false,
                Some(identity.id),
            )
            .await?;

        let device_has_pin = response
            .device_registration
            .as_ref()
            .map(|registration| registration.pin_configured)
            .unwrap_or(!response.requires_pin_setup);
        {
            let mut guard = self.device_trust_expires_at.lock().await;
            *guard = if remember_device {
                response
                    .device_registration
                    .as_ref()
                    .and_then(|registration| {
                        registration.expires_at.as_ref().cloned()
                    })
            } else {
                None
            };
        }
        let (user, permissions) = self
            .complete_token_login(response.token, Some(remember_device))
            .await?;

        let summary = crate::domains::auth::dto::UserListItemDto {
            id: user.id,
            username: user.username.clone(),
            display_name: user.display_name.clone(),
            avatar_url: user.avatar_url.clone(),
            has_pin: device_has_pin,
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
            device_has_pin,
        })
    }

    /// Authenticate using a stored PIN for the selected user.
    pub async fn authenticate_pin(
        &self,
        user_id: Uuid,
        pin: String,
    ) -> AuthResult<PlayerAuthResult> {
        let device_session_id = self
            .stored_device_session_id_for_user(user_id)
            .await?
            .ok_or(AuthError::Device(DeviceError::NotRegistered))?;
        let (challenge, pin_salt, device_signature) = self
            .request_signed_pin_challenge(device_session_id, user_id)
            .await?;
        let client_proof = Self::derive_client_pin_proof(&pin, &pin_salt)?;
        let request = PinLoginRequest {
            device_id: device_session_id,
            client_proof,
            challenge_id: challenge.challenge_id,
            device_signature,
        };

        let client_device_id = self.get_or_create_device_id().await?;
        let response: DeviceAuthResponse = self
            .post_device_json(
                v1::auth::device::PIN_LOGIN,
                &request,
                false,
                Some(client_device_id),
            )
            .await?;
        let (user, permissions) =
            self.complete_token_login(response.token, None).await?;

        let summary = crate::domains::auth::dto::UserListItemDto {
            id: user.id,
            username: user.username.clone(),
            display_name: user.display_name.clone(),
            avatar_url: user.avatar_url.clone(),
            has_pin: true,
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
            warn!("Failed to persist user summary after PIN login: {}", e);
        }

        Ok(PlayerAuthResult {
            user,
            permissions,
            device_has_pin: true,
        })
    }

    /// Check if user has PIN on this device.
    pub async fn check_device_auth(
        &self,
        user_id: Uuid,
    ) -> AuthResult<DeviceAuthStatus> {
        let cached_status = self.check_cached_device_status(user_id).await;
        let stored_device_session_id =
            self.stored_device_session_id_for_user(user_id).await?;
        let has_usable_signing_key =
            self.load_existing_device_signing_key().await?.is_some();
        if stored_device_session_id.is_some()
            && !has_usable_signing_key
            && let Err(err) = self
                .auth_storage
                .clear_device_session_for_server(
                    self.api_client.base_url(),
                    user_id,
                )
                .await
        {
            warn!(
                "Failed to clear device session after missing signing key: {}",
                err
            );
        }

        // If not authenticated, avoid online probing. Only advertise PIN when
        // both the server device session id and local key material are present;
        // otherwise fall back to password without trapping the user in a broken
        // PIN screen.
        let is_authed = self.auth_state.with_state(|state| {
            matches!(state, AuthState::Authenticated { .. })
        });
        if !is_authed {
            if let Some(mut status) = cached_status
                && stored_device_session_id.is_some()
                && has_usable_signing_key
            {
                status.device_registered = true;
                log::info!(
                    "[Auth] Using cached device status for user {}: registered={}, has_pin={}",
                    user_id,
                    status.device_registered,
                    status.has_pin
                );
                return Ok(status);
            }

            log::info!(
                "[Auth] Device session or key unavailable for user {}; requiring password login",
                user_id
            );
            return Ok(DeviceAuthStatus {
                device_registered: false,
                has_pin: false,
                remaining_attempts: None,
                pin_policy: PinPolicyResponse::default(),
                device_trust_policy: DeviceTrustPolicyResponse::default(),
            });
        }

        let current_session_for_user = match self.get_current_user().await {
            Some(current_user) if current_user.id == user_id => {
                self.current_device_session_id().await?
            }
            _ => None,
        };
        let device_session_id = current_session_for_user
            .or(stored_device_session_id)
            .ok_or(AuthError::Device(DeviceError::NotRegistered))?;

        log::info!(
            "[Auth] Checking device status online for user {} on server device session {}",
            user_id,
            device_session_id
        );

        let status_path = format!(
            "{}?device_id={}",
            v1::auth::device::STATUS,
            device_session_id
        );

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

        if status.device_registered {
            self.cache_device_status(user_id, &status).await;
        } else if let Err(err) = self
            .auth_storage
            .clear_device_session_for_server(
                self.api_client.base_url(),
                user_id,
            )
            .await
        {
            warn!("Failed to clear unregistered device session: {}", err);
        }

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
            && let Some(u) = users.into_iter().find(|u| u.id == user_id)
        {
            return Some(DeviceAuthStatus {
                device_registered: true,
                has_pin: u.has_pin,
                remaining_attempts: None,
                pin_policy: PinPolicyResponse::default(),
                device_trust_policy: DeviceTrustPolicyResponse::default(),
            });
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
            if updated
                && let Err(e) = self
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

    /// Get or create device ID
    async fn get_or_create_device_id(&self) -> AuthResult<Uuid> {
        if let Some(id) = self.device_id.get() {
            return Ok(*id);
        }

        Ok(self.get_or_create_device_identity().await?.id)
    }

    /// Expose the current device identifier to callers that need to identify themselves
    pub async fn current_device_id(&self) -> AuthResult<Uuid> {
        self.get_or_create_device_id().await
    }

    /// Get current authenticated user
    pub async fn get_current_user(&self) -> Option<User> {
        self.auth_state.with_state(|state| match state {
            AuthState::Authenticated { user, .. } => Some(*user.clone()),
            _ => None,
        })
    }

    /// Get current user permissions
    pub async fn get_current_permissions(&self) -> Option<UserPermissions> {
        self.auth_state.with_state(|state| match state {
            AuthState::Authenticated { permissions, .. } => {
                Some(*permissions.clone())
            }
            _ => None,
        })
    }

    /// Check if auto-login is enabled for current user
    pub async fn is_auto_login_enabled(&self) -> bool {
        if let Some(user) = self.get_current_user().await {
            self.auth_storage
                .is_auto_login_enabled(&user.id)
                .await
                .unwrap_or(false)
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
            // Update server-side preference so settings stay in sync across devices.
            // DeviceOnly intentionally avoids mutating the legacy user-wide default.
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
        }

        // Update in-memory auth state with the effective device-local preference
        // so UI and the stored snapshot reflect the user's choice without
        // requiring a server-wide preference mutation.
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
                *permissions,
                server_url,
            );
        }

        // Persist the updated preference to storage so auto-login reflects user intent.
        if enabled {
            if let Err(err) = self.save_current_auth().await {
                warn!("Failed to persist auto-login preference: {}", err);
            }
        } else if let Err(err) = self.clear_keychain().await {
            warn!(
                "Failed to clear auth cache after disabling auto-login: {}",
                err
            );
        }

        Ok(())
    }

    /// Backwards-compatible helper that applies the user-default scope.
    pub async fn set_auto_login(&self, enabled: bool) -> AuthResult<()> {
        self.set_auto_login_scope(enabled, AutoLoginScope::UserDefault)
            .await
    }

    async fn get_known_device_users(
        &self,
    ) -> AuthResult<Option<Vec<UserListItemDto>>> {
        let identity = self.get_or_create_device_identity().await?;
        let device_info = Self::device_info_for_identity(&identity);
        let response: KnownDeviceProfilesResponse = self
            .post_device_json(
                v1::auth::device::KNOWN_USERS,
                &KnownDeviceProfilesRequest {
                    device_info: Some(device_info),
                },
                false,
                Some(identity.id),
            )
            .await?;

        if !response.known_device || response.users.is_empty() {
            return Ok(None);
        }

        let users = response
            .users
            .into_iter()
            .map(|user| UserListItemDto {
                id: user.id,
                username: user.username,
                display_name: user.display_name,
                avatar_url: user.avatar_url,
                has_pin: user.has_pin,
                last_login: None,
            })
            .collect::<Vec<_>>();

        if let Err(e) = self
            .auth_storage
            .save_user_summaries_for_server(self.api_client.base_url(), &users)
            .await
        {
            warn!("Failed to save known-device user summaries: {}", e);
        }

        Ok(Some(users))
    }

    /// Get all users (for user selection screen)
    ///
    /// This method sends stable device info to get appropriate user information
    /// based on whether the backend recognizes this device.
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
            match self.get_known_device_users().await {
                Ok(Some(users)) => return Ok(users),
                Ok(None) => {}
                Err(err) => {
                    warn!(
                        "Known-device user lookup failed; falling back to setup/cache flow: {}",
                        err
                    );
                }
            }

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

    /// Clear local auth, remembered-device, and fallback caches for recovery.
    pub async fn reset_local_auth_state(&self) -> AuthResult<()> {
        self.api_client.set_token(None).await;
        self.auth_state.logout();
        {
            let mut guard = self.device_trust_expires_at.lock().await;
            *guard = None;
        }

        let base_url = self.api_client.base_url().to_string();
        let mut errors = Vec::new();

        if let Err(err) = self.clear_keychain().await {
            errors.push(format!("auth cache: {}", err));
        }
        if let Err(err) = self
            .auth_storage
            .clear_user_summaries_for_server(&base_url)
            .await
        {
            errors.push(format!("user cache: {}", err));
        }
        if let Err(err) = self
            .auth_storage
            .clear_device_sessions_for_server(&base_url)
            .await
        {
            errors.push(format!("device sessions: {}", err));
        }
        if let Err(err) = self.auth_storage.clear_auto_login_preferences().await
        {
            errors.push(format!("auto-login preferences: {}", err));
        }
        if let Err(err) = self.auth_storage.clear_device_key().await {
            errors.push(format!("device key: {}", err));
        }
        if let Err(err) = self.auth_storage.disable_admin_pin_unlock().await {
            errors.push(format!("admin PIN unlock: {}", err));
        }
        if let Err(err) = DeviceIdentity::reset().await {
            errors.push(format!("device identity: {}", err));
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(AuthError::Storage(StorageError::WriteFailed(
                std::io::Error::other(format!(
                    "Failed to clear all local auth state: {}",
                    errors.join(", ")
                )),
            )))
        }
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

    /// Enable admin PIN unlock.
    ///
    /// Server policy currently disables admin PIN unlock: PIN sessions are
    /// playback-scoped and admin operations require full password auth.
    pub async fn enable_admin_pin_unlock(&self) -> AuthResult<()> {
        Err(AuthError::Internal(
            "Admin PIN unlock is disabled; use password authentication for admin actions".to_string(),
        ))
    }

    /// Disable admin PIN unlock.
    pub async fn disable_admin_pin_unlock(&self) -> AuthResult<()> {
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

    #[derive(Debug, Clone)]
    struct CapturedRequest {
        path: String,
        headers: std::collections::HashMap<String, String>,
        body: serde_json::Value,
    }

    #[derive(Debug, Clone)]
    struct DeviceAuthMock {
        user: User,
        permissions: UserPermissions,
        session_id: Uuid,
        device_session_id: Uuid,
        challenge_id: Uuid,
        challenge_nonce: Vec<u8>,
        pin_salt: Vec<u8>,
        login_pin_configured: bool,
    }

    fn test_user(user_id: Uuid) -> User {
        User {
            id: user_id,
            username: "alice".to_string(),
            display_name: "Alice".to_string(),
            avatar_url: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_login: None,
            is_active: true,
            email: None,
            preferences: Default::default(),
        }
    }

    fn test_permissions(user_id: Uuid) -> UserPermissions {
        UserPermissions {
            user_id,
            roles: Vec::new(),
            permissions: std::collections::HashMap::new(),
            permission_details: None,
        }
    }

    async fn read_http_request(
        socket: &mut tokio::net::TcpStream,
    ) -> CapturedRequest {
        let mut buffer = Vec::new();
        let mut temp = [0u8; 1024];
        loop {
            let n = socket.read(&mut temp).await.expect("read request");
            if n == 0 {
                break;
            }
            buffer.extend_from_slice(&temp[..n]);
            if let Some(header_end) = find_header_end(&buffer) {
                let headers = String::from_utf8_lossy(&buffer[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                if buffer.len() >= header_end + 4 + content_length {
                    break;
                }
            }
        }

        let header_end = find_header_end(&buffer).expect("headers complete");
        let header_text = String::from_utf8_lossy(&buffer[..header_end]);
        let mut lines = header_text.lines();
        let request_line = lines.next().expect("request line");
        let path = request_line
            .split_whitespace()
            .nth(1)
            .expect("request path")
            .to_string();
        let headers = lines
            .filter_map(|line| {
                let (name, value) = line.split_once(':')?;
                Some((
                    name.trim().to_ascii_lowercase(),
                    value.trim().to_string(),
                ))
            })
            .collect::<std::collections::HashMap<_, _>>();
        let body_bytes = &buffer[header_end + 4..];
        let body = if body_bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(body_bytes)
                .unwrap_or(serde_json::Value::Null)
        };

        CapturedRequest {
            path,
            headers,
            body,
        }
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }

    async fn write_json_response(
        socket: &mut tokio::net::TcpStream,
        status: &str,
        body: serde_json::Value,
    ) {
        let body = serde_json::to_vec(&body).expect("serialize response");
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("write response headers");
        socket.write_all(&body).await.expect("write response body");
        let _ = socket.shutdown().await;
    }

    fn device_auth_token_json(mock: &DeviceAuthMock) -> serde_json::Value {
        serde_json::json!({
            "access_token": "access-token",
            "session_token": "access-token",
            "refresh_token": "refresh-token",
            "expires_in": 900,
            "session_id": mock.session_id,
            "device_session_id": mock.device_session_id,
            "user_id": mock.user.id,
            "scope": "full",
            "device_registration": {
                "id": mock.device_session_id,
                "user_id": mock.user.id,
                "device_id": mock.user.id,
                "device_name": "Test Device",
                "platform": "linux",
                "app_version": "1.0.0",
                "pin_configured": mock.login_pin_configured,
                "registered_at": Utc::now(),
                "last_used_at": Utc::now(),
                "expires_at": Utc::now(),
                "revoked": false,
                "revoked_by": null,
                "revoked_at": null
            },
            "requires_pin_setup": !mock.login_pin_configured
        })
    }

    async fn spawn_device_auth_mock(
        mock: DeviceAuthMock,
        expected_requests: usize,
    ) -> (
        String,
        std::sync::Arc<tokio::sync::Mutex<Vec<CapturedRequest>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind device auth mock");
        let addr = listener.local_addr().unwrap();
        let captured = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let captured_for_task = std::sync::Arc::clone(&captured);
        let handle = tokio::spawn(async move {
            for _ in 0..expected_requests {
                let Ok((mut socket, _peer)) = listener.accept().await else {
                    break;
                };
                let request = read_http_request(&mut socket).await;
                let path_without_query = request
                    .path
                    .split('?')
                    .next()
                    .unwrap_or(request.path.as_str())
                    .to_string();
                captured_for_task.lock().await.push(request);

                let body = match path_without_query.as_str() {
                    v1::auth::device::LOGIN => serde_json::json!({
                        "status": "success",
                        "data": device_auth_token_json(&mock)
                    }),
                    v1::auth::device::PIN_CHALLENGE => serde_json::json!({
                        "status": "success",
                        "data": {
                            "challenge_id": mock.challenge_id,
                            "nonce": BASE64.encode(&mock.challenge_nonce),
                            "expires_in_secs": 120,
                            "pin_salt": BASE64.encode(&mock.pin_salt)
                        }
                    }),
                    v1::auth::device::PIN_LOGIN => serde_json::json!({
                        "status": "success",
                        "data": device_auth_token_json(&mock)
                    }),
                    v1::users::CURRENT => serde_json::to_value(
                        ApiResponse::success(mock.user.clone()),
                    )
                    .expect("user response"),
                    v1::roles::MY_PERMISSIONS => serde_json::to_value(
                        ApiResponse::success(mock.permissions.clone()),
                    )
                    .expect("permissions response"),
                    _ => serde_json::json!({
                        "status": "error",
                        "error": format!("unexpected path {path_without_query}")
                    }),
                };
                let status = if body["status"] == "error" {
                    "404 Not Found"
                } else {
                    "200 OK"
                };
                write_json_response(&mut socket, status, body).await;
            }
        });

        (format!("http://{}", addr), captured, handle)
    }

    fn manager_with_temp_storage(
        base_url: String,
        storage: AuthStorage,
        client_device_id: Uuid,
    ) -> AuthManager {
        let client = ApiClient::new(base_url);
        let mut manager = AuthManager::new(client);
        manager.auth_storage = Arc::new(storage);
        let _ = manager.device_id.set(client_device_id);
        let _ = manager
            .device_fingerprint
            .set("test-hardware-fingerprint".to_string());
        manager
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

    #[tokio::test]
    async fn device_login_sends_device_contract_and_stores_session() {
        let user_id = Uuid::now_v7();
        let client_device_id = Uuid::now_v7();
        let device_session_id = Uuid::now_v7();
        let mock = DeviceAuthMock {
            user: test_user(user_id),
            permissions: test_permissions(user_id),
            session_id: Uuid::now_v7(),
            device_session_id,
            challenge_id: Uuid::now_v7(),
            challenge_nonce: vec![1, 2, 3, 4],
            pin_salt: vec![5; 16],
            login_pin_configured: true,
        };
        let (base_url, captured, handle) =
            spawn_device_auth_mock(mock, 3).await;
        let tmp = TempDir::new().unwrap();
        let storage =
            AuthStorage::with_cache_path(tmp.path().join(AUTH_CACHE_FILE));
        let manager = manager_with_temp_storage(
            base_url.clone(),
            storage,
            client_device_id,
        );

        let result = manager
            .authenticate_device(
                "alice".to_string(),
                "correct horse".to_string(),
                true,
            )
            .await
            .expect("device login succeeds");
        handle.await.expect("mock server completes");

        assert_eq!(result.user.id, user_id);
        assert!(result.device_has_pin);
        assert_eq!(
            manager.current_device_session_id().await.unwrap(),
            Some(device_session_id)
        );
        assert!(
            manager
                .auth_storage
                .is_auto_login_enabled(&user_id)
                .await
                .unwrap(),
            "remember=true stores device-local auto-login only after login"
        );

        let stored_session = manager
            .auth_storage
            .load_device_session_for_server(&base_url, user_id)
            .await
            .unwrap()
            .expect("device session stored");
        assert_eq!(stored_session.client_device_id, client_device_id);
        assert_eq!(stored_session.device_session_id, device_session_id);

        let requests = captured.lock().await;
        let login_request = requests
            .iter()
            .find(|request| request.path == v1::auth::device::LOGIN)
            .expect("login request captured");
        assert_eq!(
            login_request.headers.get("x-device-id"),
            Some(&client_device_id.to_string())
        );
        assert_eq!(login_request.body["username"], "alice");
        assert_eq!(login_request.body["remember_device"], true);
        assert_eq!(
            login_request.body["device_info"]["device_id"],
            client_device_id.to_string()
        );
        assert_eq!(
            login_request.body["device_info"]["hardware_id"],
            "test-hardware-fingerprint"
        );
        assert_eq!(login_request.body["device_key_alg"], "ed25519");
        let public_key = login_request.body["device_public_key"]
            .as_str()
            .expect("public key present");
        assert_eq!(BASE64.decode(public_key).unwrap().len(), 32);
    }

    #[tokio::test]
    async fn remember_false_does_not_persist_auto_login_or_public_key() {
        let user_id = Uuid::now_v7();
        let client_device_id = Uuid::now_v7();
        let mock = DeviceAuthMock {
            user: test_user(user_id),
            permissions: test_permissions(user_id),
            session_id: Uuid::now_v7(),
            device_session_id: Uuid::now_v7(),
            challenge_id: Uuid::now_v7(),
            challenge_nonce: vec![1, 2, 3, 4],
            pin_salt: vec![5; 16],
            login_pin_configured: false,
        };
        let (base_url, captured, handle) =
            spawn_device_auth_mock(mock, 3).await;
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join(AUTH_CACHE_FILE);
        let storage = AuthStorage::with_cache_path(cache_path.clone());
        let manager =
            manager_with_temp_storage(base_url, storage, client_device_id);

        manager
            .authenticate_device(
                "alice".to_string(),
                "password".to_string(),
                false,
            )
            .await
            .expect("unremembered device login succeeds");
        handle.await.expect("mock server completes");

        assert!(
            !manager
                .auth_storage
                .is_auto_login_enabled(&user_id)
                .await
                .unwrap()
        );
        assert!(
            !cache_path.exists(),
            "remember=false must not persist refresh auth cache"
        );

        let requests = captured.lock().await;
        let login_request = requests
            .iter()
            .find(|request| request.path == v1::auth::device::LOGIN)
            .expect("login request captured");
        assert_eq!(login_request.body["remember_device"], false);
        assert!(login_request.body.get("device_public_key").is_none());
        assert!(login_request.body.get("device_key_alg").is_none());
    }

    #[tokio::test]
    async fn pin_login_uses_server_device_session_challenge_and_signature() {
        let user_id = Uuid::now_v7();
        let client_device_id = Uuid::now_v7();
        let device_session_id = Uuid::now_v7();
        let challenge_id = Uuid::now_v7();
        let mock = DeviceAuthMock {
            user: test_user(user_id),
            permissions: test_permissions(user_id),
            session_id: Uuid::now_v7(),
            device_session_id,
            challenge_id,
            challenge_nonce: vec![9, 8, 7, 6],
            pin_salt: vec![0xAB; 16],
            login_pin_configured: true,
        };
        let (base_url, captured, handle) =
            spawn_device_auth_mock(mock, 4).await;
        let tmp = TempDir::new().unwrap();
        let storage =
            AuthStorage::with_cache_path(tmp.path().join(AUTH_CACHE_FILE));
        let manager = manager_with_temp_storage(
            base_url.clone(),
            storage,
            client_device_id,
        );
        manager.ensure_device_signing_key().await.unwrap();
        manager
            .auth_storage
            .save_device_session_for_server(
                &base_url,
                user_id,
                client_device_id,
                device_session_id,
            )
            .await
            .unwrap();

        let result = manager
            .authenticate_pin(user_id, "1234".to_string())
            .await
            .expect("pin login succeeds");
        handle.await.expect("mock server completes");

        assert_eq!(result.user.id, user_id);
        assert!(result.device_has_pin);

        let requests = captured.lock().await;
        let challenge_request = requests
            .iter()
            .find(|request| request.path == v1::auth::device::PIN_CHALLENGE)
            .expect("challenge request captured");
        assert_eq!(
            challenge_request.body["device_id"],
            device_session_id.to_string()
        );
        assert_eq!(
            challenge_request.headers.get("x-device-id"),
            Some(&client_device_id.to_string())
        );

        let pin_request = requests
            .iter()
            .find(|request| request.path == v1::auth::device::PIN_LOGIN)
            .expect("pin login request captured");
        assert_eq!(
            pin_request.body["device_id"],
            device_session_id.to_string()
        );
        assert_eq!(pin_request.body["challenge_id"], challenge_id.to_string());
        assert_ne!(pin_request.body["client_proof"], "1234");
        assert!(
            pin_request.body["client_proof"]
                .as_str()
                .unwrap()
                .starts_with("$argon2id$")
        );
        assert_eq!(
            BASE64
                .decode(pin_request.body["device_signature"].as_str().unwrap())
                .unwrap()
                .len(),
            64
        );
    }

    #[tokio::test]
    async fn missing_device_key_hides_pin_and_clears_stale_session() {
        let user_id = Uuid::now_v7();
        let client_device_id = Uuid::now_v7();
        let base_url = "http://127.0.0.1:9".to_string();
        let tmp = TempDir::new().unwrap();
        let storage =
            AuthStorage::with_cache_path(tmp.path().join(AUTH_CACHE_FILE));
        storage
            .save_user_summaries_for_server(
                &base_url,
                &[UserListItemDto {
                    id: user_id,
                    username: "alice".into(),
                    display_name: "Alice".into(),
                    avatar_url: None,
                    has_pin: true,
                    last_login: Some(Utc::now()),
                }],
            )
            .await
            .unwrap();
        storage
            .save_device_session_for_server(
                &base_url,
                user_id,
                client_device_id,
                Uuid::now_v7(),
            )
            .await
            .unwrap();
        let manager = manager_with_temp_storage(
            base_url.clone(),
            storage,
            client_device_id,
        );

        let status = manager
            .check_device_auth(user_id)
            .await
            .expect("status fallback succeeds");
        assert!(!status.device_registered);
        assert!(!status.has_pin);
        assert!(
            manager
                .auth_storage
                .load_device_session_for_server(&base_url, user_id)
                .await
                .unwrap()
                .is_none(),
            "missing/corrupt key should clear stale server session mapping"
        );
    }
}
