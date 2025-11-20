use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::OnceLock;

use axum::{
    Json,
    extract::{ConnectInfo, State},
};
use ferrex_core::{
    api_types::{
        ApiResponse,
        setup::{ConfirmClaimRequest, ConfirmClaimResponse, StartClaimRequest, StartClaimResponse},
    },
    setup::{ConfirmedClaim, SetupClaimError, StartedClaim},
};

use crate::{
    infra::{
        app_state::AppState,
        errors::{AppError, AppResult},
    },
    users::{UserService, setup::setup::SetupRateLimiter},
};

pub async fn start_secure_claim(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<StartClaimRequest>,
) -> AppResult<Json<ApiResponse<StartClaimResponse>>> {
    let client_ip = addr.ip();
    require_lan(client_ip)?;

    let validated_name = request
        .device_name
        .as_ref()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string());

    if let Some(ref name) = validated_name
        && name.len() > 64 {
            return Err(AppError::bad_request(
                "Device name cannot exceed 64 characters",
            ));
        }

    claim_rate_limiter()
        .check_rate_limit(&client_ip.to_string())
        .await?;

    let user_service = UserService::new(&state);
    if !user_service.needs_setup().await? {
        return Err(AppError::gone("Setup has already been completed"));
    }

    let claim_service = state.setup_claim_service();
    match claim_service
        .start_claim(validated_name, Some(client_ip))
        .await
    {
        Ok(StartedClaim {
            claim_id,
            claim_code,
            expires_at,
        }) => Ok(Json(ApiResponse::success(StartClaimResponse {
            claim_id,
            claim_code,
            expires_at,
            lan_only: true,
        }))),
        Err(SetupClaimError::ActiveClaimPending { expires_at, .. }) => {
            Err(AppError::conflict(format!(
                "A claim is already pending until {}",
                expires_at.to_rfc3339()
            )))
        }
        Err(err) => Err(map_claim_error(err)),
    }
}

pub async fn confirm_secure_claim(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<ConfirmClaimRequest>,
) -> AppResult<Json<ApiResponse<ConfirmClaimResponse>>> {
    let client_ip = addr.ip();
    require_lan(client_ip)?;

    if request.claim_code.trim().is_empty() {
        return Err(AppError::bad_request("Claim code cannot be empty"));
    }

    claim_rate_limiter()
        .check_rate_limit(&client_ip.to_string())
        .await?;

    let user_service = UserService::new(&state);
    if !user_service.needs_setup().await? {
        return Err(AppError::gone("Setup has already been completed"));
    }

    let claim_service = state.setup_claim_service();
    match claim_service.confirm_claim(&request.claim_code).await {
        Ok(ConfirmedClaim {
            claim_id,
            claim_token,
            expires_at,
        }) => Ok(Json(ApiResponse::success(ConfirmClaimResponse {
            claim_id,
            claim_token,
            expires_at,
        }))),
        Err(err) => Err(map_claim_error(err)),
    }
}

fn require_lan(ip: IpAddr) -> AppResult<()> {
    if is_lan_ip(ip) {
        Ok(())
    } else {
        Err(AppError::forbidden(
            "Setup claim endpoints require local network access",
        ))
    }
}

fn is_lan_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || is_cgnat(v4),
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local(),
    }
}

fn is_cgnat(ip: Ipv4Addr) -> bool {
    let addr = u32::from(ip);
    let start = u32::from(Ipv4Addr::new(100, 64, 0, 0));
    let end = u32::from(Ipv4Addr::new(100, 127, 255, 255));
    addr >= start && addr <= end
}

fn map_claim_error(error: SetupClaimError) -> AppError {
    match error {
        SetupClaimError::InvalidCode => AppError::bad_request("Invalid claim code supplied"),
        SetupClaimError::InvalidToken => AppError::forbidden("Invalid claim token supplied"),
        SetupClaimError::Expired { .. } => AppError::gone("Claim secret has expired"),
        SetupClaimError::ActiveClaimPending { expires_at, .. } => AppError::conflict(format!(
            "Another claim is already pending until {}",
            expires_at.to_rfc3339()
        )),
        SetupClaimError::Storage(err) => {
            AppError::internal(format!("Setup claim persistence failed: {err}"))
        }
    }
}

fn claim_rate_limiter() -> &'static SetupRateLimiter {
    static INSTANCE: OnceLock<SetupRateLimiter> = OnceLock::new();
    INSTANCE.get_or_init(SetupRateLimiter::default)
}

#[doc(hidden)]
pub async fn reset_claim_rate_limiter_for_tests() {
    claim_rate_limiter().reset().await;
}
