use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use constant_time_eq::constant_time_eq;
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Errors that can occur when working with session tokens
#[derive(Debug, Error)]
pub enum SessionTokenError {
    #[error("Invalid token format")]
    InvalidFormat,

    #[error("Token has expired")]
    Expired,

    #[error("Token generation failed")]
    GenerationFailed,
}

/// Cryptographically secure session token
///
/// This value object represents a session token that is:
/// - Cryptographically secure (256 bits of entropy)
/// - URL-safe base64 encoded
/// - Immutable once created
/// - Implements constant-time comparison
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionToken {
    /// The actual token value (base64 URL-safe encoded)
    value: String,

    /// When this token was created
    created_at: DateTime<Utc>,

    /// When this token expires
    expires_at: DateTime<Utc>,
}

impl SessionToken {
    /// Generate a new session token with the specified lifetime
    pub fn generate(lifetime: Duration) -> Result<Self, SessionTokenError> {
        use ring::rand::{SecureRandom, SystemRandom};

        let rng = SystemRandom::new();
        let mut token_bytes = [0u8; 32]; // 256 bits

        rng.fill(&mut token_bytes)
            .map_err(|_| SessionTokenError::GenerationFailed)?;

        let value = URL_SAFE_NO_PAD.encode(token_bytes);
        let created_at = Utc::now();
        let expires_at = created_at + lifetime;

        Ok(Self {
            value,
            created_at,
            expires_at,
        })
    }

    /// Create a token from an existing value (for deserialization)
    pub fn from_value(
        value: String,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, SessionTokenError> {
        // Validate the token format
        if value.is_empty() || URL_SAFE_NO_PAD.decode(&value).is_err() {
            return Err(SessionTokenError::InvalidFormat);
        }

        Ok(Self {
            value,
            created_at,
            expires_at,
        })
    }

    /// Check if the token is expired
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Check if the token is valid (not expired)
    pub fn is_valid(&self) -> bool {
        !self.is_expired()
    }

    /// Get the token value as a string reference
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Get when this token expires
    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    /// Get when this token was created
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// Constant-time comparison with another token
    pub fn secure_compare(&self, other: &str) -> bool {
        let self_bytes = self.value.as_bytes();
        let other_bytes = other.as_bytes();

        if self_bytes.len() != other_bytes.len() {
            return false;
        }

        constant_time_eq(self_bytes, other_bytes)
    }
}

impl fmt::Display for SessionToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Only show first 8 chars for security
        let preview = if self.value.len() > 8 {
            &self.value[..8]
        } else {
            &self.value
        };
        write!(f, "{}...", preview)
    }
}

// Implement zeroize manually since we can't derive it
impl Drop for SessionToken {
    fn drop(&mut self) {
        // Clear the token value from memory
        unsafe {
            self.value.as_mut_vec().fill(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_generation() {
        let token = SessionToken::generate(Duration::hours(1)).unwrap();
        assert!(!token.value.is_empty());
        assert!(token.is_valid());
    }

    #[test]
    fn test_token_expiration() {
        let created_at = Utc::now() - Duration::hours(2);
        let expires_at = created_at + Duration::hours(1);

        let token =
            SessionToken::from_value("test_token".to_string(), created_at, expires_at).unwrap();

        assert!(token.is_expired());
        assert!(!token.is_valid());
    }

    #[test]
    fn test_secure_compare() {
        let token1 = SessionToken::generate(Duration::hours(1)).unwrap();
        let token2 = SessionToken::generate(Duration::hours(1)).unwrap();

        assert!(token1.secure_compare(token1.as_str()));
        assert!(!token1.secure_compare(token2.as_str()));
    }
}
