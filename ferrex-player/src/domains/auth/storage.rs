//! Encrypted local storage for authentication data
//!
//! This module provides device-bound encrypted storage for authentication tokens
//! without requiring OS-level secret services or keychains.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use argon2::password_hash::SaltString;
use argon2::{Argon2, Params};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

use crate::domains::auth::dto::UserListItemDto;
use ferrex_core::player_prelude::{AuthToken, User, UserPermissions};
use uuid::Uuid;

pub(crate) const AUTH_CACHE_FILE: &str = "auth_cache.enc";
const NONCE_SIZE: usize = 12;
const KEY_DERIVATION_SALT: &str = "ferrex-auth-v2";
const ARGON2_MEM_COST: u32 = 64 * 1024; // 64 MB
const ARGON2_TIME_COST: u32 = 3;
const ARGON2_PARALLELISM: u32 = 4;

/// Encrypted auth data with metadata
#[derive(Debug, Serialize, Deserialize)]
struct EncryptedAuthData {
    /// Base64 encoded nonce
    nonce: String,
    /// Base64 encoded encrypted data
    ciphertext: String,
    /// When this data was encrypted
    encrypted_at: DateTime<Utc>,
    /// Version for future compatibility
    version: u32,
    /// Salt used for key derivation (added in v2)
    #[serde(skip_serializing_if = "Option::is_none")]
    salt: Option<String>,
}

/// Stored authentication data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredAuth {
    pub token: AuthToken,
    pub user: User,
    pub server_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<UserPermissions>,
    /// When this auth data was stored
    pub stored_at: DateTime<Utc>,
    /// Device trust expiry (30 days from initial login)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_trust_expires_at: Option<DateTime<Utc>>,
    /// Refresh token for getting new access tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
}

/// Local encrypted storage for authentication data
#[derive(Debug)]
pub struct AuthStorage {
    /// Path to the encrypted auth file
    cache_path: PathBuf,
}

impl AuthStorage {
    /// Create a new auth storage instance
    pub fn new() -> Result<Self> {
        // Use a distinct app name for demo runs so that demo and prod
        // keychain caches are isolated and never conflict.
        let app_name = if Self::is_demo_mode_enabled() {
            "ferrex-player-demo"
        } else {
            "ferrex-player"
        };

        let proj_dirs =
            ProjectDirs::from("", "ferrex", app_name).ok_or_else(|| {
                anyhow::anyhow!("Unable to determine config directory")
            })?;

        let cache_path = proj_dirs.data_dir().join(AUTH_CACHE_FILE);

        let storage = Self { cache_path };
        // Best-effort cleanup for a legacy, non-server-scoped user cache file that older builds
        // may have written. Keeping it risks presenting users from a previous server instance
        // after a reset. Current code only uses server-scoped caches, so remove the legacy file.
        if let Err(e) = storage.cleanup_legacy_user_cache() {
            log::debug!(
                "[AuthStorage] Legacy user cache cleanup skipped: {}",
                e
            );
        }
        Ok(storage)
    }

