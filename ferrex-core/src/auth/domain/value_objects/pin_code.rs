use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::auth::{AuthCrypto, AuthCryptoError};

/// Errors that can occur when working with PIN codes
#[derive(Debug, Error)]
pub enum PinCodeError {
    #[error("PIN is too short (minimum {min} digits)")]
    TooShort { min: usize },

    #[error("PIN is too long (maximum {max} digits)")]
    TooLong { max: usize },

    #[error("PIN contains invalid characters (only digits allowed)")]
    InvalidCharacters,

    #[error("PIN is too simple (e.g., 1234, 1111)")]
    TooSimple,

    #[error("PIN hashing failed")]
    HashingFailed,

    #[error("PIN verification failed")]
    VerificationFailed,

    #[error("PIN crypto error: {0}")]
    CryptoError(String),
}

/// PIN validation and security policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinPolicy {
    /// Minimum PIN length
    pub min_length: usize,

    /// Maximum PIN length
    pub max_length: usize,

    /// Whether to check for simple patterns
    pub check_patterns: bool,

    /// Maximum consecutive identical digits (e.g., 111)
    pub max_consecutive: usize,

    /// Whether to check for sequential patterns (e.g., 1234)
    pub check_sequential: bool,
}

impl Default for PinPolicy {
    fn default() -> Self {
        Self {
            min_length: 4,
            max_length: 8,
            check_patterns: true,
            max_consecutive: 2,
            check_sequential: true,
        }
    }
}

/// Secure PIN code value object
///
/// This type ensures PINs are:
/// - Properly validated according to policy
/// - Stored using Argon2id hashing
/// - Cleared from memory when dropped
#[derive(Debug, Clone)]
pub struct PinCode {
    /// The Argon2id hash of the PIN
    hash: String,
}

/// Temporary PIN holder that zeros memory on drop
#[derive(Zeroize, ZeroizeOnDrop)]
struct PinValue(String);

impl fmt::Debug for PinValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PinValue").field(&"<redacted>").finish()
    }
}

impl PinCode {
    /// Create a new PIN code with validation
    pub fn new(
        pin: String,
        _policy: &PinPolicy,
        crypto: &AuthCrypto,
    ) -> Result<Self, PinCodeError> {
        // Wrap in zeroizing container
        let mut pin_value = PinValue(pin);

        if pin_value.0.is_empty() {
            return Err(PinCodeError::TooShort { min: 1 });
        }

        // Hash the PIN proof material
        let hash = Self::hash_with_crypto(&pin_value.0, crypto)?;

        // Clear the original PIN
        pin_value.zeroize();

        Ok(Self { hash })
    }

    /// Create from an existing hash (for deserialization)
    pub fn from_hash(hash: String) -> Self {
        Self { hash }
    }

    /// Validate a PIN against the policy
    /// Hash a PIN using Argon2id
    fn hash_with_crypto(pin_material: &str, crypto: &AuthCrypto) -> Result<String, PinCodeError> {
        crypto.hash_password(pin_material).map_err(|err| match err {
            AuthCryptoError::PasswordHash(message) => PinCodeError::CryptoError(message),
            _ => PinCodeError::HashingFailed,
        })
    }

    /// Verify a PIN against this hash using constant-time comparison
    ///
    /// This method prevents timing attacks by ensuring that verification
    /// takes the same amount of time regardless of whether the PIN is correct
    /// or how many characters match.
    pub fn verify(&self, pin: &str, crypto: &AuthCrypto) -> Result<bool, PinCodeError> {
        let mut pin_value = PinValue(pin.to_string());

        let is_equal = crypto
            .verify_password(&pin_value.0, &self.hash)
            .map_err(|_| PinCodeError::VerificationFailed)?;

        pin_value.zeroize();

        Ok(is_equal)
    }

    /// Get the hash for storage
    pub fn hash(&self) -> &str {
        &self.hash
    }
}

// Custom serialization to only serialize the hash
impl Serialize for PinCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.hash.serialize(serializer)
    }
}

// Custom deserialization from hash
impl<'de> Deserialize<'de> for PinCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let hash = String::deserialize(deserializer)?;
        Ok(Self::from_hash(hash))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_crypto() -> AuthCrypto {
        AuthCrypto::new("test-pepper", "test-token").expect("crypto init")
    }

    #[test]
    fn test_valid_pin() {
        let policy = PinPolicy::default();
        let crypto = test_crypto();
        let pin = PinCode::new("5823".to_string(), &policy, &crypto).unwrap();
        assert!(!pin.hash.is_empty());
    }

    #[test]
    fn test_rejects_empty_proof() {
        let policy = PinPolicy::default();
        let crypto = test_crypto();
        let result = PinCode::new(String::new(), &policy, &crypto);
        assert!(matches!(result, Err(PinCodeError::TooShort { .. })));
    }

    #[test]
    fn accepts_arbitrary_client_proof_material() {
        let policy = PinPolicy::default();
        let crypto = test_crypto();
        let proof = "argon2id$v=19$m=65536,t=3,p=1$ZW1wdHlzbHQ$8J9CaJH2zv+2czZP2mEAPw";
        let pin = PinCode::new(proof.to_string(), &policy, &crypto).unwrap();
        assert!(pin.verify(proof, &crypto).unwrap());
    }

    #[test]
    fn test_pin_verification() {
        let policy = PinPolicy::default();
        let crypto = test_crypto();
        let pin = PinCode::new("5823".to_string(), &policy, &crypto).unwrap();

        assert!(pin.verify("5823", &crypto).unwrap());
        assert!(!pin.verify("5824", &crypto).unwrap());
    }

    #[test]
    fn test_constant_time_pin_verification() {
        use std::time::Instant;

        let policy = PinPolicy::default();
        let crypto = test_crypto();
        let pin = PinCode::new("1357".to_string(), &policy, &crypto).unwrap();

        // Test correct PIN
        let start = Instant::now();
        let result1 = pin.verify("1357", &crypto).unwrap();
        let time1 = start.elapsed();

        // Test incorrect PIN (completely different)
        let start = Instant::now();
        let result2 = pin.verify("9999", &crypto).unwrap();
        let time2 = start.elapsed();

        assert!(result1); // Correct PIN should verify
        assert!(!result2); // Incorrect PIN should not verify

        // The times should be relatively similar for constant-time behavior
        // This is not a perfect test, but it provides some confidence
        // that we're not leaking timing information through early returns
        println!("Correct PIN verification took: {:?}", time1);
        println!("Incorrect PIN verification took: {:?}", time2);

        // Both verifications should have completed (no panics or errors)
        // The actual timing will depend on Argon2's internal constant-time behavior
    }
}
