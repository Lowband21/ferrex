use axum::{
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use ferrex_core::{
    api::types::ApiResponse,
    domain::users::{
        auth::domain::{
            services::AuthenticationError, value_objects::SessionScope,
        },
        rbac::UserPermissions,
        user::User,
    },
};

use crate::infra::app_state::AppState;

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let token = extract_bearer_token(&request)?;

    let session = state
        .auth_service()
        .validate_session_token(&token)
        .await
        .map_err(map_authentication_error_to_status)?;

    let user = state
        .unit_of_work()
        .users
        .get_user_by_id(session.user_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let permissions = state
        .unit_of_work()
        .rbac
        .get_user_permissions(user.id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    request.extensions_mut().insert(user);
    request.extensions_mut().insert(permissions);
    request.extensions_mut().insert(session.device_session_id);
    request.extensions_mut().insert(session.scope);

    Ok(next.run(request).await)
}

pub async fn optional_auth_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    if let Ok(token) = extract_bearer_token(&request)
        && let Ok(session) =
            state.auth_service().validate_session_token(&token).await
        && let Ok(Some(user)) = state
            .unit_of_work()
            .users
            .get_user_by_id(session.user_id)
            .await
    {
        if let Ok(permissions) = state
            .unit_of_work()
            .rbac
            .get_user_permissions(user.id)
            .await
        {
            request.extensions_mut().insert(permissions);
        }
        request.extensions_mut().insert(user);
        request.extensions_mut().insert(session.device_session_id);
        request.extensions_mut().insert(session.scope);
    }

    next.run(request).await
}

pub async fn admin_middleware(request: Request, next: Next) -> Response {
    if request.extensions().get::<User>().is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(ApiResponse::<()>::error(
                "Authentication required".to_string(),
            )),
        )
            .into_response();
    }

    let permissions = match request.extensions().get::<UserPermissions>() {
        Some(perms) => perms,
        None => {
            return (
                StatusCode::FORBIDDEN,
                axum::Json(ApiResponse::<()>::error(
                    "Permission system not initialized".to_string(),
                )),
            )
                .into_response();
        }
    };

    if let Err(response) =
        ensure_admin_scope(request.extensions().get::<SessionScope>())
    {
        return *response;
    }

    if !permissions.has_role("admin")
        && !permissions.has_all_permissions(&[
            "users:read",
            "users:create",
            "users:update",
            "users:delete",
            "users:manage_roles",
        ])
    {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(ApiResponse::<()>::error(
                "Admin access required".to_string(),
            )),
        )
            .into_response();
    }

    next.run(request).await
}

fn extract_bearer_token(request: &Request) -> Result<String, StatusCode> {
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !auth_header.starts_with("Bearer ") {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(auth_header[7..].to_string())
}

fn map_authentication_error_to_status(err: AuthenticationError) -> StatusCode {
    match err {
        AuthenticationError::InvalidCredentials
        | AuthenticationError::InvalidPin
        | AuthenticationError::TooManyFailedAttempts
        | AuthenticationError::DeviceNotFound
        | AuthenticationError::DeviceNotTrusted
        | AuthenticationError::SessionExpired
        | AuthenticationError::UserNotFound => StatusCode::UNAUTHORIZED,
        AuthenticationError::DatabaseError(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

fn ensure_admin_scope(
    scope: Option<&SessionScope>,
) -> Result<(), Box<Response>> {
    let scope = scope.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            axum::Json(ApiResponse::<()>::error(
                "Authentication scope missing".to_string(),
            )),
        )
            .into_response()
    })?;

    if *scope != SessionScope::Full {
        return Err(Box::new(
            (
                StatusCode::FORBIDDEN,
                axum::Json(ApiResponse::<()>::error(
                    "Full authentication required for admin actions"
                        .to_string(),
                )),
            )
                .into_response(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_scope_returns_unauthorized() {
        let response =
            ensure_admin_scope(None).expect_err("expected unauthorized");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn playback_scope_is_rejected() {
        let response = ensure_admin_scope(Some(&SessionScope::Playback))
            .expect_err("playback scope should be rejected");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn full_scope_is_allowed() {
        ensure_admin_scope(Some(&SessionScope::Full))
            .expect("full scope should pass");
    }
}
