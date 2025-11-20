//! Device identity and registration for secure device-based authentication

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Platform types for device identification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Platform {
    #[serde(rename = "macos")]
    MacOS,
    #[serde(rename = "linux")]
    Linux,
    #[serde(rename = "windows")]
    Windows,
    #[serde(rename = "ios")]
    IOS,
    #[serde(rename = "android")]
    Android,
    #[serde(rename = "tvos")]
    TvOS,
    #[serde(rename = "web")]
    Web,
    #[serde(rename = "unknown")]
    Unknown,
}

impl Platform {
    pub fn from_user_agent(user_agent: &str) -> Self {
        let ua = user_agent.to_lowercase();
        if ua.contains("mac") || ua.contains("darwin") {
            Platform::MacOS
        } else if ua.contains("linux") {
            Platform::Linux
        } else if ua.contains("windows") || ua.contains("win32") || ua.contains("win64") {
            Platform::Windows
        } else if ua.contains("iphone") || ua.contains("ipad") && !ua.contains("mac") {
            Platform::IOS
        } else if ua.contains("android") {
            Platform::Android
        } else if ua.contains("appletv") || ua.contains("tvos") {
            Platform::TvOS
        } else if ua.contains("mozilla") || ua.contains("chrome") || ua.contains("safari") {
            Platform::Web
        } else {
            Platform::Unknown
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "macos" => Platform::MacOS,
            "linux" => Platform::Linux,
            "windows" => Platform::Windows,
            "ios" => Platform::IOS,
            "android" => Platform::Android,
            "tvos" => Platform::TvOS,
            "web" => Platform::Web,
            _ => Platform::Unknown,
        }
    }
}

impl AsRef<str> for Platform {
    fn as_ref(&self) -> &str {
        match self {
            Platform::MacOS => "macos",
            Platform::Linux => "linux",
            Platform::Windows => "windows",
            Platform::IOS => "ios",
            Platform::Android => "android",
            Platform::TvOS => "tvos",
            Platform::Web => "web",
            Platform::Unknown => "unknown",
        }
    }
}

/// Device trust lifecycle status mirroring the database enum
#[cfg_attr(feature = "database", derive(sqlx::Type))]
#[cfg_attr(
    feature = "database",
    sqlx(type_name = "auth_device_status", rename_all = "lowercase")
)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuthDeviceStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "trusted")]
    Trusted,
    #[serde(rename = "revoked")]
    Revoked,
}

impl AuthDeviceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthDeviceStatus::Pending => "pending",
            AuthDeviceStatus::Trusted => "trusted",
            AuthDeviceStatus::Revoked => "revoked",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "trusted" => Some(Self::Trusted),
            "revoked" => Some(Self::Revoked),
            _ => None,
        }
    }
}

impl Default for AuthDeviceStatus {
    fn default() -> Self {
        Self::Pending
    }
}

/// Device information sent during authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Unique device identifier (generated on first launch, stored locally)
    pub device_id: Uuid,
    /// User-friendly device name
    pub device_name: String,
    /// Platform type
    pub platform: Platform,
    /// Application version
    pub app_version: String,
    /// Optional hardware identifier (for additional validation)
    pub hardware_id: Option<String>,
}

/// Device registration stored in database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceRegistration {
    pub id: Uuid,
    pub user_id: Uuid,
    pub device_id: Uuid,
    pub device_name: String,
    pub platform: Platform,
    pub app_version: String,
    /// Cryptographically secure trust token
    pub trust_token: String,
    /// Optional PIN hash for this device-user combination
    pub pin_hash: Option<String>,
    /// When the device was first registered
    pub registered_at: DateTime<Utc>,
    /// When the device was last used
    pub last_used_at: DateTime<Utc>,
    /// When the device trust expires (None = never)
    pub expires_at: Option<DateTime<Utc>>,
    /// Whether the device has been revoked
    pub revoked: bool,
    /// Who revoked the device
    pub revoked_by: Option<Uuid>,
    /// When the device was revoked
    pub revoked_at: Option<DateTime<Utc>>,
}

