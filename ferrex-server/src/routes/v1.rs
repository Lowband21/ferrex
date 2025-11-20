use axum::{
    Router, middleware,
    routing::{get, post, put},
};

use ferrex_core::api_routes::v1;

use crate::{
    handlers::{
        admin::dev_handlers,
        handle_websocket::{self, websocket_handler},
        media::{
            handle_image::{self, serve_image_handler},
            handle_library::{
                create_library_handler, delete_library_handler, get_libraries_with_media_handler,
                get_library_handler, get_library_media_handler, get_library_sorted_indices_handler,
                post_library_filtered_indices_handler, update_library_handler,
            },
            handle_search::query_media_handler,
        },
        scan::handle_scan::{
            active_scans_handler, cancel_scan_handler, latest_progress_handler,
            media_events_sse_handler, pause_scan_handler, resume_scan_handler, scan_config_handler,
            scan_events_handler, scan_history_handler, scan_metrics_handler,
            scan_progress_sse_handler, start_scan_handler,
        },
    },
    infra::{
        app_state::AppState,
        scan::folder_inventory::{get_folder_inventory, get_scan_progress},
    },
    stream::stream_handlers,
    users::{
        admin_handlers, auth, role_handlers, session_handlers,
        setup::setup::{check_setup_status, create_initial_admin},
        user_handlers, user_management, watch_status_handlers,
    },
};

/// Create all v1 API routes
pub fn create_v1_router(state: AppState) -> Router<AppState> {
    // Combine all routes
    Router::new()
        // Public authentication endpoints
        .route(v1::auth::REGISTER, post(auth::handlers::register))
        .route(v1::auth::LOGIN, post(auth::handlers::login))
        .route(v1::auth::REFRESH, post(auth::handlers::refresh))
        // Device authentication endpoints
        .route(
            v1::auth::device::LOGIN,
            post(auth::device_handlers::device_login),
        )
        .route(
            v1::auth::device::PIN_LOGIN,
            post(auth::device_handlers::pin_login),
        )
        .route(
            v1::auth::device::STATUS,
            get(auth::device_handlers::check_device_status),
        )
        // Public user endpoints (for user selection screen)
        .route(v1::users::COLLECTION, get(user_management::list_users))
        .route(
            v1::users::PUBLIC_LIST,
            get(user_handlers::list_users_handler),
        )
        // Public setup endpoints (for first-run)
        .route(v1::setup::STATUS, get(check_setup_status))
        .route(v1::setup::CREATE_ADMIN, post(create_initial_admin))
        .route(
            v1::stream::PLAY,
            get(stream_handlers::stream_with_progress_handler),
        )
        //
        .merge(create_libraries_routes(state.clone()))
        .merge(create_scan_routes(state.clone()))
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
        .route(v1::auth::LOGOUT, post(auth::handlers::logout))
        // Device authentication management
        .route(
            v1::auth::device::SET_PIN,
            post(auth::device_handlers::set_device_pin),
        )
        .route(
            v1::auth::device::LIST,
            get(auth::device_handlers::list_user_devices),
        )
        .route(
            v1::auth::device::REVOKE,
            post(auth::device_handlers::revoke_device),
        )
        // Device trust validation endpoints
        .route(
            v1::auth::device::VALIDATE_TRUST,
            get(auth::device_validation::validate_device_trust),
        )
        .route(
            v1::auth::device::REVOKE_TRUST,
            post(auth::device_validation::revoke_device_trust),
        )
        .route(
            v1::auth::device::LIST_TRUSTED,
            get(auth::device_validation::list_trusted_devices),
        )
        .route(
            v1::auth::device::EXTEND_TRUST,
            post(auth::device_validation::extend_device_trust),
        )
        // User endpoints
        //
        .route(v1::users::CURRENT, get(auth::handlers::get_current_user))
        // Note: User profile management routes moved to new user management API
        .route(
            v1::users::sessions::COLLECTION,
            get(session_handlers::get_user_sessions_handler),
        )
        .route(
            v1::users::sessions::ITEM,
            axum::routing::delete(session_handlers::delete_session_handler),
        )
        .route(
            v1::users::sessions::COLLECTION,
            axum::routing::delete(session_handlers::delete_all_sessions_handler),
        )
        // User preferences endpoint (for current user)
        .route(
            v1::users::CURRENT_PREFERENCES,
            put(auth::user_preferences::update_preferences),
        )
        .route(
            v1::users::CURRENT_PREFERENCES,
            get(auth::user_preferences::get_preferences),
        )
        // User list endpoint (authenticated)
        .route(
            v1::users::LIST_AUTH,
            get(user_handlers::list_users_authenticated_handler),
        )
        // User management endpoints (admin)
        .route(v1::users::COLLECTION, post(user_management::create_user))
        .route(v1::users::ITEM, put(user_management::update_user))
        .route(
            v1::users::ITEM,
            axum::routing::delete(user_management::delete_user),
        )
        // Watch status endpoints
        //
        .route(
            v1::watch::UPDATE_PROGRESS,
            post(watch_status_handlers::update_progress_handler),
        )
        .route(
            v1::watch::STATE,
            get(watch_status_handlers::get_watch_state_handler),
        )
        .route(
            v1::watch::CONTINUE,
            get(watch_status_handlers::get_continue_watching_handler),
        )
        .route(
            v1::watch::CLEAR_PROGRESS,
            axum::routing::delete(watch_status_handlers::clear_progress_handler),
        )
        // Media endpoints
        //
        .route(
            v1::media::item::PROGRESS,
            get(watch_status_handlers::get_media_progress_handler),
        )
        .route(
            v1::media::item::COMPLETE,
            post(watch_status_handlers::mark_completed_handler),
        )
        .route(
            v1::media::item::IS_COMPLETED,
            get(watch_status_handlers::is_completed_handler),
        )
        // Folder inventory monitoring and control
        .route(v1::folders::INVENTORY, get(get_folder_inventory))
        .route(v1::folders::PROGRESS, get(get_scan_progress))
        //.route(
        //    "/folders/rescan/{folder_id}",
        //    post(trigger_folder_rescan),
        //)
        // Query system
        .route(v1::media::QUERY, post(query_media_handler))
        // Scanning: pending-based triggers and counts
        //.route(
        //    "/libraries/{id}/scan/pending",
        //    post(crate::media::scan::scan_handlers::scan_pending_for_library_handler),
        //)
        //.route(
        //    "/libraries/scan/pending",
        //    post(crate::media::scan::scan_handlers::scan_pending_for_all_libraries_handler),
        //)
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
            v1::stream::REPORT_PROGRESS,
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
        .route(v1::sync::WEBSOCKET, axum::routing::any(websocket_handler))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::auth_middleware,
        ))
}

