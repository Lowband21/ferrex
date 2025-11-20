use axum::{
    middleware,
    routing::{get, post, put},
    Router,
};
use crate::{
    admin_handlers, api::user_management, auth, dev_handlers, handlers, image_handlers, media_reference_handlers,
    query_handlers, role_handlers, session_handlers, stream_handlers, sync_handlers,
    user_handlers, watch_status_handlers, websocket, AppState,
};

/// Create all v1 API routes
pub fn create_v1_router(state: AppState) -> Router<AppState> {
    // Combine all routes
    Router::new()
        // Public endpoints
        .route("/auth/register", post(auth::handlers::register))
        .route("/auth/login", post(auth::handlers::login))
        .route("/auth/refresh", post(auth::handlers::refresh))
        // Device authentication endpoints
        .route("/auth/device/login", post(auth::device_handlers::device_login))
        .route("/auth/device/pin", post(auth::device_handlers::pin_login))
        .route("/auth/device/status", get(auth::device_handlers::check_device_status))
        // Public user endpoints (for user selection screen)
        .route("/users/public", get(user_handlers::list_users_handler))
        // Public setup endpoints (for first-run)
        .route("/setup/status", get(handlers::check_setup_status))
        .route("/setup/admin", post(handlers::create_initial_admin))
        // Public media endpoints
        .route("/media/:id", get(media_reference_handlers::get_media_reference_handler))
        // Image serving endpoint (public but client sends auth headers)
        .route("/images/:type/:id/:category/:index", get(image_handlers::serve_image_handler))
        // Merge protected routes
        .merge(create_protected_routes(state.clone()))
        // Merge admin routes
        .merge(create_admin_routes(state.clone()))
        // Merge role routes
        .merge(create_role_routes(state))
}

/// Create protected routes that require authentication
fn create_protected_routes(state: AppState) -> Router<AppState> {
    Router::new()
        // Auth endpoints
        .route("/auth/logout", post(auth::handlers::logout))
        .route("/users/me", get(auth::handlers::get_current_user))
        // Device authentication management
        .route("/auth/device/pin/set", post(auth::device_handlers::set_device_pin))
        .route("/auth/device/list", get(auth::device_handlers::list_user_devices))
        .route("/auth/device/revoke", post(auth::device_handlers::revoke_device))
        // Device trust validation endpoints
        .route("/auth/device/validate", get(auth::device_validation::validate_device_trust))
        .route("/auth/device/revoke-trust", post(auth::device_validation::revoke_device_trust))
        .route("/auth/device/trusted", get(auth::device_validation::list_trusted_devices))
        .route("/auth/device/extend-trust", post(auth::device_validation::extend_device_trust))
        // Watch status endpoints
        .route("/watch/progress", post(watch_status_handlers::update_progress_handler))
        .route("/watch/state", get(watch_status_handlers::get_watch_state_handler))
        .route("/watch/continue", get(watch_status_handlers::get_continue_watching_handler))
        .route("/watch/progress/:media_id", axum::routing::delete(watch_status_handlers::clear_progress_handler))
        .route("/media/:id/progress", get(watch_status_handlers::get_media_progress_handler))
        .route("/media/:id/complete", post(watch_status_handlers::mark_completed_handler))
        .route("/media/:id/is-completed", get(watch_status_handlers::is_completed_handler))
        // Streaming endpoints
        .route("/stream/:id", get(stream_handlers::stream_with_progress_handler))
        .route("/stream/:id/progress", post(stream_handlers::report_progress_handler))
        // Sync session endpoints
        .route("/sync/sessions", post(sync_handlers::create_sync_session_handler))
        .route("/sync/sessions/join/:code", get(sync_handlers::join_sync_session_handler))
        .route("/sync/sessions/:id", axum::routing::delete(sync_handlers::leave_sync_session_handler))
        .route("/sync/sessions/:id/state", get(sync_handlers::get_sync_session_state_handler))
        .route("/sync/ws", get(websocket::handler::websocket_handler))
        // Note: User profile management routes moved to new user management API
        // Session management
        .route("/users/sessions", get(session_handlers::get_user_sessions_handler))
        .route("/users/sessions/:id", axum::routing::delete(session_handlers::delete_session_handler))
        .route("/users/sessions", axum::routing::delete(session_handlers::delete_all_sessions_handler))
        // Query system
        .route("/media/query", post(query_handlers::query_media_handler))
        // Batch media fetch
        .route("/media/batch", post(media_reference_handlers::get_media_batch_handler))
        // User preferences endpoint (for current user)
        .route("/users/me/preferences", put(auth::user_preferences::update_preferences))
        .route("/users/me/preferences", get(auth::user_preferences::get_preferences))
        // User list endpoint (authenticated)
        .route("/users/list", get(user_handlers::list_users_authenticated_handler))
        // User management endpoints (admin)
        .route("/users", get(user_management::list_users))
        .route("/users", post(user_management::create_user))
        .route("/users/:id", put(user_management::update_user))
        .route("/users/:id", axum::routing::delete(user_management::delete_user))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::auth_middleware,
        ))
}

/// Create admin routes that require admin role
fn create_admin_routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/admin/users", get(admin_handlers::list_all_users))
        .route("/admin/users/:id/roles", put(admin_handlers::assign_user_roles))
        .route("/admin/users/:id", axum::routing::delete(admin_handlers::delete_user_admin))
        .route("/admin/users/:id/sessions", get(admin_handlers::get_user_sessions_admin))
        .route("/admin/users/:user_id/sessions/:session_id", axum::routing::delete(admin_handlers::revoke_user_session_admin))
        .route("/admin/stats", get(admin_handlers::get_admin_stats))
        // Development/reset endpoints (admin only)
        .route("/admin/dev/reset/check", get(dev_handlers::check_reset_status))
        .route("/admin/dev/reset/database", post(dev_handlers::reset_database))
        .route("/admin/dev/seed", post(dev_handlers::seed_database))
        .route_layer(middleware::from_fn(auth::middleware::admin_middleware))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::auth_middleware,
        ))
}

/// Create role management routes
fn create_role_routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/roles", get(role_handlers::list_roles_handler))
        .route("/permissions", get(role_handlers::list_permissions_handler))
        .route("/users/:id/permissions", get(role_handlers::get_user_permissions_handler))
        .route("/users/:id/roles", put(role_handlers::assign_user_roles_handler))
        .route("/users/:id/permissions/override", post(role_handlers::override_user_permission_handler))
        .route("/users/me/permissions", get(role_handlers::get_my_permissions_handler))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::auth_middleware,
        ))
}