//! Authentication error types
//!
//! Provides comprehensive error handling for authentication operations
//! using thiserror for proper error trait implementations.

use thiserror::Error;

/// Main authentication error type
#[derive(Debug, Error)]
pub enum AuthError {
    /// Storage initialization or operation failed
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),
    
    /// Network request failed
    #[error("Network error: {0}")]
    Network(#[from] NetworkError),
    
    /// Token validation or refresh failed
    #[error("Token error: {0}")]
    Token(#[from] TokenError),
    
    /// User validation failed
    #[error("Validation error: {0}")]
    Validation(#[from] ValidationError),
    
    /// Device authentication failed
    #[error("Device error: {0}")]
    Device(#[from] DeviceError),
    
    /// Permission check failed
    #[error("Permission denied: {0}")]
    Permission(String),
    
    /// Not authenticated
    #[error("Not authenticated")]
    NotAuthenticated,
    
    /// Generic internal error
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Storage-related errors
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Failed to initialize storage: {0}")]
    InitFailed(String),
    
    #[error("Failed to read from storage")]
    ReadFailed(#[source] std::io::Error),
    
    #[error("Failed to write to storage")]
    WriteFailed(#[source] std::io::Error),
    
    #[error("Encryption failed")]
    EncryptionFailed(#[source] anyhow::Error),
    
    #[error("Decryption failed")]
    DecryptionFailed(#[source] anyhow::Error),
    
    #[error("Key derivation failed")]
    KeyDerivationFailed,
    
    #[error("Corrupted storage data")]
    CorruptedData,
}

/// Network-related errors
#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("Request failed: {0}")]
    RequestFailed(String),
    
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
    
    #[error("Connection timeout")]
    Timeout,
    
    #[error("Server unavailable")]
    ServerUnavailable,
    
    #[error("Invalid credentials")]
    InvalidCredentials,
    
    #[error("Rate limit exceeded")]
    RateLimited,
}

/// Token-related errors
#[derive(Debug, Error)]
pub enum TokenError {
    #[error("Token expired")]
    Expired,
    
    #[error("Invalid token format")]
    InvalidFormat,
    
    #[error("Token refresh failed")]
    RefreshFailed,
    
    #[error("Token revoked")]
    Revoked,
}

/// Validation errors
#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("Invalid username: {0}")]
    InvalidUsername(String),
    
    #[error("Invalid password: {0}")]
    InvalidPassword(String),
    
    #[error("Invalid PIN: {0}")]
    InvalidPin(String),
    
    #[error("Invalid display name: {0}")]
    InvalidDisplayName(String),
    
    #[error("Invalid email: {0}")]
    InvalidEmail(String),
    
    #[error("Insufficient permissions")]
    InsufficientPermissions,
}

/// Device-related errors
#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("Device not registered")]
    NotRegistered,
    
    #[error("Device fingerprint mismatch")]
    FingerprintMismatch,
    
    #[error("Device limit exceeded")]
    LimitExceeded,
    
    #[error("Failed to generate device identity")]
    IdentityGenerationFailed,
}

/// Result type alias for authentication operations
pub type AuthResult<T> = Result<T, AuthError>;

/// Convert from anyhow errors
impl From<anyhow::Error> for AuthError {
    fn from(err: anyhow::Error) -> Self {
        AuthError::Internal(err.to_string())
    }
}

/// Convert from std::io errors
impl From<std::io::Error> for AuthError {
    fn from(err: std::io::Error) -> Self {
        AuthError::Storage(StorageError::ReadFailed(err))
    }
}

/// Convert from serde_json errors
impl From<serde_json::Error> for AuthError {
    fn from(err: serde_json::Error) -> Self {
        AuthError::Storage(StorageError::CorruptedData)
    }
}