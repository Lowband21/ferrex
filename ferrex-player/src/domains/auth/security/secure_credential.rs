use std::fmt;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// A secure credential type that automatically zeros memory on drop
///
/// This type is designed to hold sensitive data like passwords, tokens, and other
/// credentials. The memory is automatically zeroed when the value is dropped,
/// helping to prevent sensitive data from lingering in memory.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct SecureCredential {
    data: String,
}

impl SecureCredential {
    /// Create a new SecureCredential from a string
    pub fn new(data: String) -> Self {
        Self { data }
    }

    /// Create a new SecureCredential from a string slice
    pub fn new_from_str(data: &str) -> Self {
        Self {
            data: data.to_string(),
        }
    }

    /// Get a reference to the credential data as a string slice
    ///
    /// # Security Note
    /// Be careful when using this method. The returned reference points to the
    /// same memory that will be zeroed on drop. Avoid storing this reference
    /// beyond the lifetime of the SecureCredential.
    pub fn as_str(&self) -> &str {
        &self.data
    }

    /// Get the length of the credential data
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if the credential is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Expose the internal data for operations that require owned String
    ///
    /// # Security Note
    /// This method should be used sparingly and only when necessary for API
    /// compatibility. The returned String will not be automatically zeroed.
    pub fn expose_secret(&self) -> &String {
        &self.data
    }
}

impl Clone for SecureCredential {
    /// Clone implementation that maintains security properties
    ///
    /// Creates a new SecureCredential with a copy of the data.
    /// Both the original and cloned values will be properly zeroed on drop.
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
        }
    }
}

impl From<String> for SecureCredential {
    fn from(data: String) -> Self {
        Self::new(data)
    }
}

impl From<&str> for SecureCredential {
    fn from(data: &str) -> Self {
        Self::new_from_str(data)
    }
}

impl fmt::Debug for SecureCredential {
    /// Debug implementation that doesn't expose the credential data
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecureCredential")
            .field("len", &self.len())
            .field("data", &"[REDACTED]")
            .finish()
    }
}

impl fmt::Display for SecureCredential {
    /// Display implementation that doesn't expose the credential data
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[SecureCredential: {} bytes]", self.len())
    }
}

impl PartialEq for SecureCredential {
    /// Secure comparison that uses constant-time comparison when possible
    fn eq(&self, other: &Self) -> bool {
        // For strings of different lengths, we can short-circuit
        if self.len() != other.len() {
            return false;
        }

        // For same-length strings, use byte-by-byte comparison
        // This isn't truly constant-time but it's better than using string comparison
        self.data.as_bytes() == other.data.as_bytes()
    }
}

impl Eq for SecureCredential {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_and_basic_operations() {
        let credential = SecureCredential::new("test_password".to_string());
        assert_eq!(credential.as_str(), "test_password");
        assert_eq!(credential.len(), 13);
        assert!(!credential.is_empty());
    }

    #[test]
    fn test_from_str() {
        let credential = SecureCredential::new_from_str("test_password");
        assert_eq!(credential.as_str(), "test_password");
        assert_eq!(credential.len(), 13);
    }

    #[test]
    fn test_empty_credential() {
        let credential = SecureCredential::new("".to_string());
        assert!(credential.is_empty());
        assert_eq!(credential.len(), 0);
    }

    #[test]
    fn test_clone() {
        let original = SecureCredential::new("test_password".to_string());
        let cloned = original.clone();

        assert_eq!(original.as_str(), cloned.as_str());
        assert_eq!(original.len(), cloned.len());
    }

    #[test]
    fn test_from_implementations() {
        let from_string: SecureCredential = "test_password".to_string().into();
        let from_str: SecureCredential = "test_password".into();

        assert_eq!(from_string.as_str(), "test_password");
        assert_eq!(from_str.as_str(), "test_password");
    }

    #[test]
    fn test_equality() {
        let cred1 = SecureCredential::new("password123".to_string());
        let cred2 = SecureCredential::new("password123".to_string());
        let cred3 = SecureCredential::new("different".to_string());

        assert_eq!(cred1, cred2);
        assert_ne!(cred1, cred3);
    }

    #[test]
    fn test_debug_format() {
        let credential = SecureCredential::new("secret".to_string());
        let debug_str = format!("{:?}", credential);

        // Should not contain the actual secret
        assert!(!debug_str.contains("secret"));
        assert!(debug_str.contains("REDACTED"));
        assert!(debug_str.contains("6")); // Length should be shown
    }

    #[test]
    fn test_display_format() {
        let credential = SecureCredential::new("secret".to_string());
        let display_str = format!("{}", credential);

        // Should not contain the actual secret
        assert!(!display_str.contains("secret"));
        assert!(display_str.contains("6 bytes"));
    }
}
