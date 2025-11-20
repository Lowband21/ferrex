use axum::{
    middleware,
    routing::{get, post, put},
    Router,
};

use crate::{
    AppState, dev_handlers,
    media::{
        image_handlers,
        library_handlers_v2::{
            create_library_handler, delete_library_handler, get_libraries_with_media_handler,
            get_library_handler, get_library_media_handler, get_library_sorted_indices_handler,
            post_library_filtered_indices_handler, update_library_handler,
        },
        query_handlers,
        scan::{
            get_folder_inventory, get_scan_progress,
            scan_handlers::{
                media_events_sse_handler, scan_all_libraries_handler, scan_library_handler,
            },
        },
    },
    stream::{stream_handlers, transcoding::transcoding_handlers},
    users::{
        admin_handlers, auth, role_handlers, session_handlers,
        setup::setup::{check_setup_status, create_initial_admin},
        user_handlers, user_management, watch_status_handlers,
    },
    websocket,
};

/// Create all v1 API routes
pub fn create_v1_router(state: AppState) -> Router<AppState> {
    // Combine all routes
    Router::new()
        // Public authentication endpoints
        .route("/auth/register", post(auth::handlers::register))
        .route("/auth/login", post(auth::handlers::login))
        .route("/auth/refresh", post(auth::handlers::refresh))
        // Device authentication endpoints
        .route(
            "/auth/device/login",
            post(auth::device_handlers::device_login),
        )
        .route("/auth/device/pin", post(auth::device_handlers::pin_login))
        .route(
            "/auth/device/status",
            get(auth::device_handlers::check_device_status),
        )
        // Public user endpoints (for user selection screen)
        .route("/users", get(user_management::list_users))
        .route("/users/public", get(user_handlers::list_users_handler))
        // Public setup endpoints (for first-run)
        .route("/setup/status", get(check_setup_status))
        .route("/setup/admin", post(create_initial_admin))
        .route("/stream/{id}", get(crate::stream_handler))
        //
        .merge(create_libraries_routes(state.clone()))
        .merge(create_metadata_routes(state.clone()))
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
        // Device authentication management
        .route(
            "/auth/device/pin/set",
            post(auth::device_handlers::set_device_pin),
        )
        .route(
            "/auth/device/list",
            get(auth::device_handlers::list_user_devices),
        )
        .route(
            "/auth/device/revoke",
            post(auth::device_handlers::revoke_device),
        )
        // Device trust validation endpoints
        .route(
            "/auth/device/validate",
            get(auth::device_validation::validate_device_trust),
        )
        .route(
            "/auth/device/revoke-trust",
            post(auth::device_validation::revoke_device_trust),
        )
        .route(
            "/auth/device/trusted",
            get(auth::device_validation::list_trusted_devices),
        )
        .route(
            "/auth/device/extend-trust",
            post(auth::device_validation::extend_device_trust),
        )
        // User endpoints
        //
        .route("/users/me", get(auth::handlers::get_current_user))
        // Note: User profile management routes moved to new user management API
        .route(
            "/users/sessions",
            get(session_handlers::get_user_sessions_handler),
        )
        .route(
            "/users/sessions/{id}",
            axum::routing::delete(session_handlers::delete_session_handler),
        )
        .route(
            "/users/sessions",
            axum::routing::delete(session_handlers::delete_all_sessions_handler),
        )
        // User preferences endpoint (for current user)
        .route(
            "/users/me/preferences",
            put(auth::user_preferences::update_preferences),
        )
        .route(
            "/users/me/preferences",
            get(auth::user_preferences::get_preferences),
        )
        // User list endpoint (authenticated)
        .route(
            "/users/list",
            get(user_handlers::list_users_authenticated_handler),
        )
        // User management endpoints (admin)
        .route("/users", post(user_management::create_user))
        .route("/users/{id}", put(user_management::update_user))
        .route(
            "/users/{id}",
            axum::routing::delete(user_management::delete_user),
        )
        // Watch status endpoints
        //
        .route(
            "/watch/progress",
            post(watch_status_handlers::update_progress_handler),
        )
        .route(
            "/watch/state",
            get(watch_status_handlers::get_watch_state_handler),
        )
        .route(
            "/watch/continue",
            get(watch_status_handlers::get_continue_watching_handler),
        )
        .route(
            "/watch/progress/{media_id}",
            axum::routing::delete(watch_status_handlers::clear_progress_handler),
        )
        // Media endpoints
        //
        .route(
            "/media/{id}/progress",
            get(watch_status_handlers::get_media_progress_handler),
        )
        .route(
            "/media/{id}/complete",
            post(watch_status_handlers::mark_completed_handler),
        )
        .route(
            "/media/{id}/is-completed",
            get(watch_status_handlers::is_completed_handler),
        )
        // Folder inventory monitoring and control
        .route("/folders/inventory/{library_id}", get(get_folder_inventory))
        .route("/folders/progress/{library_id}", get(get_scan_progress))
        //.route(
        //    "/folders/rescan/{folder_id}",
        //    post(trigger_folder_rescan),
        //)
        // Query system
        .route("/media/query", post(query_handlers::query_media_handler))
        // Scanning: pending-based triggers and counts
        //.route(
        //    "/libraries/{id}/scan/pending",
        //    post(crate::media::scan::scan_handlers::scan_pending_for_library_handler),
        //)
        //.route(
        //    "/libraries/scan/pending",
        //    post(crate::media::scan::scan_handlers::scan_pending_for_all_libraries_handler),
        //)
        .route(
            "/libraries/{id}/scan/pending-count",
            get(crate::media::scan::scan_handlers::pending_count_for_library_handler),
        )
        //.route(
        //    "/libraries/scan/pending-count",
        //    get(crate::media::scan::scan_handlers::pending_count_all_libraries_handler),
        //)
        // Refresh metadata for a specific media item (by marking its folder pending)
        //.route(
        //    "/media/{media_type}/{id}/refresh",
        //    post(crate::media::scan::scan_handlers::efresh_metadata_handler),
        //)
        // Streaming endpoints
        //
        .route(
            "/stream/{media_type}/{id}/progress",
            post(stream_handlers::report_progress_handler),
        )
        // Sync session endpoints
        // Unimplemented
        //.route(
        //    "/sync/sessions",
        //    post(sync_handlers::create_sync_session_handler),
        //)
        //.route(
        //    "/sync/sessions/join/{code}",
        //    get(sync_handlers::join_sync_session_handler),
        //)
        //.route(
        //    "/sync/sessions/{id}",
        //    axum::routing::delete(sync_handlers::leave_sync_session_handler),
        //)
        //.route(
        //    "/sync/sessions/{id}/state",
        //    get(sync_handlers::get_sync_session_state_handler),
        //)
        .route(
            "/sync/ws",
            axum::routing::any(websocket::handler::websocket_handler),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::auth_middleware,
        ))
}

