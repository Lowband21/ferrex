use ferrex_core::{
    api::types::ApiResponse,
    database::repository_ports::security_settings::SecuritySettingsUpdate,
    domain::users::{
        auth::policy::{
            DeviceTrustPolicy, PasswordPolicy, PasswordPolicyRule,
            PinSecurityPolicy,
        },
        user::User,
    },
};

use crate::{
    handlers::users::setup::PasswordPolicyResponse,
    infra::{
        app_state::AppState,
        errors::{AppError, AppResult},
    },
};

use axum::{Extension, Json, extract::State};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

#[derive(Debug, Serialize)]
pub struct SecuritySettingsResponse {
    pub admin_password_policy: PasswordPolicyResponse,
    pub user_password_policy: PasswordPolicyResponse,
    pub pin_policy: PinPolicyResponse,
    pub device_trust_policy: DeviceTrustPolicyResponse,
}

#[derive(Debug, Serialize, Clone)]
pub struct PinPolicyResponse {
    pub min_length: u16,
    pub max_length: u16,
    pub require_numeric: bool,
    pub reject_repeated_digits: bool,
    pub max_consecutive_identical: u16,
    pub reject_sequential_digits: bool,
}

impl From<&PinSecurityPolicy> for PinPolicyResponse {
    fn from(value: &PinSecurityPolicy) -> Self {
        Self {
            min_length: value.min_length,
            max_length: value.max_length,
            require_numeric: value.require_numeric,
            reject_repeated_digits: value.reject_repeated_digits,
            max_consecutive_identical: value.max_consecutive_identical,
            reject_sequential_digits: value.reject_sequential_digits,
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct DeviceTrustPolicyResponse {
    pub remember_device_default: bool,
    pub trust_duration_days: u16,
    pub pin_max_attempts: u8,
    pub pin_lockout_minutes: u16,
    pub admin_pin_unlock_enabled: bool,
}

impl From<&DeviceTrustPolicy> for DeviceTrustPolicyResponse {
    fn from(value: &DeviceTrustPolicy) -> Self {
        Self {
            remember_device_default: value.remember_device_default,
            trust_duration_days: value.trust_duration_days,
            pin_max_attempts: value.pin_max_attempts,
            pin_lockout_minutes: value.pin_lockout_minutes,
            admin_pin_unlock_enabled: value.admin_pin_unlock_enabled,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct PasswordPolicyInput {
    pub enforce: bool,
    pub min_length: u16,
    pub require_uppercase: bool,
    pub require_lowercase: bool,
    pub require_number: bool,
    pub require_special: bool,
}

impl TryFrom<PasswordPolicyInput> for PasswordPolicy {
    type Error = AppError;

    fn try_from(value: PasswordPolicyInput) -> Result<Self, Self::Error> {
        if value.min_length == 0 {
            return Err(AppError::bad_request(
                "Minimum length must be at least 1",
            ));
        }

        Ok(PasswordPolicy {
            enforce: value.enforce,
            min_length: value.min_length,
            require_uppercase: value.require_uppercase,
            require_lowercase: value.require_lowercase,
            require_number: value.require_number,
            require_special: value.require_special,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct PinPolicyInput {
    pub min_length: u16,
    pub max_length: u16,
    pub require_numeric: bool,
    pub reject_repeated_digits: bool,
    pub max_consecutive_identical: u16,
    pub reject_sequential_digits: bool,
}

impl TryFrom<PinPolicyInput> for PinSecurityPolicy {
    type Error = AppError;

    fn try_from(value: PinPolicyInput) -> Result<Self, Self::Error> {
        let policy = PinSecurityPolicy {
            min_length: value.min_length,
            max_length: value.max_length,
            require_numeric: value.require_numeric,
            reject_repeated_digits: value.reject_repeated_digits,
            max_consecutive_identical: value.max_consecutive_identical,
            reject_sequential_digits: value.reject_sequential_digits,
        };
        validate_pin_policy(&policy)?;
        Ok(policy)
    }
}

#[derive(Debug, Deserialize)]
pub struct DeviceTrustPolicyInput {
    pub remember_device_default: bool,
    pub trust_duration_days: u16,
    pub pin_max_attempts: u8,
    pub pin_lockout_minutes: u16,
    pub admin_pin_unlock_enabled: bool,
}

impl TryFrom<DeviceTrustPolicyInput> for DeviceTrustPolicy {
    type Error = AppError;

    fn try_from(value: DeviceTrustPolicyInput) -> Result<Self, Self::Error> {
        let policy = DeviceTrustPolicy {
            remember_device_default: value.remember_device_default,
            trust_duration_days: value.trust_duration_days,
            pin_max_attempts: value.pin_max_attempts,
            pin_lockout_minutes: value.pin_lockout_minutes,
            admin_pin_unlock_enabled: value.admin_pin_unlock_enabled,
        };
        validate_device_trust_policy(&policy)?;
        Ok(policy)
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateSecuritySettingsRequest {
    pub admin_password_policy: PasswordPolicyInput,
    pub user_password_policy: PasswordPolicyInput,
    #[serde(default)]
    pub pin_policy: Option<PinPolicyInput>,
    #[serde(default)]
    pub device_trust_policy: Option<DeviceTrustPolicyInput>,
}

pub async fn get_security_settings(
    State(state): State<AppState>,
    Extension(_admin): Extension<User>,
) -> AppResult<Json<ApiResponse<SecuritySettingsResponse>>> {
    let repo = state.unit_of_work().security_settings.clone();
    let security_settings = repo.get_settings().await.map_err(|e| {
        AppError::internal(format!("Failed to load security settings: {e}"))
    })?;

    Ok(Json(ApiResponse::success(security_response(
        &security_settings,
    ))))
}

pub async fn update_security_settings(
    State(state): State<AppState>,
    Extension(admin): Extension<User>,
    Json(request): Json<UpdateSecuritySettingsRequest>,
) -> AppResult<Json<ApiResponse<SecuritySettingsResponse>>> {
    let repo = state.unit_of_work().security_settings.clone();
    let current = repo.get_settings().await.map_err(|e| {
        AppError::internal(format!("Failed to load security settings: {e}"))
    })?;

    let admin_policy: PasswordPolicy =
        request.admin_password_policy.try_into()?;
    let user_policy: PasswordPolicy =
        request.user_password_policy.try_into()?;
    let pin_policy = match request.pin_policy {
        Some(input) => input.try_into()?,
        None => current.pin_policy,
    };
    let device_trust_policy = match request.device_trust_policy {
        Some(input) => input.try_into()?,
        None => current.device_trust_policy,
    };

    // If enforcement is on, ensure the policy is actually usable
    validate_enforced_policy(&admin_policy)?;
    validate_enforced_policy(&user_policy)?;

    let updated = repo
        .update_settings(SecuritySettingsUpdate {
            admin_password_policy: admin_policy,
            user_password_policy: user_policy,
            pin_policy,
            device_trust_policy,
            updated_by: Some(admin.id),
        })
        .await
        .map_err(|e| {
            AppError::internal(format!(
                "Failed to update security settings: {e}"
            ))
        })?;

    info!(
        "Admin {} ({}) updated security settings",
        admin.username, admin.id
    );

    Ok(Json(ApiResponse::success(security_response(&updated))))
}

pub fn security_response(
    settings: &ferrex_core::domain::users::auth::policy::AuthSecuritySettings,
) -> SecuritySettingsResponse {
    SecuritySettingsResponse {
        admin_password_policy: PasswordPolicyResponse::from(
            &settings.admin_password_policy,
        ),
        user_password_policy: PasswordPolicyResponse::from(
            &settings.user_password_policy,
        ),
        pin_policy: PinPolicyResponse::from(&settings.pin_policy),
        device_trust_policy: DeviceTrustPolicyResponse::from(
            &settings.device_trust_policy,
        ),
    }
}

fn validate_enforced_policy(policy: &PasswordPolicy) -> AppResult<()> {
    if !policy.enforce {
        return Ok(());
    }

    let failures = policy.check("A").failures; // quick baseline using trivial input
    // When enforcement is on ensure min length isn't absurdly low
    if policy.min_length < 4 {
        return Err(AppError::bad_request(
            "Minimum length must be at least 4 when enforcement is enabled",
        ));
    }

    if failures.is_empty() {
        return Ok(());
    }

    warn!(
        "Password policy validation produced unexpected failures: {}",
        describe_policy_failures(&failures)
    );
    Ok(())
}

fn validate_pin_policy(policy: &PinSecurityPolicy) -> AppResult<()> {
    if !policy.require_numeric {
        return Err(AppError::bad_request(
            "Official clients currently require numeric PINs",
        ));
    }
    if policy.min_length < 4 {
        return Err(AppError::bad_request(
            "PIN minimum length must be at least 4",
        ));
    }
    if policy.max_length < policy.min_length {
        return Err(AppError::bad_request(
            "PIN maximum length must be greater than or equal to minimum length",
        ));
    }
    if policy.max_length > 16 {
        return Err(AppError::bad_request(
            "PIN maximum length cannot exceed 16",
        ));
    }
    if policy.reject_repeated_digits && policy.max_consecutive_identical == 0 {
        return Err(AppError::bad_request(
            "PIN maximum consecutive identical digits must be at least 1",
        ));
    }
    if policy.reject_repeated_digits
        && policy.max_consecutive_identical >= policy.min_length
    {
        return Err(AppError::bad_request(
            "PIN repeated-digit threshold must be less than the minimum length",
        ));
    }
    Ok(())
}

fn validate_device_trust_policy(policy: &DeviceTrustPolicy) -> AppResult<()> {
    if policy.trust_duration_days == 0 {
        return Err(AppError::bad_request(
            "Device trust duration must be at least 1 day",
        ));
    }
    if policy.trust_duration_days > 365 {
        return Err(AppError::bad_request(
            "Device trust duration cannot exceed 365 days",
        ));
    }
    if policy.pin_max_attempts == 0 || policy.pin_max_attempts > 10 {
        return Err(AppError::bad_request(
            "PIN max attempts must be between 1 and 10",
        ));
    }
    if policy.pin_lockout_minutes == 0 || policy.pin_lockout_minutes > 24 * 60 {
        return Err(AppError::bad_request(
            "PIN lockout duration must be between 1 and 1440 minutes",
        ));
    }
    if policy.admin_pin_unlock_enabled {
        return Err(AppError::bad_request(
            "Admin PIN unlock is intentionally disabled; admin actions require full password authentication",
        ));
    }
    Ok(())
}

fn describe_policy_failures(failures: &[PasswordPolicyRule]) -> String {
    if failures.is_empty() {
        return "no failures".to_string();
    }

    failures
        .iter()
        .map(|rule| rule.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}
