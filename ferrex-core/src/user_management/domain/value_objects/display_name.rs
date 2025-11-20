use serde::{Deserialize, Serialize};
use std::fmt;

/// Display name value object with validation
///
/// Represents a validated display name that follows the business rules:
/// - 1-100 characters in length
/// - Cannot be empty or only whitespace
/// - Preserves original formatting and case
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DisplayName(String);

impl DisplayName {
    /// Create a new display name with validation
    pub fn new(display_name: impl AsRef<str>) -> Result<Self, DisplayNameError> {
        let display_name = display_name.as_ref().trim().to_string();
        
        // Check for empty or whitespace-only names
        if display_name.is_empty() {
            return Err(DisplayNameError::Empty);
        }
        
        // Check length constraints
        if display_name.len() > 100 {
            return Err(DisplayNameError::TooLong);
        }
        
        // Check for control characters (except tab and newline which get trimmed)
        if display_name.chars().any(|c| c.is_control() && c != '\t' && c != '\n' && c != '\r') {
            return Err(DisplayNameError::InvalidCharacters);
        }
        
        Ok(Self(display_name))
    }

    /// Get the display name as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Get the display name as a String
    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for DisplayName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for DisplayName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Errors that can occur when creating a display name
#[derive(Debug, Clone, thiserror::Error)]
pub enum DisplayNameError {
    #[error("Display name cannot be empty")]
    Empty,
    
    #[error("Display name too long: maximum 100 characters allowed")]
    TooLong,
    
    #[error("Display name contains invalid control characters")]
    InvalidCharacters,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_display_names() {
        assert!(DisplayName::new("Alice").is_ok());
        assert!(DisplayName::new("Alice Smith").is_ok());
        assert!(DisplayName::new("Alice-Smith").is_ok());
        assert!(DisplayName::new("Alice123").is_ok());
        assert!(DisplayName::new("Alice (Admin)").is_ok());
        assert!(DisplayName::new("José María").is_ok());
    }

    #[test]
    fn invalid_display_names() {
        assert!(DisplayName::new("").is_err()); // Empty
        assert!(DisplayName::new("   ").is_err()); // Whitespace only
        assert!(DisplayName::new("a".repeat(101)).is_err()); // Too long
    }

    #[test]
    fn trimming() {
        let display_name = DisplayName::new("  Alice Smith  ").unwrap();
        assert_eq!(display_name.as_str(), "Alice Smith");
    }

    #[test]
    fn case_preservation() {
        let display_name = DisplayName::new("Alice Smith").unwrap();
        assert_eq!(display_name.as_str(), "Alice Smith");
    }
}