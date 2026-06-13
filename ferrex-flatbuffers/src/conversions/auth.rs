//! Conversions for authentication and setup FlatBuffers payloads.

use chrono::{DateTime, Utc};
use flatbuffers::{FlatBufferBuilder, WIPOffset};
use uuid::Uuid;

use crate::conversions::common::{option_timestamp_to_fb, timestamp_to_fb};
use crate::fb::auth as fb;
use crate::uuid_helpers::{fb_to_uuid, uuid_to_fb};

/// Session scope attached to an authentication token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionScope {
    /// Full access session created after password authentication.
    #[default]
    Full,
    /// Reduced-trust playback-only session.
    Playback,
}

impl From<SessionScope> for fb::SessionScope {
    fn from(scope: SessionScope) -> Self {
        match scope {
            SessionScope::Full => Self::Full,
            SessionScope::Playback => Self::Playback,
        }
    }
}

impl From<fb::SessionScope> for SessionScope {
    fn from(scope: fb::SessionScope) -> Self {
        if scope == fb::SessionScope::Playback {
            Self::Playback
        } else {
            Self::Full
        }
    }
}

/// Device platform values matching backend JSON semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Platform {
    /// Unknown or unsupported platform.
    #[default]
    Unknown,
    MacOS,
    Linux,
    Windows,
    IOS,
    Android,
    TvOS,
    Web,
}

impl Platform {
    /// Lowercase string used by the JSON API for this platform.
    pub fn as_json_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::MacOS => "macos",
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::IOS => "ios",
            Self::Android => "android",
            Self::TvOS => "tvos",
            Self::Web => "web",
        }
    }
}

impl From<Platform> for fb::Platform {
    fn from(platform: Platform) -> Self {
        match platform {
            Platform::Unknown => Self::Unknown,
            Platform::MacOS => Self::MacOS,
            Platform::Linux => Self::Linux,
            Platform::Windows => Self::Windows,
            Platform::IOS => Self::IOS,
            Platform::Android => Self::Android,
            Platform::TvOS => Self::TvOS,
            Platform::Web => Self::Web,
        }
    }
}

impl From<fb::Platform> for Platform {
    fn from(platform: fb::Platform) -> Self {
        if platform == fb::Platform::MacOS {
            Self::MacOS
        } else if platform == fb::Platform::Linux {
            Self::Linux
        } else if platform == fb::Platform::Windows {
            Self::Windows
        } else if platform == fb::Platform::IOS {
            Self::IOS
        } else if platform == fb::Platform::Android {
            Self::Android
        } else if platform == fb::Platform::TvOS {
            Self::TvOS
        } else if platform == fb::Platform::Web {
            Self::Web
        } else {
            Self::Unknown
        }
    }
}

/// Trust lifecycle status for an authenticated device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuthDeviceStatus {
    #[default]
    Pending,
    Trusted,
    Revoked,
}

impl AuthDeviceStatus {
    /// Lowercase string used by the JSON API for this status.
    pub fn as_json_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Trusted => "trusted",
            Self::Revoked => "revoked",
        }
    }
}

impl From<AuthDeviceStatus> for fb::AuthDeviceStatus {
    fn from(status: AuthDeviceStatus) -> Self {
        match status {
            AuthDeviceStatus::Pending => Self::Pending,
            AuthDeviceStatus::Trusted => Self::Trusted,
            AuthDeviceStatus::Revoked => Self::Revoked,
        }
    }
}

impl From<fb::AuthDeviceStatus> for AuthDeviceStatus {
    fn from(status: fb::AuthDeviceStatus) -> Self {
        if status == fb::AuthDeviceStatus::Trusted {
            Self::Trusted
        } else if status == fb::AuthDeviceStatus::Revoked {
            Self::Revoked
        } else {
            Self::Pending
        }
    }
}

/// Borrowed authentication token payload.
#[derive(Debug, Clone, Copy)]
pub struct AuthToken<'a> {
    pub access_token: &'a str,
    pub refresh_token: &'a str,
    pub expires_in: u32,
    pub session_id: Option<Uuid>,
    pub device_session_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub scope: SessionScope,
}

