//! Development utilities and handlers
//!
//! This module provides endpoints for development and testing purposes,
//! including database reset functionality. Reset functionality requires
//! admin permissions to prevent accidental data loss.

use ferrex_model::MovieReferenceBatchSize;

use ferrex_core::{
    api::types::{
        ApiResponse,
        admin::{ResetDatabaseRequest, ResetDatabaseResult},
    },
    domain::users::user::User,
    types::{LibraryId, LibraryType, library::Library},
};

use crate::{
    handlers::media::handle_library::delete_library_with_runtime_cleanup,
    handlers::users::{UserService, user_service::CreateUserParams},
    infra::{
        app_state::AppState,
        demo_mode,
        errors::{AppError, AppResult},
    },
};

use axum::{Extension, Json, extract::State};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::path::PathBuf;
use tracing::{info, warn};
use uuid::Uuid;

/// Response for reset check endpoint
#[derive(Debug, Serialize)]
pub struct ResetCheckResponse {
    /// Whether the server is in development mode
    pub is_development: bool,
    /// Whether reset functionality is available
    pub can_reset: bool,
    /// Current number of users
    pub user_count: usize,
    /// Current number of libraries
    pub library_count: usize,
    /// Current number of media items
    pub media_count: usize,
}

#[derive(Debug, Default)]
struct UserCleanupCounts {
    sessions: usize,
    watch_status: usize,
}

#[derive(Debug, Default)]
struct FullClearCounts {
    intelligence_root_rows: u64,
    collection_root_rows: u64,
    maintenance_rows: u64,
    media_cache_root_rows: u64,
}

fn is_full_clear(request: &ResetDatabaseRequest) -> bool {
    request.reset_users && request.reset_libraries && request.reset_media
}

fn invoking_user_last(
    mut users: Vec<User>,
    invoking_user_id: Uuid,
) -> Vec<User> {
    if let Some(index) =
        users.iter().position(|user| user.id == invoking_user_id)
    {
        let invoking_user = users.remove(index);
        users.push(invoking_user);
    }
    users
}

fn checked_i64_count(value: i64, label: &str) -> AppResult<usize> {
    usize::try_from(value).map_err(|_| {
        AppError::internal(format!(
            "Invalid {label} row count returned by database"
        ))
    })
}

fn checked_u64_count(value: u64, label: &str) -> AppResult<usize> {
    usize::try_from(value).map_err(|_| {
        AppError::internal(format!(
            "{label} row count exceeds this platform's limit"
        ))
    })
}

async fn count_media_items(
    pool: &PgPool,
    library_ids: &[Uuid],
) -> AppResult<usize> {
    if library_ids.is_empty() {
        return Ok(0);
    }

    let count = sqlx::query_scalar!(
        r#"
        SELECT (
            (SELECT COUNT(*) FROM movie_references WHERE library_id = ANY($1))
            + (SELECT COUNT(*) FROM series WHERE library_id = ANY($1))
            + (SELECT COUNT(*) FROM season_references WHERE library_id = ANY($1))
            + (
                SELECT COUNT(*)
                FROM episode_references er
                JOIN series s ON s.id = er.series_id
                WHERE s.library_id = ANY($1)
            )
        )::bigint AS "count!"
        "#,
        library_ids,
    )
    .fetch_one(pool)
    .await
    .map_err(AppError::from)?;

    checked_i64_count(count, "media")
}