impl DeviceRegistration {
    /// Check if the device registration is still valid
    pub fn is_valid(&self) -> bool {
        if self.revoked {
            return false;
        }

        if let Some(expires_at) = self.expires_at
            && expires_at < Utc::now()
        {
            return false;
        }

        true
    }

    /// Check if PIN is required for this device
    pub fn requires_pin(&self) -> bool {
        self.pin_hash.is_some()
    }
}

/// Request to register a new device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterDeviceRequest {
    pub device_info: DeviceInfo,
    /// Whether to remember this device (longer trust period)
    pub remember_device: bool,
}

/// Response from device registration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterDeviceResponse {
    pub device_registration: DeviceRegistration,
    pub trust_token: String,
}

/// Request for device-based authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAuthRequest {
    pub device_id: Uuid,
    pub user_id: Uuid,
    pub trust_token: String,
    pub pin: Option<String>,
}

/// Device check result
#[derive(Debug, Clone)]
pub enum DeviceCheckResult {
    /// Device is trusted and can be used for authentication
    Trusted(DeviceRegistration),
    /// Device is not registered for this user
    NotRegistered,
    /// Device was revoked
    Revoked,
    /// Device trust has expired
    Expired,
}

/// Generate a cryptographically secure trust token
pub fn generate_trust_token() -> String {
    use rand::distr::Alphanumeric;
    use rand::{Rng, rng};

    rng()
        .sample_iter(&Alphanumeric)
        .take(64)
        .map(char::from)
        .collect()
}

// Note: Device salt derivation is implemented in the server module
// to have access to proper cryptographic dependencies.

// Legacy device-user credential removed; user-level PIN is canonical.

/// Authenticated device record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticatedDevice {
    pub id: Uuid,
    pub user_id: Uuid,
    pub fingerprint: String,
    pub name: String,
    pub platform: Platform,
    pub app_version: Option<String>,
    pub hardware_id: Option<String>,
    pub status: AuthDeviceStatus,
    pub pin_hash: Option<String>,
    pub pin_set_at: Option<DateTime<Utc>>,
    pub pin_last_used_at: Option<DateTime<Utc>>,
    pub failed_attempts: i32,
    pub locked_until: Option<DateTime<Utc>>,
    pub first_authenticated_by: Uuid,
    pub first_authenticated_at: DateTime<Utc>,
    pub trusted_until: Option<DateTime<Utc>>,
    pub last_seen_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub auto_login_enabled: bool,
    pub revoked_by: Option<Uuid>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revoked_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: serde_json::Value,
}

impl AuthenticatedDevice {
    pub fn is_revoked(&self) -> bool {
        self.status == AuthDeviceStatus::Revoked
    }

    pub fn is_trusted(&self) -> bool {
        matches!(self.status, AuthDeviceStatus::Trusted)
    }
}

/// Parameters for updating a device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceUpdateParams {
    pub name: Option<String>,
    pub app_version: Option<String>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub trusted_until: Option<DateTime<Utc>>,
    pub last_activity: Option<DateTime<Utc>>,
    pub status: Option<AuthDeviceStatus>,
    pub auto_login_enabled: Option<bool>,
    pub locked_until: Option<Option<DateTime<Utc>>>,
    pub revoked_by: Option<Option<Uuid>>,
    pub revoked_reason: Option<Option<String>>,
    pub revoked_at: Option<Option<DateTime<Utc>>>,
}

impl Default for DeviceUpdateParams {
    fn default() -> Self {
        Self {
            name: None,
            app_version: None,
            last_seen_at: None,
            trusted_until: None,
            last_activity: None,
            status: None,
            auto_login_enabled: None,
            locked_until: None,
            revoked_by: None,
            revoked_reason: None,
            revoked_at: None,
        }
    }
}
