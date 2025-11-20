use anyhow::Result;
use chrono::{Duration, Utc};
use ferrex_core::user::Claims;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use sqlx::PgPool;
use std::env;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// JWT key manager for handling multiple keys during rotation
#[derive(Clone)]
pub struct JwtKeyManager {
    keys: Arc<RwLock<Vec<String>>>,
}

impl Default for JwtKeyManager {
    fn default() -> Self {
        Self::new()
    }
}

impl JwtKeyManager {
    /// Create a new key manager with the current JWT secret
    pub fn new() -> Self {
        let current_secret = env::var("JWT_SECRET").expect("JWT_SECRET must be set");
        Self {
            keys: Arc::new(RwLock::new(vec![current_secret])),
        }
    }

    /// Get the current (first) key for signing
    pub fn get_current_key(&self) -> String {
        let keys = self.keys.read().unwrap();
        keys[0].clone()
    }

    /// Get all keys for verification
    pub fn get_all_keys(&self) -> Vec<String> {
        let keys = self.keys.read().unwrap();
        keys.clone()
    }

    /// Add a new key and make it the current key
    /// Previous keys are kept for verification of existing tokens
    pub fn rotate_key(&self, new_key: String) {
        let mut keys = self.keys.write().unwrap();
        keys.insert(0, new_key);

        // Keep only the last 5 keys to prevent unlimited growth
        if keys.len() > 5 {
            keys.truncate(5);
        }
    }

    /// Remove old keys, keeping only the specified number
    pub fn cleanup_old_keys(&self, keep_count: usize) {
        let mut keys = self.keys.write().unwrap();
        if keys.len() > keep_count {
            keys.truncate(keep_count);
        }
    }
}

/// Global key manager instance
static KEY_MANAGER: std::sync::LazyLock<JwtKeyManager> =
    std::sync::LazyLock::new(JwtKeyManager::new);

/// Get the global key manager instance
pub fn get_key_manager() -> &'static JwtKeyManager {
    &KEY_MANAGER
}

/// Legacy function for backward compatibility
pub fn get_jwt_secret() -> String {
    get_key_manager().get_current_key()
}

pub fn generate_access_token(user_id: Uuid) -> Result<String, jsonwebtoken::errors::Error> {
    let now = Utc::now();
    let exp = now + Duration::seconds(900); // 15 minutes

    let claims = Claims {
        sub: user_id,
        exp: exp.timestamp(),
        iat: now.timestamp(),
        jti: Uuid::new_v4().to_string(),
    };

    // Always sign with the current (first) key
    let secret = get_key_manager().get_current_key();
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_ref()),
    )
}

pub fn generate_refresh_token() -> String {
    Uuid::new_v4().to_string()
}

pub async fn validate_token(
    token: &str,
    db: &PgPool,
) -> Result<Claims, jsonwebtoken::errors::Error> {
    let keys = get_key_manager().get_all_keys();
    let validation = Validation::new(Algorithm::HS256);

    // Try to decode with each key until one succeeds
    let mut last_error = None;
    for secret in keys {
        match decode::<Claims>(
            token,
            &DecodingKey::from_secret(secret.as_ref()),
            &validation,
        ) {
            Ok(token_data) => {
                let claims = token_data.claims;

                // Check if token is blacklisted
                let jti = &claims.jti;

                if is_token_revoked(db, jti).await? {
                    return Err(jsonwebtoken::errors::Error::from(
                        jsonwebtoken::errors::ErrorKind::InvalidToken,
                    ));
                }

                return Ok(claims);
            }
            Err(e) => {
                last_error = Some(e);
                continue;
            }
        }
    }

    // If we get here, none of the keys worked
    Err(last_error.unwrap_or_else(|| {
        jsonwebtoken::errors::Error::from(jsonwebtoken::errors::ErrorKind::InvalidToken)
    }))
}

