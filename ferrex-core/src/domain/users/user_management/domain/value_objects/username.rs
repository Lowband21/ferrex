use serde::{Deserialize, Serialize};
use std::fmt;

/// Username value object with validation
///
/// Represents a validated username that follows the business rules:
/// - 3-30 characters in length
/// - Alphanumeric characters and underscores only
/// - Normalized to lowercase for consistency
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Username(String);

impl Username {
    /// Create a new username with validation
    pub fn new(username: impl AsRef<str>) -> Result<Self, UsernameError> {
        let username = username.as_ref().trim().to_lowercase();

        // Check length constraints
        if username.len() < 3 {
            return Err(UsernameError::TooShort);
        }

        if username.len() > 30 {
            return Err(UsernameError::TooLong);
        }

        // Check character constraints
        if !username.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(UsernameError::InvalidCharacters);
        }

        // Cannot start with underscore
        if username.starts_with('_') {
            return Err(UsernameError::InvalidFormat);
        }

        // Cannot end with underscore
        if username.ends_with('_') {
            return Err(UsernameError::InvalidFormat);
        }

        // Cannot have consecutive underscores
        if username.contains("__") {
            return Err(UsernameError::InvalidFormat);
        }

        Ok(Self(username))
    }

    /// Get the username as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Get the username as a String
    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for Username {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for Username {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Errors that can occur when creating a username
#[derive(Debug, Clone, thiserror::Error)]
pub enum UsernameError {
    #[error("Username too short: minimum 3 characters required")]
    TooShort,

    #[error("Username too long: maximum 30 characters allowed")]
    TooLong,

    #[error(
        "Username contains invalid characters: only alphanumeric and underscore allowed"
    )]
    InvalidCharacters,

    #[error(
        "Username format invalid: cannot start/end with underscore or contain consecutive underscores"
    )]
    InvalidFormat,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_usernames() {
        assert!(Username::new("alice").is_ok());
        assert!(Username::new("alice_123").is_ok());
        assert!(Username::new("user_name").is_ok());
        assert!(Username::new("test123").is_ok());
    }

    #[test]
    fn invalid_usernames() {
        assert!(Username::new("ab").is_err()); // Too short
        assert!(Username::new("a".repeat(31)).is_err()); // Too long
        assert!(Username::new("alice@bob").is_err()); // Invalid character
        assert!(Username::new("_alice").is_err()); // Starts with underscore
        assert!(Username::new("alice_").is_err()); // Ends with underscore
        assert!(Username::new("alice__bob").is_err()); // Consecutive underscores
    }

    #[test]
    fn normalization() {
        let username = Username::new("  Alice  ").unwrap();
        assert_eq!(username.as_str(), "alice");
    }
}