async fn clear_full_wipe_roots(pool: &PgPool) -> AppResult<FullClearCounts> {
    let mut tx = pool.begin().await.map_err(AppError::from)?;
    let mut counts = FullClearCounts::default();

    // Provenance edges have kind-specific CHECK constraints. Delete them
    // before any referenced intelligence row can be nulled by an FK action.
    counts.intelligence_root_rows +=
        sqlx::query!("DELETE FROM intelligence_artifact_sources")
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?
            .rows_affected();
    counts.intelligence_root_rows +=
        sqlx::query!("DELETE FROM intelligence_artifacts")
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?
            .rows_affected();
    counts.intelligence_root_rows +=
        sqlx::query!("DELETE FROM intelligence_runs")
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?
            .rows_affected();
    counts.intelligence_root_rows +=
        sqlx::query!("DELETE FROM intelligence_search_documents")
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?
            .rows_affected();
    counts.intelligence_root_rows +=
        sqlx::query!("DELETE FROM intelligence_media_context")
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?
            .rows_affected();

    // Shelf placements and imported sources intentionally outlive collection
    // definitions, so they are roots rather than collection-owned children.
    counts.collection_root_rows +=
        sqlx::query!("DELETE FROM collection_shelf_placements")
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?
            .rows_affected();
    counts.collection_root_rows +=
        sqlx::query!("DELETE FROM collection_sources")
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?
            .rows_affected();
    counts.collection_root_rows +=
        sqlx::query!("DELETE FROM collection_definitions")
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?
            .rows_affected();

    counts.maintenance_rows +=
        sqlx::query!("DELETE FROM library_maintenance_operations")
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?
            .rows_affected();

    tx.commit().await.map_err(AppError::from)?;
    Ok(counts)
}

async fn clear_global_media_caches(
    pool: &PgPool,
    counts: &mut FullClearCounts,
) -> AppResult<()> {
    let mut tx = pool.begin().await.map_err(AppError::from)?;

    counts.media_cache_root_rows += sqlx::query!("DELETE FROM cached_images")
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?
        .rows_affected();
    counts.media_cache_root_rows +=
        sqlx::query!("DELETE FROM tmdb_image_variants")
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?
            .rows_affected();
    counts.media_cache_root_rows += sqlx::query!("DELETE FROM persons")
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?
        .rows_affected();

    tx.commit().await.map_err(AppError::from)?;
    Ok(())
}