/// Device information sent during device login/profile lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub device_id: Uuid,
    pub device_name: String,
    pub platform: Platform,
    pub app_version: String,
    pub hardware_id: Option<String>,
}

/// Device password login request decoded from FlatBuffers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceLoginRequest {
    pub username: String,
    pub password: String,
    pub device_info: Option<DeviceInfo>,
    pub remember_device: bool,
    pub device_public_key: Option<String>,
    pub device_key_alg: Option<String>,
}

/// Device registration summary returned with device password/PIN login.
#[derive(Debug, Clone, Copy)]
pub struct DeviceRegistration<'a> {
    pub id: Uuid,
    pub user_id: Uuid,
    pub device_id: Uuid,
    pub device_name: &'a str,
    pub platform: Platform,
    pub app_version: &'a str,
    pub pin_configured: bool,
    pub registered_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked: bool,
    pub revoked_by: Option<Uuid>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// AuthToken-shaped response returned by device password login and PIN login.
#[derive(Debug, Clone, Copy)]
pub struct DeviceLoginResponse<'a> {
    pub access_token: &'a str,
    pub session_token: &'a str,
    pub refresh_token: &'a str,
    pub expires_in: u32,
    pub session_id: Option<Uuid>,
    pub device_session_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub scope: SessionScope,
    pub device_registration: Option<DeviceRegistration<'a>>,
    pub requires_pin_setup: bool,
}

/// PIN challenge request decoded from FlatBuffers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinChallengeRequest {
    pub device_id: Uuid,
}

/// PIN challenge response payload.
#[derive(Debug, Clone, Copy)]
pub struct PinChallengeResponse<'a> {
    pub challenge_id: Uuid,
    pub nonce: &'a str,
    pub expires_in_secs: i64,
    pub pin_salt: &'a str,
}

/// PIN login request decoded from FlatBuffers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinLoginRequest {
    pub device_id: Uuid,
    pub client_proof: String,
    pub challenge_id: Uuid,
    pub device_signature: String,
}

/// Device auth status response payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceAuthStatus {
    pub device_registered: bool,
    pub has_pin: bool,
    pub remaining_attempts: Option<u8>,
}

/// Authenticated-device management summary.
#[derive(Debug, Clone, Copy)]
pub struct AuthenticatedDevice<'a> {
    pub id: Uuid,
    pub user_id: Uuid,
    pub fingerprint: &'a str,
    pub name: &'a str,
    pub platform: Platform,
    pub app_version: Option<&'a str>,
    pub hardware_id: Option<&'a str>,
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
    pub revoked_reason: Option<&'a str>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Known-device profile lookup request decoded from FlatBuffers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownDeviceProfilesRequest {
    pub device_info: Option<DeviceInfo>,
}

/// Minimal pre-auth user card for known-device profile selection.
#[derive(Debug, Clone, Copy)]
pub struct KnownDeviceUserCard<'a> {
    pub id: Uuid,
    pub username: &'a str,
    pub display_name: &'a str,
    pub avatar_url: Option<&'a str>,
    pub has_pin: bool,
}

/// Password policy fields exposed during setup and security settings flows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasswordPolicy {
    pub enforce: bool,
    pub min_length: u16,
    pub require_uppercase: bool,
    pub require_lowercase: bool,
    pub require_number: bool,
    pub require_special: bool,
}

/// PIN policy fields exposed to clients before deriving PIN proofs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinPolicy {
    pub min_length: u16,
    pub max_length: u16,
    pub require_numeric: bool,
    pub reject_repeated_digits: bool,
    pub max_consecutive_identical: u16,
    pub reject_sequential_digits: bool,
}

/// Device trust and remember-device policy fields exposed to clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceTrustPolicy {
    pub remember_device_default: bool,
    pub trust_duration_days: u16,
    pub pin_max_attempts: u8,
    pub pin_lockout_minutes: u16,
    pub admin_pin_unlock_enabled: bool,
}