/// Create libraries routes
fn create_libraries_routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/library/events/sse", get(media_events_sse_handler))
        .route(
            "/libraries",
            get(get_libraries_with_media_handler).post(create_library_handler),
        )
        .route("/libraries/{id}", get(get_library_handler))
        .route("/libraries/{id}", axum::routing::put(update_library_handler))
        .route(
            "/libraries/{id}",
            axum::routing::delete(delete_library_handler),
        )
        .route("/library/scan", post(scan_library_handler))
        .route("/libraries/scan", post(scan_all_libraries_handler))
        .route("/libraries/{id}/media", get(get_library_media_handler))
        // New binary indices endpoints
        .route(
            "/libraries/{id}/indices/sorted",
            get(get_library_sorted_indices_handler),
        )
        .route(
            "/libraries/{id}/indices/filter",
            post(post_library_filtered_indices_handler),
        )
    //.route_layer(middleware::from_fn_with_state(
    //    state.clone(),
    //    auth::middleware::auth_middleware,
    //))
}

/// Create metadata routes
fn create_metadata_routes(state: AppState) -> Router<AppState> {
    Router::new().route(
        "/images/{type}/{id}/{category}/{index}",
        get(image_handlers::serve_image_handler),
    )
    //.route_layer(middleware::from_fn_with_state(
    //    state.clone(),
    //    auth::middleware::auth_middleware,
    //))
}

/// Create admin routes that require admin role
fn create_admin_routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/admin/users", get(admin_handlers::list_all_users))
        .route(
            "/admin/users/{id}/roles",
            put(admin_handlers::assign_user_roles),
        )
        .route(
            "/admin/users/{id}",
            axum::routing::delete(admin_handlers::delete_user_admin),
        )
        .route(
            "/admin/users/{id}/sessions",
            get(admin_handlers::get_user_sessions_admin),
        )
        .route(
            "/admin/users/{user_id}/sessions/{session_id}",
            axum::routing::delete(admin_handlers::revoke_user_session_admin),
        )
        .route("/admin/stats", get(admin_handlers::get_admin_stats))
        // Development/reset endpoints (admin only)
        .route(
            "/admin/dev/reset/check",
            get(dev_handlers::check_reset_status),
        )
        .route(
            "/admin/dev/reset/database",
            post(dev_handlers::reset_database),
        )
        // Admin session management for PIN authentication
        .route(
            "/admin/sessions/register",
            post(auth::pin_handlers::register_admin_session),
        )
        .route(
            "/admin/sessions/{device_id}",
            axum::routing::delete(auth::pin_handlers::remove_admin_session),
        )
        .route("/admin/dev/seed", post(dev_handlers::seed_database))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::auth_middleware,
        ))
        .route_layer(middleware::from_fn(auth::middleware::admin_middleware))
}

/// Create role management routes
fn create_role_routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/roles", get(role_handlers::list_roles_handler))
        .route("/permissions", get(role_handlers::list_permissions_handler))
        .route(
            "/users/{id}/permissions",
            get(role_handlers::get_user_permissions_handler),
        )
        .route(
            "/users/{id}/roles",
            put(role_handlers::assign_user_roles_handler),
        )
        .route(
            "/users/{id}/permissions/override",
            post(role_handlers::override_user_permission_handler),
        )
        .route(
            "/users/me/permissions",
            get(role_handlers::get_my_permissions_handler),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::auth_middleware,
        ))
}
