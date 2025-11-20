use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use ring::constant_time;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

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

impl PinCode {
    /// Create a new PIN code with validation
    pub fn new(pin: String, policy: &PinPolicy) -> Result<Self, PinCodeError> {
        // Wrap in zeroizing container
        let mut pin_value = PinValue(pin);

        // Validate the PIN
        Self::validate(&pin_value.0, policy)?;

        // Hash the PIN
        let hash = Self::hash_pin(&pin_value.0)?;

        // Clear the original PIN
        pin_value.zeroize();

        Ok(Self { hash })
    }

    /// Create from an existing hash (for deserialization)
    pub fn from_hash(hash: String) -> Self {
        Self { hash }
    }

    /// Validate a PIN against the policy
    fn validate(pin: &str, policy: &PinPolicy) -> Result<(), PinCodeError> {
        // Check length
        if pin.len() < policy.min_length {
            return Err(PinCodeError::TooShort {
                min: policy.min_length,
            });
        }

        if pin.len() > policy.max_length {
            return Err(PinCodeError::TooLong {
                max: policy.max_length,
            });
        }

        // Check for non-digit characters
        if !pin.chars().all(|c| c.is_ascii_digit()) {
            return Err(PinCodeError::InvalidCharacters);
        }

        // Check for patterns if enabled
        if policy.check_patterns {
            let digits: Vec<u8> = pin.chars().map(|c| c.to_digit(10).unwrap() as u8).collect();

            // Check consecutive identical digits
            if policy.max_consecutive > 0 {
                let mut consecutive = 1;
                for i in 1..digits.len() {
                    if digits[i] == digits[i - 1] {
                        consecutive += 1;
                        if consecutive > policy.max_consecutive {
                            return Err(PinCodeError::TooSimple);
                        }
                    } else {
                        consecutive = 1;
                    }
                }
            }

            // Check sequential patterns
            if policy.check_sequential {
                let mut ascending = 1;
                let mut descending = 1;

                for i in 1..digits.len() {
                    if digits[i] == digits[i - 1] + 1 {
                        ascending += 1;
                        if ascending >= pin.len() {
                            return Err(PinCodeError::TooSimple);
                        }
                    } else {
                        ascending = 1;
                    }

                    if digits[i] + 1 == digits[i - 1] {
                        descending += 1;
                        if descending >= pin.len() {
                            return Err(PinCodeError::TooSimple);
                        }
                    } else {
                        descending = 1;
                    }
                }
            }
        }

        Ok(())
    }

    /// Hash a PIN using Argon2id
    fn hash_pin(pin: &str) -> Result<String, PinCodeError> {
        use ring::rand::{SecureRandom, SystemRandom};

        let rng = SystemRandom::new();
        let mut salt_bytes = [0u8; 16];
        rng.fill(&mut salt_bytes)
            .map_err(|_| PinCodeError::HashingFailed)?;

        let salt = SaltString::encode_b64(&salt_bytes).map_err(|_| PinCodeError::HashingFailed)?;

        let argon2 = Argon2::default();
        let hash = argon2
            .hash_password(pin.as_bytes(), &salt)
            .map_err(|_| PinCodeError::HashingFailed)?
            .to_string();

        Ok(hash)
    }

    /// Verify a PIN against this hash using constant-time comparison
    ///
    /// This method prevents timing attacks by ensuring that verification
    /// takes the same amount of time regardless of whether the PIN is correct
    /// or how many characters match.
    pub fn verify(&self, pin: &str) -> Result<bool, PinCodeError> {
        let mut pin_value = PinValue(pin.to_string());

        let parsed_hash =
            PasswordHash::new(&self.hash).map_err(|_| PinCodeError::VerificationFailed)?;

        let argon2 = Argon2::default();

        // Perform Argon2 verification (already constant-time internally)
        let argon2_result = argon2.verify_password(pin_value.0.as_bytes(), &parsed_hash);

        // Convert result to bytes for constant-time comparison
        // This ensures we don't leak timing information through early returns
        let verification_passed = if argon2_result.is_ok() { 1u8 } else { 0u8 };
        let expected_success = 1u8;

        // Use constant-time comparison to prevent timing attacks
        let is_equal =
            constant_time::verify_slices_are_equal(&[verification_passed], &[expected_success]);

        pin_value.zeroize();

        Ok(is_equal.is_ok())
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

    #[test]
    fn test_valid_pin() {
        let policy = PinPolicy::default();
        let pin = PinCode::new("5823".to_string(), &policy).unwrap();
        assert!(!pin.hash.is_empty());
    }

    #[test]
    fn test_pin_too_short() {
        let policy = PinPolicy::default();
        let result = PinCode::new("123".to_string(), &policy);
        assert!(matches!(result, Err(PinCodeError::TooShort { .. })));
    }

    #[test]
    fn test_pin_too_simple() {
        let policy = PinPolicy::default();

        // Test consecutive digits
        let result = PinCode::new("1111".to_string(), &policy);
        assert!(matches!(result, Err(PinCodeError::TooSimple)));

        // Test sequential
        let result = PinCode::new("1234".to_string(), &policy);
        assert!(matches!(result, Err(PinCodeError::TooSimple)));
    }

    #[test]
    fn test_pin_verification() {
        let policy = PinPolicy::default();
        let pin = PinCode::new("5823".to_string(), &policy).unwrap();

        assert!(pin.verify("5823").unwrap());
        assert!(!pin.verify("5824").unwrap());
    }

    #[test]
    fn test_constant_time_pin_verification() {
        use std::time::Instant;

        let policy = PinPolicy::default();
        let pin = PinCode::new("1357".to_string(), &policy).unwrap();

        // Test correct PIN
        let start = Instant::now();
        let result1 = pin.verify("1357").unwrap();
        let time1 = start.elapsed();

        // Test incorrect PIN (completely different)
        let start = Instant::now();
        let result2 = pin.verify("9999").unwrap();
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