/// Server setup status payload.
#[derive(Debug, Clone, Copy)]
pub struct SetupStatus {
    pub needs_setup: bool,
    pub has_admin: bool,
    pub requires_setup_token: bool,
    pub user_count: u32,
    pub library_count: u32,
    pub admin_password_policy: Option<PasswordPolicy>,
    pub user_password_policy: Option<PasswordPolicy>,
    pub pin_policy: Option<PinPolicy>,
    pub device_trust_policy: Option<DeviceTrustPolicy>,
}

/// Current-user profile payload.
#[derive(Debug, Clone, Copy)]
pub struct UserProfile<'a> {
    pub id: Uuid,
    pub username: &'a str,
    pub display_name: &'a str,
    pub avatar_url: Option<&'a str>,
    pub email: Option<&'a str>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_login: Option<DateTime<Utc>>,
    pub is_active: bool,
}

/// Owned login request decoded from FlatBuffers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    pub device_name: Option<String>,
}

/// Owned refresh request decoded from FlatBuffers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

/// Parse a FlatBuffers `LoginRequest` request body.
pub fn parse_login_request(
    bytes: &[u8],
) -> Result<LoginRequest, flatbuffers::InvalidFlatbuffer> {
    let request = flatbuffers::root::<fb::LoginRequest>(bytes)?;

    Ok(LoginRequest {
        username: request.username().to_string(),
        password: request.password().to_string(),
        device_name: request.device_name().map(ToOwned::to_owned),
    })
}

/// Parse a FlatBuffers `DeviceLoginRequest` request body.
pub fn parse_device_login_request(
    bytes: &[u8],
) -> Result<DeviceLoginRequest, flatbuffers::InvalidFlatbuffer> {
    let request = flatbuffers::root::<fb::DeviceLoginRequest>(bytes)?;

    Ok(DeviceLoginRequest {
        username: request.username().to_string(),
        password: request.password().to_string(),
        device_info: request.device_info().map(device_info_from_fb),
        remember_device: request.remember_device(),
        device_public_key: request.device_public_key().map(ToOwned::to_owned),
        device_key_alg: request.device_key_alg().map(ToOwned::to_owned),
    })
}

/// Parse a FlatBuffers `PinChallengeRequest` request body.
pub fn parse_pin_challenge_request(
    bytes: &[u8],
) -> Result<PinChallengeRequest, flatbuffers::InvalidFlatbuffer> {
    let request = flatbuffers::root::<fb::PinChallengeRequest>(bytes)?;

    Ok(PinChallengeRequest {
        device_id: fb_to_uuid(request.device_id()),
    })
}

/// Parse a FlatBuffers `PinLoginRequest` request body.
pub fn parse_pin_login_request(
    bytes: &[u8],
) -> Result<PinLoginRequest, flatbuffers::InvalidFlatbuffer> {
    let request = flatbuffers::root::<fb::PinLoginRequest>(bytes)?;

    Ok(PinLoginRequest {
        device_id: fb_to_uuid(request.device_id()),
        client_proof: request.client_proof().to_string(),
        challenge_id: fb_to_uuid(request.challenge_id()),
        device_signature: request.device_signature().to_string(),
    })
}

/// Parse a FlatBuffers `KnownDeviceProfilesRequest` request body.
pub fn parse_known_device_profiles_request(
    bytes: &[u8],
) -> Result<KnownDeviceProfilesRequest, flatbuffers::InvalidFlatbuffer> {
    let request = flatbuffers::root::<fb::KnownDeviceProfilesRequest>(bytes)?;

    Ok(KnownDeviceProfilesRequest {
        device_info: request.device_info().map(device_info_from_fb),
    })
}

/// Parse a FlatBuffers `RefreshRequest` request body.
pub fn parse_refresh_request(
    bytes: &[u8],
) -> Result<RefreshRequest, flatbuffers::InvalidFlatbuffer> {
    let request = flatbuffers::root::<fb::RefreshRequest>(bytes)?;

    Ok(RefreshRequest {
        refresh_token: request.refresh_token().to_string(),
    })
}

