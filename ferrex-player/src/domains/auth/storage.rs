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

use ferrex_core::rbac::UserPermissions;
use ferrex_core::user::{AuthToken, User};
use uuid::Uuid;

const AUTH_CACHE_FILE: &str = "auth_cache.enc";
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
        let proj_dirs = ProjectDirs::from("", "ferrex", "media-player")
            .ok_or_else(|| anyhow::anyhow!("Unable to determine config directory"))?;

        let cache_path = proj_dirs.data_dir().join(AUTH_CACHE_FILE);

        Ok(Self { cache_path })
    }

    /// Get path to auth cache file
    pub fn cache_path(&self) -> &PathBuf {
        &self.cache_path
    }

    /// Create auth storage with custom cache path (for testing)
    #[cfg(any(test, feature = "testing"))]
    pub fn with_cache_path(cache_path: PathBuf) -> Self {
        Self { cache_path }
    }

    /// Derive encryption key from device fingerprint using Argon2
    ///
    /// This creates a deterministic key based on the device fingerprint,
    /// ensuring that auth data can only be decrypted on the same device.
    /// Uses Argon2id for strong key derivation resistant to GPU/ASIC attacks.
    fn derive_key(device_fingerprint: &str, salt: &[u8]) -> Result<Key<Aes256Gcm>> {
        // Create Argon2 instance with custom parameters
        let params = Params::new(
            ARGON2_MEM_COST,
            ARGON2_TIME_COST,
            ARGON2_PARALLELISM,
            Some(32), // Output length for AES-256
        )
        .map_err(|e| anyhow::anyhow!("Invalid Argon2 parameters: {}", e))?;

        let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

        // Combine device fingerprint with app-specific salt
        let password = format!("{}{}", device_fingerprint, KEY_DERIVATION_SALT);

        // Derive key using Argon2
        let mut output = [0u8; 32];
        argon2
            .hash_password_into(password.as_bytes(), salt, &mut output)
            .map_err(|e| anyhow::anyhow!("Key derivation failed: {}", e))?;

        Ok(Key::<Aes256Gcm>::from_slice(&output).clone())
    }

    /// Save authentication data encrypted with device-specific key
    pub async fn save_auth(&self, auth: &StoredAuth, device_fingerprint: &str) -> Result<()> {
        // Add timestamp
        let mut auth_with_time = auth.clone();
        auth_with_time.stored_at = Utc::now();

        // Serialize the auth data
        let plaintext =
            serde_json::to_vec(&auth_with_time).context("Failed to serialize auth data")?;

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
    pub async fn load_auth(&self, device_fingerprint: &str) -> Result<Option<StoredAuth>> {
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
        let encrypted_data: EncryptedAuthData =
            serde_json::from_str(&json).context("Failed to parse encrypted auth data")?;

        // Decode base64
        let nonce_bytes = BASE64
            .decode(&encrypted_data.nonce)
            .context("Failed to decode nonce")?;
        let ciphertext = BASE64
            .decode(&encrypted_data.ciphertext)
            .context("Failed to decode ciphertext")?;

        // Handle different versions
        let (key, needs_migration) = match encrypted_data.version {
            1 => {
                // Legacy SHA256 format - derive key using old method
                log::info!("Loading v1 auth cache - will migrate to v2 on next save");
                panic!("Found v1 auth cache");
            }
            2 => {
                // Current Argon2 format
                match encrypted_data.salt.as_ref() {
                    Some(salt_str) => {
                        let salt = SaltString::from_b64(salt_str)
                            .map_err(|e| anyhow::anyhow!("Invalid salt format: {}", e))?;
                        (
                            Self::derive_key(device_fingerprint, salt.as_str().as_bytes())?,
                            false,
                        )
                    }
                    None => {
                        return Err(anyhow::anyhow!("v2 auth cache missing required salt"));
                    }
                }
            }
            _ => {
                log::warn!("Unsupported auth cache version: {}", encrypted_data.version);
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
        let auth: StoredAuth =
            serde_json::from_slice(&plaintext).context("Failed to deserialize auth data")?;

        log::info!("Successfully loaded auth for user: {}", auth.user.username);

        // If we loaded a v1 file, automatically migrate it to v2
        if needs_migration {
            log::info!("Migrating auth cache from v1 to v2 format");
            if let Err(e) = self.save_auth(&auth, device_fingerprint).await {
                log::warn!("Failed to migrate auth cache to v2: {}", e);
            }
        }

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
        let auto_login_map: std::collections::HashMap<Uuid, bool> = serde_json::from_str(&data)?;

        Ok(auto_login_map.get(user_id).copied().unwrap_or(false))
    }

    /// Set auto-login preference for a specific user on this device
    pub async fn set_auto_login(&self, user_id: &Uuid, enabled: bool) -> Result<()> {
        let auto_login_path = self
            .cache_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Invalid cache path"))?
            .join("auto_login.json");

        // Read existing preferences
        let mut auto_login_map: std::collections::HashMap<Uuid, bool> = if auto_login_path.exists()
        {
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

        Ok(unlock_state.enabled)
    }

    /// Enable admin PIN unlock for all users on this device
    pub async fn enable_admin_pin_unlock(&self, admin_user_id: &Uuid) -> Result<()> {
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
    use tempfile::TempDir;
    use uuid::Uuid;

    fn create_test_auth() -> StoredAuth {
        StoredAuth {
            token: AuthToken {
                access_token: "<REDACTED>".to_string(),
                refresh_token: "refresh_token".to_string(),
                expires_in: 3600,
            },
            user: User {
                id: Uuid::new_v4(),
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
            server_url: "http://localhost:3000".to_string(),
            permissions: None,
            stored_at: Utc::now(),
            device_trust_expires_at: Some(Utc::now() + chrono::Duration::days(30)),
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
}
