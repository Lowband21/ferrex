//! Authentication DTOs shared at the player API boundary.

use ferrex_core::{
    domain::users::auth::device::DeviceInfo,
    player_prelude::{User, UserPermissions},
};
use serde::Serialize;
use uuid::Uuid;

pub use ferrex_player_foundation::auth::{
    DeviceAuthStatus, DeviceTrustPolicyResponse, PasswordPolicyResponse,
    PinPolicyResponse, SetupStatus,
};

/// Authenticated player identity and permission state returned by login flows.
#[derive(Debug, Clone)]
pub struct PlayerAuthResult {
    pub user: User,
    pub permissions: UserPermissions,
    pub device_has_pin: bool,
}

/// Scope for updating remember-device/auto-login preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoLoginScope {
    /// Only update device-local state (trust record, cache).
    DeviceOnly,
    /// Update both device-local state and the user-wide server preference.
    UserDefault,
}

/// Device login request accepted by the server auth API.
#[derive(Debug, Serialize)]
pub struct DeviceLoginRequest {
    pub username: String,
    pub password: String,
    pub device_info: Option<DeviceInfo>,
    pub remember_device: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_public_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_key_alg: Option<String>,
}

/// PIN login request accepted by the server auth API.
#[derive(Debug, Serialize)]
pub struct PinLoginRequest {
    /// Server-side device session id returned by `/auth/device/login`.
    pub device_id: Uuid,
    /// Client-derived PIN proof (PHC string).
    pub client_proof: String,
    pub challenge_id: Uuid,
    pub device_signature: String,
}

/// Device PIN setup/change request accepted by the server auth API.
#[derive(Debug, Serialize)]
pub struct SetPinRequest {
    /// Server-side device session id returned by `/auth/device/login`.
    pub device_id: Uuid,
    /// Client-derived PIN proof (PHC string).
    pub client_proof: String,
    pub challenge_id: Uuid,
    pub device_signature: String,
}