async fn prepare_user_reset(
    pool: &PgPool,
    invoking_user_id: Uuid,
) -> AppResult<UserCleanupCounts> {
    let mut tx = pool.begin().await.map_err(AppError::from)?;

    // These sync FKs use restrictive defaults instead of cascading from users.
    sqlx::query!("DELETE FROM sync_session_history")
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
    sqlx::query!("DELETE FROM sync_sessions")
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;

    // A grantor can otherwise be kept alive by grants owned by another user.
    // Preserve the grants themselves so the invoking administrator retains
    // recovery authority until their user is deliberately deleted last.
    sqlx::query!("UPDATE user_permissions SET granted_by = NULL")
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
    sqlx::query!("UPDATE user_roles SET granted_by = NULL")
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;

    // A device session can also cascade through `first_authenticated_by`, even
    // when it belongs to the invoking user. Anchor their devices to their owner
    // and detach their bearer sessions from the secondary device FK so deleting
    // another user cannot invalidate the administrator's recovery session.
    sqlx::query!(
        r#"
        UPDATE auth_device_sessions
        SET first_authenticated_by = user_id
        WHERE user_id = $1
          AND first_authenticated_by <> user_id
        "#,
        invoking_user_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(AppError::from)?;
    sqlx::query!(
        r#"
        UPDATE auth_sessions
        SET device_session_id = NULL
        WHERE user_id = $1
        "#,
        invoking_user_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(AppError::from)?;

    // Count sessions now but leave them user-owned. Their FK cascade runs with
    // each user deletion, which keeps the invoking administrator's sessions
    // usable if an earlier user cannot be deleted.
    let sessions = sqlx::query_scalar!(
        r#"SELECT COUNT(*)::bigint AS "count!" FROM auth_sessions"#
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(AppError::from)?;
    let watch_status = sqlx::query!("DELETE FROM user_watch_progress")
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?
        .rows_affected()
        + sqlx::query!("DELETE FROM user_completed_media")
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?
            .rows_affected()
        + sqlx::query!("DELETE FROM user_episode_state")
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?
            .rows_affected()
        + sqlx::query!("DELETE FROM user_view_history")
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?
            .rows_affected();

    // These operational/authentication records either have SET NULL ownership
    // or no owner FK and would otherwise survive a first-run reset.
    sqlx::query!("DELETE FROM auth_events")
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
    sqlx::query!("DELETE FROM security_audit_log")
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
    sqlx::query!("DELETE FROM login_attempts")
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
    sqlx::query!("DELETE FROM rate_limit_state")
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
    sqlx::query!("DELETE FROM setup_claims")
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
    sqlx::query!("DELETE FROM auth_security_settings")
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;

    tx.commit().await.map_err(AppError::from)?;

    Ok(UserCleanupCounts {
        sessions: checked_i64_count(sessions, "session")?,
        watch_status: checked_u64_count(watch_status, "watch status")?,
    })
}

/// Check if database reset is available
///
/// This endpoint returns information about the current database state
/// and whether reset functionality is available.
pub async fn check_reset_status(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
) -> AppResult<Json<ApiResponse<ResetCheckResponse>>> {
    // Check if user has admin permissions
    let perms = state
        .unit_of_work()
        .rbac
        .get_user_permissions(user.id)
        .await
        .map_err(|e| {
            AppError::internal(format!("Failed to get permissions: {}", e))
        })?;

    let can_reset = perms.has_permission("server:reset_database")
        || perms.has_role("admin");

    // Get current counts
    let users =
        state
            .unit_of_work()
            .users
            .get_all_users()
            .await
            .map_err(|e| {
                AppError::internal(format!("Failed to get users: {}", e))
            })?;

    let libraries = state
        .unit_of_work()
        .libraries
        .list_libraries()
        .await
        .map_err(|e| {
            AppError::internal(format!("Failed to get libraries: {}", e))
        })?;
    let libraries = demo_mode::filter_libraries(&state, libraries);
    let library_ids = libraries
        .iter()
        .map(|library| *library.id.as_uuid())
        .collect::<Vec<_>>();
    let postgres = state.postgres();
    let media_count = count_media_items(postgres.pool(), &library_ids).await?;

    let response = ResetCheckResponse {
        is_development: cfg!(debug_assertions),
        can_reset,
        user_count: users.len(),
        library_count: libraries.len(),
        media_count,
    };

    Ok(Json(ApiResponse::success(response)))
}

/// Reset the database for testing
///
/// This endpoint allows resetting various parts of the database
/// to restore the first-run experience. Requires admin permissions.
pub async fn reset_database(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<ResetDatabaseRequest>,
) -> AppResult<Json<ApiResponse<ResetDatabaseResult>>> {
    // Check permissions
    let perms = state
        .unit_of_work()
        .rbac
        .get_user_permissions(user.id)
        .await
        .map_err(|e| {
            AppError::internal(format!("Failed to get permissions: {}", e))
        })?;

    if !perms.has_permission("server:reset_database")
        && !perms.has_role("admin")
    {
        return Err(AppError::forbidden(
            "Database reset requires admin permissions",
        ));
    }

    // Verify confirmation
    if request.confirmation != "RESET_DATABASE" {
        return Err(AppError::bad_request(
            "Invalid confirmation. Must be 'RESET_DATABASE'",
        ));
    }

    if request.reset_media && !request.reset_libraries {
        return Err(AppError::bad_request(
            "Media reset requires library reset so runtime state is cleaned safely",
        ));
    }

    warn!(
        "Database reset requested with options: users={}, libraries={}, media={}",
        request.reset_users, request.reset_libraries, request.reset_media
    );

    let full_clear = is_full_clear(&request);
    let invoking_user_id = user.id;
    let postgres = state.postgres();
    let pool = postgres.pool();
    let mut result = ResetDatabaseResult::default();

    // Resolve and count every target before mutating the database. This keeps
    // the response tied to the exact library/user set selected for this run.
    let libraries = if request.reset_libraries {
        let libraries = state
            .unit_of_work()
            .libraries
            .list_libraries()
            .await
            .map_err(|e| {
            AppError::internal(format!("Failed to get libraries: {e}"))
        })?;
        demo_mode::filter_libraries(&state, libraries)
    } else {
        Vec::new()
    };
    let library_ids = libraries
        .iter()
        .map(|library| *library.id.as_uuid())
        .collect::<Vec<_>>();
    let media_count = count_media_items(pool, &library_ids).await?;

    let users = if request.reset_users {
        state
            .unit_of_work()
            .users
            .get_all_users()
            .await
            .map_err(|e| {
                AppError::internal(format!("Failed to get users: {e}"))
            })?
    } else {
        Vec::new()
    };

    // Full clear includes detached/global roots which do not cascade from a
    // library or user. Removing provenance/collection roots first also avoids
    // their restrictive CHECK/FK combinations blocking the owner deletions.
    let mut full_clear_counts = if full_clear {
        Some(clear_full_wipe_roots(pool).await?)
    } else {
        None
    };

    // Delete library-owned data before users. A failed library cascade must not
    // leave the installation without the administrator who can repair it.
    if request.reset_libraries {
        info!("Resetting library data...");

        for library in libraries {
            delete_library_with_runtime_cleanup(&state, library.id)
                .await
                .map_err(|e| {
                    AppError::internal(format!(
                        "Failed to delete library {}: {}",
                        library.id, e
                    ))
                })?;
            result.libraries_deleted += 1;
        }
        result.media_deleted = media_count;

        info!(
            "Library data reset complete. {} libraries and {} media items deleted",
            result.libraries_deleted, result.media_deleted
        );
    }

    if let Some(counts) = full_clear_counts.as_mut() {
        // Global image/person caches are no longer referenced after all library
        // rows have been removed, so they can now be deleted transactionally.
        clear_global_media_caches(pool, counts).await?;
    }

    if request.reset_users {
        info!("Resetting user data...");

        let cleanup_counts = prepare_user_reset(pool, invoking_user_id).await?;

        // Preserve the invoking administrator and their role until every other
        // user deletion succeeds. If an earlier delete fails, they can log in
        // again and retry or repair the remaining state.
        for user in invoking_user_last(users, invoking_user_id) {
            state
                .unit_of_work()
                .users
                .delete_user(user.id)
                .await
                .map_err(|e| {
                    AppError::internal(format!(
                        "Failed to delete user {}: {}",
                        user.id, e
                    ))
                })?;
            result.users_deleted += 1;
        }

        result.sessions_deleted = cleanup_counts.sessions;
        result.watch_status_deleted = cleanup_counts.watch_status;

        info!(
            "User data reset complete. {} users, {} sessions, and {} watch rows deleted",
            result.users_deleted,
            result.sessions_deleted,
            result.watch_status_deleted
        );
    }

    if request.reset_media {
        info!(
            "Media reset completed through library-owned cascades: {} logical media items deleted",
            result.media_deleted
        );
    }

    if let Some(counts) = full_clear_counts {
        info!(
            intelligence_root_rows = counts.intelligence_root_rows,
            collection_root_rows = counts.collection_root_rows,
            maintenance_rows = counts.maintenance_rows,
            media_cache_root_rows = counts.media_cache_root_rows,
            "Detached/global clear-all data deleted"
        );
    }

    info!("Database reset completed successfully");

    Ok(Json(ApiResponse::success(result)))
}

/// Seed the database with test data
///
/// This endpoint can be used to quickly populate the database
/// with test data for development purposes.
#[derive(Debug, Deserialize)]
pub struct SeedDatabaseRequest {
    /// Number of test users to create
    pub user_count: usize,
    /// Create a test library
    pub create_library: bool,
    /// Library path (if create_library is true)
    pub library_path: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SeedResult {
    /// Users created
    pub users_created: usize,
    /// Libraries created
    pub libraries_created: usize,
}

pub async fn seed_database(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(request): Json<SeedDatabaseRequest>,
) -> AppResult<Json<ApiResponse<SeedResult>>> {
    // Check permissions
    let perms = state
        .unit_of_work()
        .rbac
        .get_user_permissions(user.id)
        .await
        .map_err(|e| {
            AppError::internal(format!("Failed to get permissions: {}", e))
        })?;

    if !perms.has_permission("server:seed_database") && !perms.has_role("admin")
    {
        return Err(AppError::forbidden(
            "Database seeding requires admin permissions",
        ));
    }

    let mut result = SeedResult {
        users_created: 0,
        libraries_created: 0,
    };

    // Create test users
    if request.user_count > 0 {
        use uuid::Uuid;

        let user_service = UserService::new(&state);

        // Create a test admin first
        let admin_id = match user_service
            .create_user(CreateUserParams {
                username: "testadmin".to_string(),
                display_name: "Test Admin".to_string(),
                password: "AdminPass123".to_string(),
                email: None,
                avatar_url: None,
                role_ids: Vec::new(),
                is_active: true,
                created_by: None,
            })
            .await
        {
            Ok(admin) => {
                // Assign admin role
                let admin_role_id =
                    Uuid::parse_str("00000000-0000-0000-0000-000000000001")
                        .expect("Invalid admin role UUID");
                user_service
                    .assign_role(admin.id, admin_role_id, admin.id)
                    .await?;
                result.users_created += 1;
                admin.id
            }
            Err(e) => {
                warn!("Failed to create test admin (may already exist): {}", e);
                Uuid::nil()
            }
        };

        // Create regular test users
        for i in 1..request.user_count {
            match user_service
                .create_user(CreateUserParams {
                    username: format!("testuser{}", i),
                    display_name: format!("Test User {}", i),
                    password: format!("{:04}", i), // 4-digit PIN
                    email: None,
                    avatar_url: None,
                    role_ids: Vec::new(),
                    is_active: true,
                    created_by: Some(admin_id),
                })
                .await
            {
                Ok(_) => result.users_created += 1,
                Err(e) => warn!("Failed to create test user {}: {}", i, e),
            }
        }
    }

    // Create test library
    if request.create_library
        && let Some(path) = request.library_path
    {
        let library = Library {
            id: LibraryId::new(),
            name: "Test Library".to_string(),
            library_type: LibraryType::Movies,
            paths: vec![PathBuf::from(path.clone())],
            scan_interval_minutes: 60,
            last_scan: None,
            enabled: true,
            auto_scan: true,
            watch_for_changes: false,
            analyze_on_scan: true,
            max_retry_attempts: 3,
            movie_ref_batch_size: MovieReferenceBatchSize::default(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            media: None,
        };

        match state.unit_of_work().libraries.create_library(library).await {
            Ok(_) => {
                result.libraries_created = 1;
                info!("Created test library at path: {}", path);
            }
            Err(e) => warn!("Failed to create test library: {}", e),
        }
    }

    Ok(Json(ApiResponse::success(result)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_user(id: Uuid, username: &str) -> User {
        let now = chrono::Utc::now();
        User {
            id,
            username: username.to_string(),
            display_name: username.to_string(),
            avatar_url: None,
            created_at: now,
            updated_at: now,
            last_login: None,
            is_active: true,
            email: None,
            preferences: Default::default(),
        }
    }

    #[test]
    fn test_reset_request_validation() {
        let valid = ResetDatabaseRequest::clear_all_data();

        assert!(is_full_clear(&valid));
        assert!(valid.reset_users);
        assert!(valid.reset_libraries);
        assert!(valid.reset_media);
        assert_eq!(valid.confirmation, "RESET_DATABASE");

        let invalid = ResetDatabaseRequest {
            reset_users: true,
            reset_libraries: true,
            reset_media: false,
            confirmation: "wrong".to_string(),
        };

        // Should be invalid
        assert_ne!(invalid.confirmation, "RESET_DATABASE");
        assert!(!is_full_clear(&invalid));
    }

    #[test]
    fn invoking_administrator_is_always_deleted_last() {
        let first_id = Uuid::new_v4();
        let invoking_id = Uuid::new_v4();
        let third_id = Uuid::new_v4();
        let users = vec![
            test_user(invoking_id, "invoking"),
            test_user(first_id, "first"),
            test_user(third_id, "third"),
        ];

        let ordered = invoking_user_last(users, invoking_id);
        let ordered_ids =
            ordered.iter().map(|user| user.id).collect::<Vec<_>>();

        assert_eq!(ordered_ids, vec![first_id, third_id, invoking_id]);
    }

    #[test]
    fn missing_invoking_user_does_not_reorder_targets() {
        let first_id = Uuid::new_v4();
        let second_id = Uuid::new_v4();
        let users =
            vec![test_user(first_id, "first"), test_user(second_id, "second")];

        let ordered = invoking_user_last(users, Uuid::new_v4());
        let ordered_ids =
            ordered.iter().map(|user| user.id).collect::<Vec<_>>();

        assert_eq!(ordered_ids, vec![first_id, second_id]);
    }
}