// Legacy synchronous version for backward compatibility where revocation check is not needed
pub fn validate_token_sync(token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let keys = get_key_manager().get_all_keys();
    let validation = Validation::new(Algorithm::HS256);

    // Try to decode with each key until one succeeds
    let mut last_error = None;
    for secret in keys {
        match decode::<Claims>(
            token,
            &DecodingKey::from_secret(secret.as_ref()),
            &validation,
        ) {
            Ok(token_data) => return Ok(token_data.claims),
            Err(e) => {
                last_error = Some(e);
                continue;
            }
        }
    }

    // If we get here, none of the keys worked
    Err(last_error.unwrap_or_else(|| {
        jsonwebtoken::errors::Error::from(jsonwebtoken::errors::ErrorKind::InvalidToken)
    }))
}

async fn is_token_revoked(db: &PgPool, jti: &str) -> Result<bool, jsonwebtoken::errors::Error> {
    let result = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM jwt_blacklist WHERE jti = $1)",
        jti
    )
    .fetch_one(db)
    .await
    .map_err(|_| {
        jsonwebtoken::errors::Error::from(jsonwebtoken::errors::ErrorKind::InvalidToken)
    })?;

    Ok(result.unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_and_validate_token() {
        let user_id = Uuid::new_v4();
        let token = generate_access_token(user_id).expect("Failed to generate token");

        let claims = validate_token_sync(&token).expect("Failed to validate token");
        assert_eq!(claims.sub, user_id);
    }

    #[test]
    fn test_expired_token() {
        let user_id = Uuid::new_v4();
        let now = Utc::now();

        let claims = Claims {
            sub: user_id,
            exp: (now - Duration::seconds(100)).timestamp(), // Expired
            iat: (now - Duration::seconds(1000)).timestamp(),
            jti: Uuid::new_v4().to_string(),
        };

        let secret = get_jwt_secret();
        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(secret.as_ref()),
        )
        .unwrap();

        let result = validate_token_sync(&token);
        assert!(result.is_err());
    }

    #[test]
    fn test_key_manager_rotation() {
        let manager = JwtKeyManager::new();
        let original_key = manager.get_current_key();

        // Rotate to a new key
        let new_key = "new-secret-key".to_string();
        manager.rotate_key(new_key.clone());

        // Current key should be the new one
        assert_eq!(manager.get_current_key(), new_key);

        // All keys should include both old and new
        let all_keys = manager.get_all_keys();
        assert_eq!(all_keys.len(), 2);
        assert_eq!(all_keys[0], new_key);
        assert_eq!(all_keys[1], original_key);
    }

    #[test]
    fn test_multi_key_token_validation() {
        let user_id = Uuid::new_v4();
        let manager = JwtKeyManager::new();

        // Generate token with original key
        let token = generate_access_token(user_id).expect("Failed to generate token");

        // Should validate with original key
        let claims = validate_token_sync(&token).expect("Failed to validate with original key");
        assert_eq!(claims.sub, user_id);

        // Rotate key
        manager.rotate_key("new-secret-key".to_string());

        // Old token should still validate (using old key)
        let claims =
            validate_token_sync(&token).expect("Failed to validate with old key after rotation");
        assert_eq!(claims.sub, user_id);

        // New token should be generated with new key
        let new_token = generate_access_token(user_id).expect("Failed to generate new token");
        let new_claims = validate_token_sync(&new_token).expect("Failed to validate new token");
        assert_eq!(new_claims.sub, user_id);
    }

    #[test]
    fn test_key_cleanup() {
        let manager = JwtKeyManager::new();

        // Add multiple keys
        for i in 1..=10 {
            manager.rotate_key(format!("key-{}", i));
        }

        // Should have maximum of 5 keys
        assert_eq!(manager.get_all_keys().len(), 5);

        // Clean up to keep only 2 keys
        manager.cleanup_old_keys(2);
        assert_eq!(manager.get_all_keys().len(), 2);
    }
}
