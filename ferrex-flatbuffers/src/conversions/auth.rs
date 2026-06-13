//! Conversions for authentication and setup FlatBuffers payloads.

use chrono::{DateTime, Utc};
use flatbuffers::{FlatBufferBuilder, WIPOffset};
use uuid::Uuid;

use crate::conversions::common::{option_timestamp_to_fb, timestamp_to_fb};
use crate::fb::auth as fb;
use crate::uuid_helpers::uuid_to_fb;

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

/// Parse a FlatBuffers `RefreshRequest` request body.
pub fn parse_refresh_request(
    bytes: &[u8],
) -> Result<RefreshRequest, flatbuffers::InvalidFlatbuffer> {
    let request = flatbuffers::root::<fb::RefreshRequest>(bytes)?;

    Ok(RefreshRequest {
        refresh_token: request.refresh_token().to_string(),
    })
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
