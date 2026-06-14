use argon2::{
    Algorithm, Argon2, Params, ParamsBuilder, Version,
    password_hash::{
        PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
    },
};
use hmac::{Hmac, Mac};
use password_hash::Error as PasswordHashError;
use rand::{TryRngCore, rngs::OsRng};
use sha2::Sha256;
use thiserror::Error;
use zeroize::Zeroizing;

/// Centralized cryptographic helper for authentication-sensitive hashing.
///
/// The helper encapsulates two core primitives:
/// - Argon2id for password/PIN hashing with a server-side pepper.
/// - HMAC-SHA-256 for hashing opaque bearer tokens before persistence.
///
/// Keeping these in one place guarantees consistent parameter choices and
/// makes it easier to rotate peppers/keys in the future.
#[derive(Debug)]
pub struct AuthCrypto {
    argon2: Argon2<'static>,
    password_pepper: Zeroizing<Vec<u8>>,
    token_hmac_key: Zeroizing<Vec<u8>>,
}

#[derive(Debug, Error)]
pub enum AuthCryptoError {
    #[error("password pepper must not be empty")]
    EmptyPasswordPepper,
    #[error("token HMAC key must not be empty")]
    EmptyTokenKey,
    #[error("invalid Argon2 parameters: {0}")]
    InvalidArgon2Params(String),
    #[error("password hashing error: {0}")]
    PasswordHash(String),
}

impl From<PasswordHashError> for AuthCryptoError {
    fn from(err: PasswordHashError) -> Self {
        AuthCryptoError::PasswordHash(err.to_string())
    }
}

impl AuthCrypto {
    /// Recommended defaults target ~64 MiB memory and 3 iterations which is a
    /// solid baseline for servers without dedicated tuning.
    const DEFAULT_MEMORY_KIB: u32 = 64 * 1024; // 64 MiB
    const DEFAULT_ITERATIONS: u32 = 3;
    const DEFAULT_PARALLELISM: u32 = 1;
    const SALT_LENGTH: usize = password_hash::Salt::RECOMMENDED_LENGTH;

    /// Build a helper with default Argon2id parameters.
    pub fn new(
        password_pepper: impl AsRef<[u8]>,
        token_hmac_key: impl AsRef<[u8]>,
    ) -> Result<Self, AuthCryptoError> {
        Self::with_params(
            password_pepper,
            token_hmac_key,
            ParamsBuilder::new()
                .m_cost(Self::DEFAULT_MEMORY_KIB)
                .t_cost(Self::DEFAULT_ITERATIONS)
                .p_cost(Self::DEFAULT_PARALLELISM)
                .output_len(32)
                .build()
                .map_err(|err| {
                    AuthCryptoError::InvalidArgon2Params(err.to_string())
                })?,
        )
    }

    /// Build a helper with caller-specified Argon2 parameters (useful for
    /// integration tests or constrained environments).
    pub fn with_params(
        password_pepper: impl AsRef<[u8]>,
        token_hmac_key: impl AsRef<[u8]>,
        params: Params,
    ) -> Result<Self, AuthCryptoError> {
        let pepper = password_pepper.as_ref();
        if pepper.is_empty() {
            return Err(AuthCryptoError::EmptyPasswordPepper);
        }

        let key = token_hmac_key.as_ref();
        if key.is_empty() {
            return Err(AuthCryptoError::EmptyTokenKey);
        }

        let argon2 =
            Argon2::new(Algorithm::Argon2id, Version::default(), params);

        Ok(Self {
            argon2,
            password_pepper: Zeroizing::new(pepper.to_vec()),
            token_hmac_key: Zeroizing::new(key.to_vec()),
        })
    }

    /// Hash a password (or PIN) using Argon2id with a random salt and shared
    /// pepper. The resulting PHC string is suitable for storage.
    pub fn hash_password(
        &self,
        password: &str,
    ) -> Result<String, AuthCryptoError> {
        let mut material = Zeroizing::new(Vec::with_capacity(
            password.len() + self.password_pepper.len(),
        ));
        material.extend_from_slice(password.as_bytes());
        material.extend_from_slice(&self.password_pepper);

        // Use the workspace's rand crate so minimal builds avoid depending on
        // password_hash's optional rand_core shim.
        let mut salt_bytes = [0u8; Self::SALT_LENGTH];
        OsRng
            .try_fill_bytes(&mut salt_bytes)
            .map_err(|err| AuthCryptoError::PasswordHash(err.to_string()))?;
        let salt = SaltString::encode_b64(&salt_bytes)
            .map_err(AuthCryptoError::from)?;
        let hash = self.argon2.hash_password(&material, &salt)?.to_string();
        Ok(hash)
    }

    /// Verify a password/PIN against a stored hash, applying the shared pepper.
    pub fn verify_password(
        &self,
        password: &str,
        password_hash: &str,
    ) -> Result<bool, AuthCryptoError> {
        let parsed = PasswordHash::new(password_hash)?;

        let mut material = Zeroizing::new(Vec::with_capacity(
            password.len() + self.password_pepper.len(),
        ));
        material.extend_from_slice(password.as_bytes());
        material.extend_from_slice(&self.password_pepper);

        Ok(self.argon2.verify_password(&material, &parsed).is_ok())
    }

    /// Hash an opaque bearer token (session, refresh, etc.) using HMAC-SHA-256
    /// with the configured secret key. The digest is returned as hex for
    /// storage in the database.
    pub fn hash_token(&self, token: &str) -> String {
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = HmacSha256::new_from_slice(&self.token_hmac_key)
            .expect("HMAC-SHA-256 accepts keys of any size");
        mac.update(token.as_bytes());

        let digest = mac.finalize().into_bytes();
        hex::encode(digest)
    }

    #[cfg(test)]
    pub fn password_pepper(&self) -> &[u8] {
        &self.password_pepper
    }

    #[cfg(test)]
    pub fn token_key(&self) -> &[u8] {
        &self.token_hmac_key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_passwords_and_verifies() {
        let crypto = AuthCrypto::new("pepper", "token-key").unwrap();
        let hash = crypto.hash_password("correct horse").unwrap();
        assert!(crypto.verify_password("correct horse", &hash).unwrap());
        assert!(!crypto.verify_password("battery staple", &hash).unwrap());
    }

    #[test]
    fn hashes_tokens_to_hex() {
        let crypto = AuthCrypto::new("pepper", "token-key").unwrap();
        let digest = crypto.hash_token("opaque-token");
        assert_eq!(digest.len(), 64);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn rejects_empty_inputs() {
        assert!(matches!(
            AuthCrypto::new("", "token"),
            Err(AuthCryptoError::EmptyPasswordPepper)
        ));
        assert!(matches!(
            AuthCrypto::new("pepper", ""),
            Err(AuthCryptoError::EmptyTokenKey)
        ));
    }
}