fn device_info_from_fb(info: fb::DeviceInfo<'_>) -> DeviceInfo {
    DeviceInfo {
        device_id: fb_to_uuid(info.device_id()),
        device_name: info.device_name().to_string(),
        platform: info.platform().into(),
        app_version: info.app_version().to_string(),
        hardware_id: info.hardware_id().map(ToOwned::to_owned),
    }
}

fn string_opt<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    value: Option<&str>,
) -> Option<flatbuffers::WIPOffset<&'a str>> {
    value.map(|value| builder.create_string(value))
}

/// Build a FlatBuffers `AuthToken` table.
pub fn build_auth_token<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    token: &AuthToken<'_>,
) -> WIPOffset<fb::AuthToken<'a>> {
    let access_token = builder.create_string(token.access_token);
    let refresh_token = builder.create_string(token.refresh_token);
    let session_id = token.session_id.as_ref().map(uuid_to_fb);
    let device_session_id = token.device_session_id.as_ref().map(uuid_to_fb);
    let user_id = token.user_id.as_ref().map(uuid_to_fb);

    fb::AuthToken::create(
        builder,
        &fb::AuthTokenArgs {
            access_token: Some(access_token),
            refresh_token: Some(refresh_token),
            expires_in: token.expires_in,
            session_id: session_id.as_ref(),
            device_session_id: device_session_id.as_ref(),
            user_id: user_id.as_ref(),
            scope: token.scope.into(),
        },
    )
}

/// Serialize an `AuthToken` into root FlatBuffers bytes.
pub fn serialize_auth_token(token: &AuthToken<'_>) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::with_capacity(256);
    let token = build_auth_token(&mut builder, token);
    builder.finish(token, None);
    builder.finished_data().to_vec()
}

/// Build a FlatBuffers `DeviceInfo` table.
pub fn build_device_info<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    info: &DeviceInfo,
) -> WIPOffset<fb::DeviceInfo<'a>> {
    let device_id = uuid_to_fb(&info.device_id);
    let device_name = builder.create_string(&info.device_name);
    let app_version = builder.create_string(&info.app_version);
    let hardware_id = string_opt(builder, info.hardware_id.as_deref());

    fb::DeviceInfo::create(
        builder,
        &fb::DeviceInfoArgs {
            device_id: Some(&device_id),
            device_name: Some(device_name),
            platform: info.platform.into(),
            app_version: Some(app_version),
            hardware_id,
        },
    )
}

/// Build a FlatBuffers `DeviceLoginRequest` table.
pub fn build_device_login_request<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    request: &DeviceLoginRequest,
) -> WIPOffset<fb::DeviceLoginRequest<'a>> {
    let username = builder.create_string(&request.username);
    let password = builder.create_string(&request.password);
    let device_info = request
        .device_info
        .as_ref()
        .map(|info| build_device_info(builder, info));
    let device_public_key =
        string_opt(builder, request.device_public_key.as_deref());
    let device_key_alg = string_opt(builder, request.device_key_alg.as_deref());

    fb::DeviceLoginRequest::create(
        builder,
        &fb::DeviceLoginRequestArgs {
            username: Some(username),
            password: Some(password),
            device_info,
            remember_device: request.remember_device,
            device_public_key,
            device_key_alg,
        },
    )
}

/// Serialize a `DeviceLoginRequest` into root FlatBuffers bytes.
pub fn serialize_device_login_request(request: &DeviceLoginRequest) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::with_capacity(512);
    let request = build_device_login_request(&mut builder, request);
    builder.finish(request, None);
    builder.finished_data().to_vec()
}