/// Create libraries routes
fn create_libraries_routes(state: AppState) -> Router<AppState> {
    Router::new()
        //.route("/library/events/sse", get(media_events_sse_handler))
        .route(
            v1::libraries::COLLECTION,
            get(get_libraries_with_media_handler).post(create_library_handler),
        )
        .route(v1::libraries::ITEM, get(get_library_handler))
        .route(
            v1::libraries::ITEM,
            axum::routing::put(update_library_handler),
        )
        .route(
            v1::libraries::ITEM,
            axum::routing::delete(delete_library_handler),
        )
        .route(v1::libraries::MEDIA, get(get_library_media_handler))
        .route(
            v1::libraries::SORTED_INDICES,
            get(get_library_sorted_indices_handler),
        )
        .route(
            v1::libraries::FILTERED_INDICES,
            post(post_library_filtered_indices_handler),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::auth_middleware,
        ))
}

fn create_scan_routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route(v1::libraries::scans::START, post(start_scan_handler))
        .route(v1::libraries::scans::PAUSE, post(pause_scan_handler))
        .route(v1::libraries::scans::RESUME, post(resume_scan_handler))
        .route(v1::libraries::scans::CANCEL, post(cancel_scan_handler))
        .route(v1::scan::ACTIVE, get(active_scans_handler))
        .route(v1::scan::HISTORY, get(scan_history_handler))
        .route(v1::scan::PROGRESS, get(latest_progress_handler))
        .route(v1::scan::EVENTS, get(scan_events_handler))
        .route(v1::scan::PROGRESS_STREAM, get(scan_progress_sse_handler))
        .route(v1::scan::METRICS, get(scan_metrics_handler))
        .route(v1::scan::CONFIG, get(scan_config_handler))
        .route(v1::events::MEDIA, get(media_events_sse_handler))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::auth_middleware,
        ))
}

fn create_metadata_routes(state: AppState) -> Router<AppState> {
    Router::new().route(v1::images::SERVE, get(serve_image_handler))
    //.route_layer(middleware::from_fn_with_state(
    //    state.clone(),
    //    auth::middleware::auth_middleware,
    //))
}

/// Create admin routes that require admin role
fn create_admin_routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route(v1::admin::USERS, get(admin_handlers::list_all_users))
        .route(
            v1::admin::USER_ROLES,
            put(admin_handlers::assign_user_roles),
        )
        .route(
            v1::admin::USER_ITEM,
            axum::routing::delete(admin_handlers::delete_user_admin),
        )
        .route(
            v1::admin::USER_SESSIONS,
            get(admin_handlers::get_user_sessions_admin),
        )
        .route(
            v1::admin::REVOKE_SESSION,
            axum::routing::delete(admin_handlers::revoke_user_session_admin),
        )
        .route(v1::admin::STATS, get(admin_handlers::get_admin_stats))
        // Development/reset endpoints (admin only)
        .route(
            v1::admin::dev::RESET_CHECK,
            get(dev_handlers::check_reset_status),
        )
        .route(
            v1::admin::dev::RESET_DATABASE,
            post(dev_handlers::reset_database),
        )
        // Admin session management for PIN authentication
        .route(
            v1::admin::sessions::REGISTER,
            post(auth::pin_handlers::register_admin_session),
        )
        .route(
            v1::admin::sessions::REMOVE,
            axum::routing::delete(auth::pin_handlers::remove_admin_session),
        )
        .route(v1::admin::dev::SEED, post(dev_handlers::seed_database))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::auth_middleware,
        ))
        .route_layer(middleware::from_fn(auth::middleware::admin_middleware))
}

/// Create role management routes
fn create_role_routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route(v1::roles::LIST, get(role_handlers::list_roles_handler))
        .route(
            v1::roles::PERMISSIONS,
            get(role_handlers::list_permissions_handler),
        )
        .route(
            v1::roles::USER_PERMISSIONS,
            get(role_handlers::get_user_permissions_handler),
        )
        .route(
            v1::roles::USER_ROLES,
            put(role_handlers::assign_user_roles_handler),
        )
        .route(
            v1::roles::OVERRIDE_PERMISSION,
            post(role_handlers::override_user_permission_handler),
        )
        .route(
            v1::roles::MY_PERMISSIONS,
            get(role_handlers::get_my_permissions_handler),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::auth_middleware,
        ))
}
