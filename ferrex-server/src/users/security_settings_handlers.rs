use axum::{Extension, Json, extract::State};
use ferrex_core::{
    api_types::ApiResponse,
    auth::policy::{PasswordPolicy, PasswordPolicyRule},
    database::ports::security_settings::SecuritySettingsUpdate,
    user::User,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::infra::{
    app_state::AppState,
    errors::{AppError, AppResult},
};
use crate::users::setup::setup::PasswordPolicyResponse;

#[derive(Debug, Serialize)]
pub struct SecuritySettingsResponse {
    pub admin_password_policy: PasswordPolicyResponse,
    pub user_password_policy: PasswordPolicyResponse,
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
            return Err(AppError::bad_request("Minimum length must be at least 1"));
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
pub struct UpdateSecuritySettingsRequest {
    pub admin_password_policy: PasswordPolicyInput,
    pub user_password_policy: PasswordPolicyInput,
}

pub async fn get_security_settings(
    State(state): State<AppState>,
    Extension(_admin): Extension<User>,
) -> AppResult<Json<ApiResponse<SecuritySettingsResponse>>> {
    let repo = state.unit_of_work.security_settings.clone();
    let security_settings = repo
        .get_settings()
        .await
        .map_err(|e| AppError::internal(format!("Failed to load security settings: {e}")))?;

    Ok(Json(ApiResponse::success(SecuritySettingsResponse {
        admin_password_policy: PasswordPolicyResponse::from(
            &security_settings.admin_password_policy,
        ),
        user_password_policy: PasswordPolicyResponse::from(&security_settings.user_password_policy),
    })))
}

pub async fn update_security_settings(
    State(state): State<AppState>,
    Extension(admin): Extension<User>,
    Json(request): Json<UpdateSecuritySettingsRequest>,
) -> AppResult<Json<ApiResponse<SecuritySettingsResponse>>> {
    let admin_policy: PasswordPolicy = request.admin_password_policy.try_into()?;
    let user_policy: PasswordPolicy = request.user_password_policy.try_into()?;

    // If enforcement is on, ensure the policy is actually usable
    validate_enforced_policy(&admin_policy)?;
    validate_enforced_policy(&user_policy)?;

    let repo = state.unit_of_work.security_settings.clone();
    let updated = repo
        .update_settings(SecuritySettingsUpdate {
            admin_password_policy: admin_policy,
            user_password_policy: user_policy,
            updated_by: Some(admin.id),
        })
        .await
        .map_err(|e| AppError::internal(format!("Failed to update security settings: {e}")))?;

    info!(
        "Admin {} ({}) updated security settings",
        admin.username, admin.id
    );

    Ok(Json(ApiResponse::success(SecuritySettingsResponse {
        admin_password_policy: PasswordPolicyResponse::from(&updated.admin_password_policy),
        user_password_policy: PasswordPolicyResponse::from(&updated.user_password_policy),
    })))
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
