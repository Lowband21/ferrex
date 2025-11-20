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
        } else if ua.contains("windows")
            || ua.contains("win32")
            || ua.contains("win64")
        {
            Platform::Windows
        } else if ua.contains("iphone")
            || ua.contains("ipad") && !ua.contains("mac")
        {
            Platform::IOS
        } else if ua.contains("android") {
            Platform::Android
        } else if ua.contains("appletv") || ua.contains("tvos") {
            Platform::TvOS
        } else if ua.contains("mozilla")
            || ua.contains("chrome")
            || ua.contains("safari")
        {
            Platform::Web
        } else {
            Platform::Unknown
        }
    }
}

impl std::str::FromStr for Platform {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Platform, anyhow::Error> {
        match value {
            "macos" => Ok(Platform::MacOS),
            "linux" => Ok(Platform::Linux),
            "windows" => Ok(Platform::Windows),
            "ios" => Ok(Platform::IOS),
            "android" => Ok(Platform::Android),
            "tvos" => Ok(Platform::TvOS),
            "web" => Ok(Platform::Web),
            _ => Err(anyhow::Error::msg("Unknown platform")),
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
#[derive(
    Default, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq,
)]
pub enum AuthDeviceStatus {
    #[serde(rename = "pending")]
    #[default]
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
}

impl std::str::FromStr for AuthDeviceStatus {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<AuthDeviceStatus, anyhow::Error> {
        match value {
            "pending" => Ok(Self::Pending),
            "trusted" => Ok(Self::Trusted),
            "revoked" => Ok(Self::Revoked),
            _ => Err(anyhow::Error::msg("Invalid auth device status")),
        }
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
    /// Indicates whether the user had a PIN configured when this device was registered
    pub pin_configured: bool,
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
        self.pin_configured
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
}

/// Request for device-based authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAuthRequest {
    pub device_id: Uuid,
    pub user_id: Uuid,
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
    pub pin_configured: bool,
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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