/// Build a FlatBuffers `DeviceRegistration` table.
pub fn build_device_registration<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    registration: &DeviceRegistration<'_>,
) -> WIPOffset<fb::DeviceRegistration<'a>> {
    let id = uuid_to_fb(&registration.id);
    let user_id = uuid_to_fb(&registration.user_id);
    let device_id = uuid_to_fb(&registration.device_id);
    let device_name = builder.create_string(registration.device_name);
    let app_version = builder.create_string(registration.app_version);
    let registered_at = timestamp_to_fb(&registration.registered_at);
    let last_used_at = timestamp_to_fb(&registration.last_used_at);
    let expires_at = registration.expires_at.as_ref().map(timestamp_to_fb);
    let revoked_by = registration.revoked_by.as_ref().map(uuid_to_fb);
    let revoked_at = registration.revoked_at.as_ref().map(timestamp_to_fb);

    fb::DeviceRegistration::create(
        builder,
        &fb::DeviceRegistrationArgs {
            id: Some(&id),
            user_id: Some(&user_id),
            device_id: Some(&device_id),
            device_name: Some(device_name),
            platform: registration.platform.into(),
            app_version: Some(app_version),
            pin_configured: registration.pin_configured,
            registered_at: Some(&registered_at),
            last_used_at: Some(&last_used_at),
            expires_at: expires_at.as_ref(),
            revoked: registration.revoked,
            revoked_by: revoked_by.as_ref(),
            revoked_at: revoked_at.as_ref(),
        },
    )
}

/// Build a FlatBuffers `DeviceLoginResponse` table.
pub fn build_device_login_response<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    response: &DeviceLoginResponse<'_>,
) -> WIPOffset<fb::DeviceLoginResponse<'a>> {
    let access_token = builder.create_string(response.access_token);
    let session_token = builder.create_string(response.session_token);
    let refresh_token = builder.create_string(response.refresh_token);
    let session_id = response.session_id.as_ref().map(uuid_to_fb);
    let device_session_id = response.device_session_id.as_ref().map(uuid_to_fb);
    let user_id = response.user_id.as_ref().map(uuid_to_fb);
    let device_registration = response
        .device_registration
        .as_ref()
        .map(|registration| build_device_registration(builder, registration));

    fb::DeviceLoginResponse::create(
        builder,
        &fb::DeviceLoginResponseArgs {
            access_token: Some(access_token),
            session_token: Some(session_token),
            refresh_token: Some(refresh_token),
            expires_in: response.expires_in,
            session_id: session_id.as_ref(),
            device_session_id: device_session_id.as_ref(),
            user_id: user_id.as_ref(),
            scope: response.scope.into(),
            device_registration,
            requires_pin_setup: response.requires_pin_setup,
        },
    )
}

/// Serialize a `DeviceLoginResponse` into root FlatBuffers bytes.
pub fn serialize_device_login_response(
    response: &DeviceLoginResponse<'_>,
) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::with_capacity(512);
    let response = build_device_login_response(&mut builder, response);
    builder.finish(response, None);
    builder.finished_data().to_vec()
}

/// Build a FlatBuffers `PinChallengeRequest` table.
pub fn build_pin_challenge_request<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    request: &PinChallengeRequest,
) -> WIPOffset<fb::PinChallengeRequest<'a>> {
    let device_id = uuid_to_fb(&request.device_id);

    fb::PinChallengeRequest::create(
        builder,
        &fb::PinChallengeRequestArgs {
            device_id: Some(&device_id),
        },
    )
}

/// Serialize a `PinChallengeRequest` into root FlatBuffers bytes.
pub fn serialize_pin_challenge_request(
    request: &PinChallengeRequest,
) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::with_capacity(128);
    let request = build_pin_challenge_request(&mut builder, request);
    builder.finish(request, None);
    builder.finished_data().to_vec()
}

/// Build a FlatBuffers `PinChallengeResponse` table.
pub fn build_pin_challenge_response<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    response: &PinChallengeResponse<'_>,
) -> WIPOffset<fb::PinChallengeResponse<'a>> {
    let challenge_id = uuid_to_fb(&response.challenge_id);
    let nonce = builder.create_string(response.nonce);
    let pin_salt = builder.create_string(response.pin_salt);

    fb::PinChallengeResponse::create(
        builder,
        &fb::PinChallengeResponseArgs {
            challenge_id: Some(&challenge_id),
            nonce: Some(nonce),
            expires_in_secs: response.expires_in_secs,
            pin_salt: Some(pin_salt),
        },
    )
}

