use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use constant_time_eq::constant_time_eq;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum RefreshTokenError {
    #[error("invalid token format")]
    InvalidFormat,
    #[error("token generation failed")]
    GenerationFailed,
}

/// Refresh token value object handling rotation metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshToken {
    value: String,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    family_id: Uuid,
    generation: u32,
}

impl RefreshToken {
    pub fn generate(lifetime: Duration) -> Result<Self, RefreshTokenError> {
        Self::generate_with_family(lifetime, Uuid::now_v7(), 1)
    }

    pub fn rotate(&self, lifetime: Duration) -> Result<Self, RefreshTokenError> {
        Self::generate_with_family(lifetime, self.family_id, self.generation + 1)
    }

    pub fn from_value(
        value: String,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        family_id: Uuid,
        generation: u32,
    ) -> Result<Self, RefreshTokenError> {
        if value.is_empty() || generation == 0 || expires_at <= issued_at {
            return Err(RefreshTokenError::InvalidFormat);
        }

        let is_b64 = URL_SAFE_NO_PAD.decode(&value).is_ok();
        let is_hex = value.len() % 2 == 0 && value.chars().all(|c| c.is_ascii_hexdigit());

        if !is_b64 && !is_hex {
            return Err(RefreshTokenError::InvalidFormat);
        }

        Ok(Self {
            value,
            issued_at,
            expires_at,
            family_id,
            generation,
        })
    }

    fn generate_with_family(
        lifetime: Duration,
        family_id: Uuid,
        generation: u32,
    ) -> Result<Self, RefreshTokenError> {
        use ring::rand::{SecureRandom, SystemRandom};

        let rng = SystemRandom::new();
        let mut token_bytes = [0u8; 32];
        rng.fill(&mut token_bytes)
            .map_err(|_| RefreshTokenError::GenerationFailed)?;

        let value = URL_SAFE_NO_PAD.encode(token_bytes);
        let issued_at = Utc::now();
        let expires_at = issued_at + lifetime;

        Ok(Self {
            value,
            issued_at,
            expires_at,
            family_id,
            generation,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.value
    }

    pub fn issued_at(&self) -> DateTime<Utc> {
        self.issued_at
    }

    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    pub fn family_id(&self) -> Uuid {
        self.family_id
    }

    pub fn generation(&self) -> u32 {
        self.generation
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }

    pub fn secure_compare(&self, other: &str) -> bool {
        let self_bytes = self.value.as_bytes();
        let other_bytes = other.as_bytes();

        if self_bytes.len() != other_bytes.len() {
            return false;
        }

        constant_time_eq(self_bytes, other_bytes)
    }
}

impl Drop for RefreshToken {
    fn drop(&mut self) {
        unsafe {
            self.value.as_mut_vec().fill(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_refresh_tokens() {
        let token = RefreshToken::generate(Duration::days(30)).unwrap();
        assert_eq!(token.generation(), 1);
        assert!(token.expires_at() > token.issued_at());
    }

    #[test]
    fn rotates_refresh_tokens() {
        let token = RefreshToken::generate(Duration::days(30)).unwrap();
        let rotated = token.rotate(Duration::days(30)).unwrap();
        assert_eq!(token.family_id(), rotated.family_id());
        assert_eq!(rotated.generation(), token.generation() + 1);
    }
}
