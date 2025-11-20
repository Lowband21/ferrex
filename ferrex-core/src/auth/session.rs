//! Session management for authenticated users

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// User session with device tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSession {
    pub id: Uuid,
    pub user_id: Uuid,
    pub device_id: Uuid,
    pub session_token: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub revoked: bool,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl DeviceSession {
    /// Create a new session
    pub fn new(
        user_id: Uuid,
        device_id: Uuid,
        ip_address: Option<String>,
        user_agent: Option<String>,
        duration: Duration,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            user_id,
            device_id,
            session_token: generate_session_token(),
            created_at: now,
            expires_at: now + duration,
            last_activity: now,
            ip_address,
            user_agent,
            revoked: false,
            revoked_at: None,
        }
    }
    
    /// Check if the session is still valid
    pub fn is_valid(&self) -> bool {
        !self.revoked && self.expires_at > Utc::now()
    }
    
    /// Update last activity timestamp
    pub fn touch(&mut self) {
        self.last_activity = Utc::now();
    }
    
    /// Revoke the session
    pub fn revoke(&mut self) {
        self.revoked = true;
        self.revoked_at = Some(Utc::now());
    }
}

/// Session configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Duration for regular sessions
    pub session_duration: Duration,
    /// Duration for "remember me" sessions
    pub remember_duration: Duration,
    /// Whether to extend session on activity
    pub extend_on_activity: bool,
    /// Maximum concurrent sessions per user
    pub max_sessions_per_user: Option<usize>,
    /// Maximum concurrent sessions per device
    pub max_sessions_per_device: Option<usize>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            session_duration: Duration::hours(24),
            remember_duration: Duration::days(30),
            extend_on_activity: true,
            max_sessions_per_user: Some(10),
            max_sessions_per_device: Some(1),
        }
    }
}

/// Generate a cryptographically secure session token
pub fn generate_session_token() -> String {
    use rand::{thread_rng, Rng};
    use rand::distributions::Alphanumeric;
    
    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(64)
        .map(char::from)
        .collect()
}

// Note: Session token hashing is implemented in the server module
// to have access to proper cryptographic dependencies.

/// Session creation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub user_id: Uuid,
    pub device_id: Uuid,
    pub remember_me: bool,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

/// Session creation response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionResponse {
    pub session: DeviceSession,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u32,
}

/// Session validation result
#[derive(Debug, Clone)]
pub enum SessionValidationResult {
    /// Session is valid and active
    Valid(DeviceSession),
    /// Session has expired
    Expired,
    /// Session was revoked
    Revoked,
    /// Session not found
    NotFound,
}

/// Session activity update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionActivity {
    pub session_id: Uuid,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// Session revocation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevokeSessionRequest {
    pub session_id: Uuid,
    pub revoked_by: Uuid,
    pub reason: Option<String>,
}

/// List sessions request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSessionsRequest {
    pub user_id: Option<Uuid>,
    pub device_id: Option<Uuid>,
    pub include_expired: bool,
    pub include_revoked: bool,
}

/// Session summary for user display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: Uuid,
    pub device_name: String,
    pub platform: String,
    pub last_activity: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub ip_address: Option<String>,
    pub is_current: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_session_creation() {
        let session = DeviceSession::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Some("192.168.1.1".to_string()),
            Some("Mozilla/5.0".to_string()),
            Duration::hours(24),
        );
        
        assert!(session.is_valid());
        assert_eq!(session.session_token.len(), 64);
    }
    
    #[test]
    fn test_session_expiration() {
        let mut session = DeviceSession::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            None,
            None,
            Duration::hours(24),
        );
        
        // Manually set expiration to past
        session.expires_at = Utc::now() - Duration::hours(1);
        assert!(!session.is_valid());
    }
    
    #[test]
    fn test_session_revocation() {
        let mut session = DeviceSession::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            None,
            None,
            Duration::hours(24),
        );
        
        assert!(session.is_valid());
        session.revoke();
        assert!(!session.is_valid());
        assert!(session.revoked_at.is_some());
    }
}