/// Serialize a `PinChallengeResponse` into root FlatBuffers bytes.
pub fn serialize_pin_challenge_response(
    response: &PinChallengeResponse<'_>,
) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::with_capacity(256);
    let response = build_pin_challenge_response(&mut builder, response);
    builder.finish(response, None);
    builder.finished_data().to_vec()
}

/// Build a FlatBuffers `PinLoginRequest` table.
pub fn build_pin_login_request<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    request: &PinLoginRequest,
) -> WIPOffset<fb::PinLoginRequest<'a>> {
    let device_id = uuid_to_fb(&request.device_id);
    let client_proof = builder.create_string(&request.client_proof);
    let challenge_id = uuid_to_fb(&request.challenge_id);
    let device_signature = builder.create_string(&request.device_signature);

    fb::PinLoginRequest::create(
        builder,
        &fb::PinLoginRequestArgs {
            device_id: Some(&device_id),
            client_proof: Some(client_proof),
            challenge_id: Some(&challenge_id),
            device_signature: Some(device_signature),
        },
    )
}

/// Serialize a `PinLoginRequest` into root FlatBuffers bytes.
pub fn serialize_pin_login_request(request: &PinLoginRequest) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::with_capacity(256);
    let request = build_pin_login_request(&mut builder, request);
    builder.finish(request, None);
    builder.finished_data().to_vec()
}

/// Build a FlatBuffers `DeviceAuthStatus` table.
pub fn build_device_auth_status<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    status: &DeviceAuthStatus,
) -> WIPOffset<fb::DeviceAuthStatus<'a>> {
    fb::DeviceAuthStatus::create(
        builder,
        &fb::DeviceAuthStatusArgs {
            device_registered: status.device_registered,
            has_pin: status.has_pin,
            remaining_attempts: status.remaining_attempts,
        },
    )
}

/// Serialize a `DeviceAuthStatus` into root FlatBuffers bytes.
pub fn serialize_device_auth_status(status: &DeviceAuthStatus) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::with_capacity(128);
    let status = build_device_auth_status(&mut builder, status);
    builder.finish(status, None);
    builder.finished_data().to_vec()
}

/// Build a FlatBuffers `AuthenticatedDevice` table.
pub fn build_authenticated_device<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    device: &AuthenticatedDevice<'_>,
) -> WIPOffset<fb::AuthenticatedDevice<'a>> {
    let id = uuid_to_fb(&device.id);
    let user_id = uuid_to_fb(&device.user_id);
    let fingerprint = builder.create_string(device.fingerprint);
    let name = builder.create_string(device.name);
    let app_version = string_opt(builder, device.app_version);
    let hardware_id = string_opt(builder, device.hardware_id);
    let locked_until = device.locked_until.as_ref().map(timestamp_to_fb);
    let first_authenticated_by = uuid_to_fb(&device.first_authenticated_by);
    let first_authenticated_at =
        timestamp_to_fb(&device.first_authenticated_at);
    let trusted_until = device.trusted_until.as_ref().map(timestamp_to_fb);
    let last_seen_at = timestamp_to_fb(&device.last_seen_at);
    let last_activity = timestamp_to_fb(&device.last_activity);
    let revoked_by = device.revoked_by.as_ref().map(uuid_to_fb);
    let revoked_at = device.revoked_at.as_ref().map(timestamp_to_fb);
    let revoked_reason = string_opt(builder, device.revoked_reason);
    let created_at = timestamp_to_fb(&device.created_at);
    let updated_at = timestamp_to_fb(&device.updated_at);

    fb::AuthenticatedDevice::create(
        builder,
        &fb::AuthenticatedDeviceArgs {
            id: Some(&id),
            user_id: Some(&user_id),
            fingerprint: Some(fingerprint),
            name: Some(name),
            platform: device.platform.into(),
            app_version,
            hardware_id,
            status: device.status.into(),
            pin_configured: device.pin_configured,
            failed_attempts: device.failed_attempts,
            locked_until: locked_until.as_ref(),
            first_authenticated_by: Some(&first_authenticated_by),
            first_authenticated_at: Some(&first_authenticated_at),
            trusted_until: trusted_until.as_ref(),
            last_seen_at: Some(&last_seen_at),
            last_activity: Some(&last_activity),
            auto_login_enabled: device.auto_login_enabled,
            revoked_by: revoked_by.as_ref(),
            revoked_at: revoked_at.as_ref(),
            revoked_reason,
            created_at: Some(&created_at),
            updated_at: Some(&updated_at),
        },
    )
}

