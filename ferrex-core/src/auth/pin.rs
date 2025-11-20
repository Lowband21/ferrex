//! PIN management for device-based authentication

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// PIN policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinPolicy {
    /// Minimum PIN length (recommended: 6)
    pub min_length: usize,
    /// Maximum PIN length
    pub max_length: usize,
    /// Maximum failed attempts before lockout
    pub max_attempts: u8,
    /// Duration of lockout after max attempts
    pub lockout_duration_minutes: u32,
    /// Whether PIN requires trusted device
    pub requires_device_trust: bool,
    /// Whether to allow simple PINs (1234, 0000, etc)
    pub allow_simple_pins: bool,
}

impl Default for PinPolicy {
    fn default() -> Self {
        Self {
            min_length: 6, // 6 digits for better security than 4
            max_length: 8,
            max_attempts: 5,
            lockout_duration_minutes: 30,
            requires_device_trust: true,
            allow_simple_pins: false,
        }
    }
}

/// PIN attempt tracking for rate limiting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinAttemptTracker {
    pub device_id: Uuid,
    pub user_id: Uuid,
    pub failed_attempts: u8,
    pub last_attempt: DateTime<Utc>,
    pub locked_until: Option<DateTime<Utc>>,
}

impl PinAttemptTracker {
    pub fn new(device_id: Uuid, user_id: Uuid) -> Self {
        Self {
            device_id,
            user_id,
            failed_attempts: 0,
            last_attempt: Utc::now(),
            locked_until: None,
        }
    }

    /// Check if the account is currently locked
    pub fn is_locked(&self) -> bool {
        if let Some(locked_until) = self.locked_until {
            locked_until > Utc::now()
        } else {
            false
        }
    }

    /// Record a failed attempt
    pub fn record_failure(&mut self, policy: &PinPolicy) {
        self.failed_attempts += 1;
        self.last_attempt = Utc::now();

        if self.failed_attempts >= policy.max_attempts {
            self.locked_until =
                Some(Utc::now() + Duration::minutes(policy.lockout_duration_minutes as i64));
        }
    }

    /// Reset tracker after successful authentication
    pub fn reset(&mut self) {
        self.failed_attempts = 0;
        self.locked_until = None;
        self.last_attempt = Utc::now();
    }
}

/// PIN validation errors
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
pub enum PinError {
    #[error("PIN is too short (minimum {min} characters)")]
    TooShort { min: usize },

    #[error("PIN is too long (maximum {max} characters)")]
    TooLong { max: usize },

    #[error("PIN must contain only digits")]
    InvalidCharacters,

    #[error("PIN is too simple (avoid sequences like 1234 or repeated digits)")]
    TooSimple,

    #[error("Account is locked until {until}")]
    LockedOut { until: DateTime<Utc> },

    #[error("Invalid PIN ({attempts_remaining} attempts remaining)")]
    Invalid { attempts_remaining: u8 },

    #[error("PIN verification failed")]
    VerificationFailed,
}

/// Validate a PIN according to policy
pub fn validate_pin(pin: &str, policy: &PinPolicy) -> Result<(), PinError> {
    // Length check
    if pin.len() < policy.min_length {
        return Err(PinError::TooShort {
            min: policy.min_length,
        });
    }
    if pin.len() > policy.max_length {
        return Err(PinError::TooLong {
            max: policy.max_length,
        });
    }

    // Must be all digits
    if !pin.chars().all(|c| c.is_numeric()) {
        return Err(PinError::InvalidCharacters);
    }

    // Check for simple patterns if policy disallows them
    if !policy.allow_simple_pins && is_simple_pin(pin) {
        return Err(PinError::TooSimple);
    }

    Ok(())
}

/// Check if a PIN is too simple (sequential or repeated digits)
fn is_simple_pin(pin: &str) -> bool {
    let digits: Vec<u8> = pin
        .chars()
        .filter_map(|c| c.to_digit(10).map(|d| d as u8))
        .collect();

    if digits.is_empty() {
        return false;
    }

    // Check for all same digit (0000, 1111, etc)
    if digits.windows(2).all(|w| w[0] == w[1]) {
        return true;
    }

    // Check for sequential ascending (1234, 2345, etc)
    if digits.windows(2).all(|w| w[1] == w[0] + 1) {
        return true;
    }

    // Check for sequential descending (4321, 9876, etc)
    if digits.windows(2).all(|w| w[0] > 0 && w[1] == w[0] - 1) {
        return true;
    }

    // Common patterns
    let common_patterns = [
        "123456", "654321", "111111", "000000", "123123", "121212", "123321", "696969", "112233",
        "159753",
    ];

    common_patterns.contains(&pin)
}

// Note: PIN hashing and verification are implemented in the server module
// to have access to proper cryptographic dependencies.
// These functions are placeholders for the expected interface.

/// Request to set or update a PIN
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetPinRequest {
    pub device_id: Uuid,
    pub current_password_or_pin: String,
    pub new_pin: String,
}

/// PIN setup result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetPinResponse {
    pub success: bool,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_pin_detection() {
        assert!(is_simple_pin("1234"));
        assert!(is_simple_pin("0000"));
        assert!(is_simple_pin("1111"));
        assert!(is_simple_pin("9876"));
        assert!(is_simple_pin("123456"));

        assert!(!is_simple_pin("1357"));
        assert!(!is_simple_pin("2468"));
        assert!(!is_simple_pin("9183"));
    }

    #[test]
    fn test_pin_validation() {
        let policy = PinPolicy::default();

        assert!(validate_pin("123456", &policy).is_err()); // Too simple
        assert!(validate_pin("12345", &policy).is_err()); // Too short
        assert!(validate_pin("123456789", &policy).is_err()); // Too long
        assert!(validate_pin("12345a", &policy).is_err()); // Invalid chars

        assert!(validate_pin("182736", &policy).is_ok());
        assert!(validate_pin("924681", &policy).is_ok());
    }
}