    /// Detect whether demo mode is enabled using the same environment/CLI
    /// checks used by the runtime bootstrap.
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
        if Self::is_demo_mode_enabled_env() {
            return true;
        }
        // CLI fallback
        std::env::args().any(|arg| arg == "--demo")
    }

    fn device_key_path(&self) -> PathBuf {
        self.cache_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("device_key.enc")
    }

    /// Save device private key encrypted with a random wrapping key (not derived from fingerprint)
    pub async fn save_device_key(&self, private_key: &[u8]) -> Result<()> {
        // Create or load wrapping key from filesystem
        let wrap_path = self
            .cache_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("device_key_wrap.key");
        let wrap_key = if wrap_path.exists() {
            tokio::fs::read(&wrap_path).await?
        } else {
            let mut key = [0u8; 32];
            getrandom::getrandom(&mut key)
                .map_err(|e| anyhow::anyhow!("rng failed: {}", e))?;
            if let Some(parent) = wrap_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(&wrap_path, &key).await?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms =
                    tokio::fs::metadata(&wrap_path).await?.permissions();
                perms.set_mode(0o600);
                tokio::fs::set_permissions(&wrap_path, perms).await?;
            }
            key.to_vec()
        };

        // Derive encryption key from wrap key via HKDF-SHA256 (no Argon2 for random keys)
        let hk = hkdf::Hkdf::<sha2::Sha256>::new(None, &wrap_key);
        let mut okm = [0u8; 32];
        hk.expand(b"ferrex-device-key-v1", &mut okm)
            .map_err(|_| anyhow::anyhow!("HKDF expand failed"))?;
        let key = *Key::<Aes256Gcm>::from_slice(&okm);
        let cipher = Aes256Gcm::new(&key);
        let nonce_bytes = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(&nonce_bytes, private_key)
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;
        let encrypted = EncryptedAuthData {
            nonce: BASE64.encode(nonce_bytes),
            ciphertext: BASE64.encode(ciphertext),
            encrypted_at: Utc::now(),
            version: 2,
            salt: None,
        };
        let json = serde_json::to_string_pretty(&encrypted)?;
        let path = self.device_key_path();
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&path, json).await?;
        Ok(())
    }

    /// Load device private key if present
    pub async fn load_device_key(&self) -> Result<Option<Vec<u8>>> {
        let path = self.device_key_path();
        if !path.exists() {
            return Ok(None);
        }
        let data = tokio::fs::read_to_string(&path).await?;
        let encrypted: EncryptedAuthData = serde_json::from_str(&data)?;
        // Load or create wrap key
        let wrap_path = self
            .cache_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("device_key_wrap.key");
        let wrap_key = if wrap_path.exists() {
            tokio::fs::read(&wrap_path).await?
        } else {
            return Ok(None);
        };
        // Derive encryption key
        let key = if let Some(salt_str) = encrypted.salt.as_ref() {
            // Backward compatibility: older format used Argon2id + random salt
            let salt = SaltString::from_b64(salt_str)
                .map_err(|e| anyhow::anyhow!("Invalid salt format: {}", e))?;
            let params = Params::new(
                ARGON2_MEM_COST,
                ARGON2_TIME_COST,
                ARGON2_PARALLELISM,
                Some(32),
            )
            .map_err(|e| anyhow::anyhow!("Invalid Argon2 parameters: {}", e))?;
            let argon2 = Argon2::new(
                argon2::Algorithm::Argon2id,
                argon2::Version::V0x13,
                params,
            );
            let mut out = [0u8; 32];
            argon2
                .hash_password_into(
                    &wrap_key,
                    salt.as_str().as_bytes(),
                    &mut out,
                )
                .map_err(|e| anyhow::anyhow!("Key derivation failed: {}", e))?;
            *Key::<Aes256Gcm>::from_slice(&out)
        } else {
            // New format: HKDF-SHA256
            let hk = hkdf::Hkdf::<sha2::Sha256>::new(None, &wrap_key);
            let mut okm = [0u8; 32];
            hk.expand(b"ferrex-device-key-v1", &mut okm)
                .map_err(|_| anyhow::anyhow!("HKDF expand failed"))?;
            *Key::<Aes256Gcm>::from_slice(&okm)
        };
        let cipher = Aes256Gcm::new(&key);
        let nonce_bytes = BASE64.decode(encrypted.nonce)?;
        let ciphertext = BASE64.decode(encrypted.ciphertext)?;
        let plaintext = cipher
            .decrypt(Nonce::from_slice(&nonce_bytes), ciphertext.as_ref())
            .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;
        Ok(Some(plaintext))
    }

    /// Get path to auth cache file
    pub fn cache_path(&self) -> &PathBuf {
        &self.cache_path
    }

    pub fn with_cache_path(cache_path: PathBuf) -> Self {
        Self { cache_path }
    }

    fn users_cache_path(&self) -> PathBuf {
        self.cache_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("users_cache.json")
    }

    /// Remove legacy global user cache file if present (non-server-scoped), to prevent
    /// stale user lists from previous server instances being shown.
    fn cleanup_legacy_user_cache(&self) -> anyhow::Result<()> {
        let legacy = self.users_cache_path();
        if legacy.exists() {
            std::fs::remove_file(&legacy)?;
            log::warn!(
                "[AuthStorage] Removed legacy global user cache at {:?}",
                legacy
            );
        }
        Ok(())
    }

    fn server_hash(base_url: &str) -> String {
        let normalized = base_url.trim().trim_end_matches('/').to_lowercase();
        use sha2::{Digest, Sha256};
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

    fn users_cache_path_for_server(&self, base_url: &str) -> PathBuf {
        let server_dir = self
            .cache_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("servers")
            .join(Self::server_hash(base_url));
        server_dir.join("users_cache.json")
    }

    /// Load locally cached user summaries for offline user selection
    pub async fn load_user_summaries(&self) -> Result<Vec<UserListItemDto>> {
        let path = self.users_cache_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let data = tokio::fs::read_to_string(&path).await?;
        let users: Vec<UserListItemDto> = serde_json::from_str(&data)?;
        Ok(users)
    }

    /// Load cached user summaries scoped to a specific server base URL
    pub async fn load_user_summaries_for_server(
        &self,
        base_url: &str,
    ) -> Result<Vec<UserListItemDto>> {
        let path = self.users_cache_path_for_server(base_url);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let data = tokio::fs::read_to_string(&path).await?;
        let users: Vec<UserListItemDto> = serde_json::from_str(&data)?;
        Ok(users)
    }

    /// Save user summaries atomically
    pub async fn save_user_summaries(
        &self,
        users: &[UserListItemDto],
    ) -> Result<()> {
        let path = self.users_cache_path();
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let data = serde_json::to_string_pretty(users)?;
        tokio::fs::write(&path, data).await?;
        Ok(())
    }

    /// Save cached user summaries scoped to a specific server base URL
    pub async fn save_user_summaries_for_server(
        &self,
        base_url: &str,
        users: &[UserListItemDto],
    ) -> Result<()> {
        let path = self.users_cache_path_for_server(base_url);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let data = serde_json::to_string_pretty(users)?;
        tokio::fs::write(&path, data).await?;
        Ok(())
    }

    /// Clear locally cached user summaries to avoid stale user selection
    pub async fn clear_user_summaries(&self) -> Result<()> {
        let path = self.users_cache_path();
        if path.exists() {
            tokio::fs::remove_file(&path).await?;
            log::info!("Cleared cached user summaries at {:?}", path);
        }
        Ok(())
    }

    /// Clear cached user summaries for a specific server base URL
    pub async fn clear_user_summaries_for_server(
        &self,
        base_url: &str,
    ) -> Result<()> {
        let path = self.users_cache_path_for_server(base_url);
        if path.exists() {
            tokio::fs::remove_file(&path).await?;
            log::info!(
                "Cleared cached user summaries for server {} at {:?}",
                base_url,
                path
            );
        }
        Ok(())
    }

    /// Upsert a single user summary into the cache
    pub async fn upsert_user_summary(
        &self,
        summary: &UserListItemDto,
    ) -> Result<()> {
        let mut users = self.load_user_summaries().await.unwrap_or_default();
        if let Some(existing) = users.iter_mut().find(|u| u.id == summary.id) {
            *existing = summary.clone();
        } else {
            users.push(summary.clone());
        }
        self.save_user_summaries(&users).await
    }

    /// Upsert a single user summary into the cache for a specific server
    pub async fn upsert_user_summary_for_server(
        &self,
        base_url: &str,
        summary: &UserListItemDto,
    ) -> Result<()> {
        let mut users = self
            .load_user_summaries_for_server(base_url)
            .await
            .unwrap_or_default();
        if let Some(existing) = users.iter_mut().find(|u| u.id == summary.id) {
            *existing = summary.clone();
        } else {
            users.push(summary.clone());
        }
        self.save_user_summaries_for_server(base_url, &users).await
    }

    /// Derive encryption key from device fingerprint using Argon2
    ///
    /// This creates a deterministic key based on the device fingerprint,
    /// ensuring that auth data can only be decrypted on the same device.
    /// Uses Argon2id for strong key derivation resistant to GPU/ASIC attacks.
    fn derive_key(
        device_fingerprint: &str,
        salt: &[u8],
    ) -> Result<Key<Aes256Gcm>> {
        // Create Argon2 instance with custom parameters
        let params = Params::new(
            ARGON2_MEM_COST,
            ARGON2_TIME_COST,
            ARGON2_PARALLELISM,
            Some(32), // Output length for AES-256
        )
        .map_err(|e| anyhow::anyhow!("Invalid Argon2 parameters: {}", e))?;

        let argon2 = Argon2::new(
            argon2::Algorithm::Argon2id,
            argon2::Version::V0x13,
            params,
        );

        // Combine device fingerprint with app-specific salt
        let password = format!("{}{}", device_fingerprint, KEY_DERIVATION_SALT);

        // Derive key using Argon2
        let mut output = [0u8; 32];
        argon2
            .hash_password_into(password.as_bytes(), salt, &mut output)
            .map_err(|e| anyhow::anyhow!("Key derivation failed: {}", e))?;

        Ok(*Key::<Aes256Gcm>::from_slice(&output))
    }

    /// Save authentication data encrypted with device-specific key
    pub async fn save_auth(
        &self,
        auth: &StoredAuth,
        device_fingerprint: &str,
    ) -> Result<()> {
        // Add timestamp
        let mut auth_with_time = auth.clone();
        auth_with_time.stored_at = Utc::now();

        // Serialize the auth data
        let plaintext = serde_json::to_vec(&auth_with_time)
            .context("Failed to serialize auth data")?;

        // Generate random salt for key derivation
        let salt = SaltString::generate(&mut OsRng);
        let salt_bytes = salt.as_str().as_bytes();

        // Derive key from device fingerprint using Argon2
        let key = Self::derive_key(device_fingerprint, salt_bytes)?;

        // Create cipher
        let cipher = Aes256Gcm::new(&key);

        // Generate random nonce
        let nonce_bytes = Aes256Gcm::generate_nonce(&mut OsRng);

        // Encrypt
        let ciphertext = cipher
            .encrypt(&nonce_bytes, plaintext.as_ref())
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

        // Create encrypted data structure with v2 format
        let encrypted_data = EncryptedAuthData {
            nonce: BASE64.encode(nonce_bytes),
            ciphertext: BASE64.encode(ciphertext),
            encrypted_at: Utc::now(),
            version: 2,
            salt: Some(salt.to_string()),
        };

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&encrypted_data)
            .context("Failed to serialize encrypted data")?;

        // Ensure directory exists
        if let Some(parent) = self.cache_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Failed to create auth directory")?;
        }

        // Write to file
        tokio::fs::write(&self.cache_path, json)
            .await
            .context("Failed to write auth cache")?;

        log::info!("Saved encrypted auth data to {:?}", self.cache_path);
        Ok(())
    }

    /// Load and decrypt authentication data
    pub async fn load_auth(
        &self,
        device_fingerprint: &str,
    ) -> Result<Option<StoredAuth>> {
        // Check if file exists
        if !self.cache_path.exists() {
            log::debug!("No auth cache file found at {:?}", self.cache_path);
            return Ok(None);
        }

        // Read encrypted data
        let json = tokio::fs::read_to_string(&self.cache_path)
            .await
            .context("Failed to read auth cache")?;

        // Parse encrypted data
        let encrypted_data: EncryptedAuthData = serde_json::from_str(&json)
            .context("Failed to parse encrypted auth data")?;

        // Decode base64
        let nonce_bytes = BASE64
            .decode(&encrypted_data.nonce)
            .context("Failed to decode nonce")?;

        if nonce_bytes.len() != NONCE_SIZE {
            return Err(anyhow::anyhow!(
                "Invalid nonce length: expected {} bytes, got {}",
                NONCE_SIZE,
                nonce_bytes.len()
            ));
        }
        let ciphertext = BASE64
            .decode(&encrypted_data.ciphertext)
            .context("Failed to decode ciphertext")?;

        // Handle supported versions (v2 only). Any other version is treated as
        // unsupported and will be ignored (cache cleared or skipped).
        let key = match encrypted_data.version {
            1 => {
                // Legacy v1 format detected. We no longer support decrypting the
                // old format here. Treat it as stale/invalid cached auth rather
                // than crashing startup. Clear the cache and continue without
                // stored auth so the app can reach the login screen.
                log::warn!(
                    "Unsupported v1 auth cache detected; clearing and proceeding without stored auth"
                );
                let _ = self.clear_auth().await;
                return Ok(None);
            }
            2 => {
                // Current Argon2 format
                match encrypted_data.salt.as_ref() {
                    Some(salt_str) => {
                        let salt =
                            SaltString::from_b64(salt_str).map_err(|e| {
                                anyhow::anyhow!("Invalid salt format: {}", e)
                            })?;
                        Self::derive_key(
                            device_fingerprint,
                            salt.as_str().as_bytes(),
                        )?
                    }
                    None => {
                        return Err(anyhow::anyhow!(
                            "v2 auth cache missing required salt"
                        ));
                    }
                }
            }
            _ => {
                log::warn!(
                    "Unsupported auth cache version: {}",
                    encrypted_data.version
                );
                return Ok(None);
            }
        };

        // Create cipher
        let cipher = Aes256Gcm::new(&key);

        // Create nonce
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Decrypt
        let plaintext = cipher.decrypt(nonce, ciphertext.as_ref()).map_err(|e| {
            anyhow::anyhow!(
                "Decryption failed: {}. This usually means the device fingerprint has changed.",
                e
            )
        })?;

        // Deserialize auth data
        let auth: StoredAuth = serde_json::from_slice(&plaintext)
            .context("Failed to deserialize auth data")?;

        log::info!("Successfully loaded auth for user: {}", auth.user.username);

        Ok(Some(auth))
    }

    /// Clear stored authentication
    pub async fn clear_auth(&self) -> Result<()> {
        if self.cache_path.exists() {
            tokio::fs::remove_file(&self.cache_path)
                .await
                .context("Failed to remove auth cache")?;
            log::info!("Cleared auth cache");
        }
        Ok(())
    }

    /// Check if auth cache exists
    pub fn has_cached_auth(&self) -> bool {
        self.cache_path.exists()
    }

    /// Clear device status cache
    pub async fn clear_device_status(&self) -> Result<()> {
        // For now, this is a no-op as device status is managed server-side
        // In the future, we might cache device status locally
        Ok(())
    }

    /// Check if auto-login is enabled for a specific user on this device
    pub async fn is_auto_login_enabled(&self, user_id: &Uuid) -> Result<bool> {
        // Read auto-login preferences from a separate file
        let auto_login_path = self
            .cache_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Invalid cache path"))?
            .join("auto_login.json");

        if !auto_login_path.exists() {
            return Ok(false);
        }

        let data = tokio::fs::read_to_string(&auto_login_path).await?;
        let auto_login_map: std::collections::HashMap<Uuid, bool> =
            serde_json::from_str(&data)?;

        Ok(auto_login_map.get(user_id).copied().unwrap_or(false))
    }

    /// Set auto-login preference for a specific user on this device
    pub async fn set_auto_login(
        &self,
        user_id: &Uuid,
        enabled: bool,
    ) -> Result<()> {
        let auto_login_path = self
            .cache_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Invalid cache path"))?
            .join("auto_login.json");

        // Read existing preferences
        let mut auto_login_map: std::collections::HashMap<Uuid, bool> =
            if auto_login_path.exists() {
                let data = tokio::fs::read_to_string(&auto_login_path).await?;
                serde_json::from_str(&data)?
            } else {
                std::collections::HashMap::new()
            };

        // Update preference
        auto_login_map.insert(*user_id, enabled);

        // Write back
        let data = serde_json::to_string_pretty(&auto_login_map)?;
        tokio::fs::write(&auto_login_path, data).await?;

        Ok(())
    }

    /// Check if admin has unlocked PIN access for all users on this device
    pub async fn is_admin_pin_unlock_enabled(&self) -> Result<bool> {
        let admin_unlock_path = self
            .cache_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Invalid cache path"))?
            .join("admin_pin_unlock.json");

        if !admin_unlock_path.exists() {
            return Ok(false);
        }

        #[derive(serde::Deserialize)]
        struct AdminPinUnlock {
            enabled: bool,
            unlocked_by: Uuid,
            unlocked_at: chrono::DateTime<chrono::Utc>,
        }

        let data = tokio::fs::read_to_string(&admin_unlock_path).await?;
        let unlock_state: AdminPinUnlock = serde_json::from_str(&data)?;

        if unlock_state.enabled {
            log::debug!(
                "Admin PIN unlock enabled by {} at {}",
                unlock_state.unlocked_by,
                unlock_state.unlocked_at
            );
        }

        Ok(unlock_state.enabled)
    }

    /// Enable admin PIN unlock for all users on this device
    pub async fn enable_admin_pin_unlock(
        &self,
        admin_user_id: &Uuid,
    ) -> Result<()> {
        let admin_unlock_path = self
            .cache_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Invalid cache path"))?
            .join("admin_pin_unlock.json");

        #[derive(serde::Serialize)]
        struct AdminPinUnlock {
            enabled: bool,
            unlocked_by: Uuid,
            unlocked_at: chrono::DateTime<chrono::Utc>,
        }

        let unlock_state = AdminPinUnlock {
            enabled: true,
            unlocked_by: *admin_user_id,
            unlocked_at: chrono::Utc::now(),
        };

        let data = serde_json::to_string_pretty(&unlock_state)?;
        tokio::fs::write(&admin_unlock_path, data).await?;

        log::info!("Admin PIN unlock enabled by user {}", admin_user_id);
        Ok(())
    }

    /// Disable admin PIN unlock for all users on this device
    pub async fn disable_admin_pin_unlock(&self) -> Result<()> {
        let admin_unlock_path = self
            .cache_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Invalid cache path"))?
            .join("admin_pin_unlock.json");

        if admin_unlock_path.exists() {
            tokio::fs::remove_file(&admin_unlock_path).await?;
            log::info!("Admin PIN unlock disabled");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrex_core::domain::users::auth::domain::value_objects::SessionScope;
    use tempfile::TempDir;
    use uuid::Uuid;

    fn create_test_auth() -> StoredAuth {
        StoredAuth {
            token: AuthToken {
                access_token: "<REDACTED>".to_string(),
                refresh_token: "refresh_token".to_string(),
                expires_in: 3600,
                session_id: None,
                device_session_id: None,
                user_id: None,
                scope: SessionScope::Full,
            },
            user: User {
                id: Uuid::now_v7(),
                username: "testuser".to_string(),
                display_name: "Test User".to_string(),
                avatar_url: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                last_login: Some(Utc::now()),
                is_active: true,
                email: None,
                preferences: Default::default(),
            },
            server_url: "https://localhost:3000".to_string(),
            permissions: None,
            stored_at: Utc::now(),
            device_trust_expires_at: Some(
                Utc::now() + chrono::Duration::days(30),
            ),
            refresh_token: Some("refresh_token".to_string()),
        }
    }

    #[tokio::test]
    async fn test_save_and_load_auth() {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join(AUTH_CACHE_FILE);

        let storage = AuthStorage { cache_path };
        let device_fingerprint = "test-device-123";
        let auth = create_test_auth();

        // Save auth
        storage.save_auth(&auth, device_fingerprint).await.unwrap();

        // Load auth with same fingerprint
        let loaded = storage.load_auth(device_fingerprint).await.unwrap();
        assert!(loaded.is_some());

        let loaded_auth = loaded.unwrap();
        assert_eq!(loaded_auth.user.username, auth.user.username);
        assert_eq!(loaded_auth.token.access_token, auth.token.access_token);
    }

    #[tokio::test]
    async fn test_wrong_fingerprint_fails() {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join(AUTH_CACHE_FILE);

        let storage = AuthStorage { cache_path };
        let auth = create_test_auth();

        // Save with one fingerprint
        storage.save_auth(&auth, "device-1").await.unwrap();

        // Try to load with different fingerprint
        let result = storage.load_auth("device-2").await;
        assert!(result.is_err() || result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_clear_auth() {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join(AUTH_CACHE_FILE);

        let storage = AuthStorage {
            cache_path: cache_path.clone(),
        };
        let auth = create_test_auth();

        // Save auth
        storage.save_auth(&auth, "device").await.unwrap();
        assert!(cache_path.exists());

        // Clear auth
        storage.clear_auth().await.unwrap();
        assert!(!cache_path.exists());
    }

    #[tokio::test]
    async fn v1_auth_cache_is_ignored_and_cleared() {
        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join(AUTH_CACHE_FILE);

        // Craft a minimal v1-style file that passes JSON and base64 parsing.
        // Note: Nonce must be 12 bytes when decoded; 16 'A' => 12 zero bytes.
        let v1_json = serde_json::json!({
            "nonce": "AAAAAAAAAAAAAAAA",
            "ciphertext": "AQIDBAUGBwgJCgsMDQ4P", // arbitrary valid base64
            "encrypted_at": Utc::now(),
            "version": 1
        });
        tokio::fs::write(
            &cache_path,
            serde_json::to_string_pretty(&v1_json).unwrap(),
        )
        .await
        .unwrap();

        let storage = AuthStorage {
            cache_path: cache_path.clone(),
        };
        // Attempt to load with any fingerprint should not panic; should return None
        let loaded = storage.load_auth("any-fingerprint").await.unwrap();
        assert!(
            loaded.is_none(),
            "v1 cache should be ignored and yield None"
        );
        // File should be cleared as part of v1 handling
        assert!(
            !cache_path.exists(),
            "v1 cache file should be removed after detection"
        );
    }

    #[tokio::test]
    async fn server_scoped_user_cache_clears_on_request() {
        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join(AUTH_CACHE_FILE);

        let storage = AuthStorage { cache_path };

        // Seed a server-scoped user cache
        let base_url = "http://localhost:3000";
        let sample = vec![crate::domains::auth::dto::UserListItemDto {
            id: Uuid::now_v7(),
            username: "alice".into(),
            display_name: "Alice".into(),
            avatar_url: None,
            has_pin: true,
            last_login: Some(Utc::now()),
        }];
        storage
            .save_user_summaries_for_server(base_url, &sample)
            .await
            .unwrap();

        // Sanity: cache file should exist
        let server_cache = storage.users_cache_path_for_server(base_url);
        assert!(server_cache.exists());

        // Clear cache and verify removal
        storage
            .clear_user_summaries_for_server(base_url)
            .await
            .unwrap();
        assert!(
            !server_cache.exists(),
            "server-scoped users cache should be deleted"
        );
    }
}