/// Serialize authenticated-device summaries into a response buffer.
pub fn serialize_authenticated_devices(
    devices: &[AuthenticatedDevice<'_>],
) -> Vec<u8> {
    let mut builder =
        FlatBufferBuilder::with_capacity(512 * devices.len().max(1));
    let devices: Vec<_> = devices
        .iter()
        .map(|device| build_authenticated_device(&mut builder, device))
        .collect();
    let devices = builder.create_vector(&devices);
    let response = fb::AuthenticatedDevicesResponse::create(
        &mut builder,
        &fb::AuthenticatedDevicesResponseArgs {
            devices: Some(devices),
        },
    );
    builder.finish(response, None);
    builder.finished_data().to_vec()
}

/// Build a FlatBuffers `KnownDeviceProfilesRequest` table.
pub fn build_known_device_profiles_request<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    request: &KnownDeviceProfilesRequest,
) -> WIPOffset<fb::KnownDeviceProfilesRequest<'a>> {
    let device_info = request
        .device_info
        .as_ref()
        .map(|info| build_device_info(builder, info));

    fb::KnownDeviceProfilesRequest::create(
        builder,
        &fb::KnownDeviceProfilesRequestArgs { device_info },
    )
}

/// Serialize a `KnownDeviceProfilesRequest` into root FlatBuffers bytes.
pub fn serialize_known_device_profiles_request(
    request: &KnownDeviceProfilesRequest,
) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::with_capacity(256);
    let request = build_known_device_profiles_request(&mut builder, request);
    builder.finish(request, None);
    builder.finished_data().to_vec()
}

/// Build a FlatBuffers `KnownDeviceUserCard` table.
pub fn build_known_device_user_card<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    user: &KnownDeviceUserCard<'_>,
) -> WIPOffset<fb::KnownDeviceUserCard<'a>> {
    let id = uuid_to_fb(&user.id);
    let username = builder.create_string(user.username);
    let display_name = builder.create_string(user.display_name);
    let avatar_url = string_opt(builder, user.avatar_url);

    fb::KnownDeviceUserCard::create(
        builder,
        &fb::KnownDeviceUserCardArgs {
            id: Some(&id),
            username: Some(username),
            display_name: Some(display_name),
            avatar_url,
            has_pin: user.has_pin,
        },
    )
}

/// Serialize known-device profile-selection user cards into a response buffer.
pub fn serialize_known_device_profiles_response(
    known_device: bool,
    users: &[KnownDeviceUserCard<'_>],
) -> Vec<u8> {
    let mut builder =
        FlatBufferBuilder::with_capacity(256 * users.len().max(1));
    let users: Vec<_> = users
        .iter()
        .map(|user| build_known_device_user_card(&mut builder, user))
        .collect();
    let users = builder.create_vector(&users);
    let response = fb::KnownDeviceProfilesResponse::create(
        &mut builder,
        &fb::KnownDeviceProfilesResponseArgs {
            known_device,
            users: Some(users),
        },
    );
    builder.finish(response, None);
    builder.finished_data().to_vec()
}

/// Build a FlatBuffers `PasswordPolicy` table.
pub fn build_password_policy<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    policy: &PasswordPolicy,
) -> WIPOffset<fb::PasswordPolicy<'a>> {
    fb::PasswordPolicy::create(
        builder,
        &fb::PasswordPolicyArgs {
            enforce: policy.enforce,
            min_length: policy.min_length,
            require_uppercase: policy.require_uppercase,
            require_lowercase: policy.require_lowercase,
            require_number: policy.require_number,
            require_special: policy.require_special,
        },
    )
}

/// Build a FlatBuffers `PinPolicy` table.
pub fn build_pin_policy<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    policy: &PinPolicy,
) -> WIPOffset<fb::PinPolicy<'a>> {
    fb::PinPolicy::create(
        builder,
        &fb::PinPolicyArgs {
            min_length: policy.min_length,
            max_length: policy.max_length,
            require_numeric: policy.require_numeric,
            reject_repeated_digits: policy.reject_repeated_digits,
            max_consecutive_identical: policy.max_consecutive_identical,
            reject_sequential_digits: policy.reject_sequential_digits,
        },
    )
}

/// Build a FlatBuffers `DeviceTrustPolicy` table.
pub fn build_device_trust_policy<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    policy: &DeviceTrustPolicy,
) -> WIPOffset<fb::DeviceTrustPolicy<'a>> {
    fb::DeviceTrustPolicy::create(
        builder,
        &fb::DeviceTrustPolicyArgs {
            remember_device_default: policy.remember_device_default,
            trust_duration_days: policy.trust_duration_days,
            pin_max_attempts: policy.pin_max_attempts,
            pin_lockout_minutes: policy.pin_lockout_minutes,
            admin_pin_unlock_enabled: policy.admin_pin_unlock_enabled,
        },
    )
}

/// Build a FlatBuffers `SetupStatus` table.
pub fn build_setup_status<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    status: &SetupStatus,
) -> WIPOffset<fb::SetupStatus<'a>> {
    let admin_password_policy = status
        .admin_password_policy
        .as_ref()
        .map(|policy| build_password_policy(builder, policy));
    let user_password_policy = status
        .user_password_policy
        .as_ref()
        .map(|policy| build_password_policy(builder, policy));
    let pin_policy = status
        .pin_policy
        .as_ref()
        .map(|policy| build_pin_policy(builder, policy));
    let device_trust_policy = status
        .device_trust_policy
        .as_ref()
        .map(|policy| build_device_trust_policy(builder, policy));

    fb::SetupStatus::create(
        builder,
        &fb::SetupStatusArgs {
            needs_setup: status.needs_setup,
            has_admin: status.has_admin,
            requires_setup_token: status.requires_setup_token,
            user_count: status.user_count,
            library_count: status.library_count,
            admin_password_policy,
            user_password_policy,
            pin_policy,
            device_trust_policy,
        },
    )
}

/// Serialize a `SetupStatus` into root FlatBuffers bytes.
pub fn serialize_setup_status(status: &SetupStatus) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::with_capacity(384);
    let status = build_setup_status(&mut builder, status);
    builder.finish(status, None);
    builder.finished_data().to_vec()
}

/// Build a FlatBuffers `UserProfile` table.
pub fn build_user_profile<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    profile: &UserProfile<'_>,
) -> WIPOffset<fb::UserProfile<'a>> {
    let id = uuid_to_fb(&profile.id);
    let username = builder.create_string(profile.username);
    let display_name = builder.create_string(profile.display_name);
    let avatar_url = string_opt(builder, profile.avatar_url);
    let email = string_opt(builder, profile.email);
    let created_at = timestamp_to_fb(&profile.created_at);
    let updated_at = timestamp_to_fb(&profile.updated_at);
    let last_login = option_timestamp_to_fb(profile.last_login.as_ref());

    fb::UserProfile::create(
        builder,
        &fb::UserProfileArgs {
            id: Some(&id),
            username: Some(username),
            display_name: Some(display_name),
            avatar_url,
            email,
            created_at: Some(&created_at),
            updated_at: Some(&updated_at),
            last_login: Some(&last_login),
            is_active: profile.is_active,
        },
    )
}

/// Serialize a `UserProfile` into root FlatBuffers bytes.
pub fn serialize_user_profile(profile: &UserProfile<'_>) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::with_capacity(256);
    let profile = build_user_profile(&mut builder, profile);
    builder.finish(profile, None);
    builder.finished_data().to_vec()
}
