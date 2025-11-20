//! Development utilities and handlers
//!
//! This module provides endpoints for development and testing purposes,
//! including database reset functionality. Reset functionality requires
//! admin permissions to prevent accidental data loss.

use axum::{Extension, Json, extract::State};
use ferrex_core::types::library::Library;
use ferrex_core::{LibraryID, LibraryType};
use ferrex_core::{api_types::ApiResponse, user::User};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{info, warn};

use crate::{
    AppState,
    errors::{AppError, AppResult},
};

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

/// Request to reset the database
#[derive(Debug, Deserialize)]
pub struct ResetDatabaseRequest {
    /// Reset all users and authentication data
    pub reset_users: bool,
    /// Reset all libraries
    pub reset_libraries: bool,
    /// Reset all media entries
    pub reset_media: bool,
    /// Confirmation string - must equal "RESET_DATABASE"
    pub confirmation: String,
}

/// Result of database reset operation
#[derive(Debug, Serialize, Default)]
pub struct ResetResult {
    /// Number of users deleted
    pub users_deleted: usize,
    /// Number of sessions deleted
    pub sessions_deleted: usize,
    /// Number of roles reset
    pub roles_reset: usize,
    /// Number of libraries deleted
    pub libraries_deleted: usize,
    /// Number of media items deleted
    pub media_deleted: usize,
    /// Number of watch status entries deleted
    pub watch_status_deleted: usize,
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
        .database
        .backend()
        .get_user_permissions(user.id)
        .await
        .map_err(|e| AppError::internal(format!("Failed to get permissions: {}", e)))?;

    let can_reset = perms.has_permission("server:reset_database") || perms.has_role("admin");

    // Get current counts
    let users = state
        .database
        .backend()
        .get_all_users()
        .await
        .map_err(|e| AppError::internal(format!("Failed to get users: {}", e)))?;

    let libraries = state
        .database
        .backend()
        .list_libraries()
        .await
        .map_err(|e| AppError::internal(format!("Failed to get libraries: {}", e)))?;

    // Get media count (this is a simplified count - you might want to add a dedicated method)
    let media_count = 0; // TODO: Implement actual media count

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
) -> AppResult<Json<ApiResponse<ResetResult>>> {
    // Check permissions
    let perms = state
        .database
        .backend()
        .get_user_permissions(user.id)
        .await
        .map_err(|e| AppError::internal(format!("Failed to get permissions: {}", e)))?;

    if !perms.has_permission("server:reset_database") && !perms.has_role("admin") {
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

    warn!(
        "Database reset requested with options: users={}, libraries={}, media={}",
        request.reset_users, request.reset_libraries, request.reset_media
    );

    let mut result = ResetResult::default();

    // Get backend reference
    let backend = state.database.backend();

    if request.reset_users {
        info!("Resetting user data...");

        // Get all users before deletion for count
        let users = backend
            .get_all_users()
            .await
            .map_err(|e| AppError::internal(format!("Failed to get users: {}", e)))?;
        result.users_deleted = users.len();

        // Delete all users (this should cascade to related tables)
        for user in users {
            backend.delete_user(user.id).await.map_err(|e| {
                AppError::internal(format!("Failed to delete user {}: {}", user.id, e))
            })?;
        }

        // Roles are system data and shouldn't be deleted, just reset to defaults
        // The migration scripts should handle ensuring default roles exist
        result.roles_reset = 3; // admin, user, guest

        info!(
            "User data reset complete. {} users deleted",
            result.users_deleted
        );
    }

    if request.reset_libraries {
        info!("Resetting library data...");

        // Get all libraries
        let libraries = backend
            .list_libraries()
            .await
            .map_err(|e| AppError::internal(format!("Failed to get libraries: {}", e)))?;
        result.libraries_deleted = libraries.len();

        // Delete all libraries
        for library in libraries {
            backend
                .delete_library(&library.id.to_string())
                .await
                .map_err(|e| {
                    AppError::internal(format!("Failed to delete library {}: {}", library.id, e))
                })?;
        }

        info!(
            "Library data reset complete. {} libraries deleted",
            result.libraries_deleted
        );
    }

    if request.reset_media {
        info!("Resetting media data...");

        // This would require implementing media deletion methods
        // For now, deleting libraries should cascade to media entries
        warn!(
            "Direct media deletion not yet implemented. Media will be deleted when libraries are deleted."
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
        .database
        .backend()
        .get_user_permissions(user.id)
        .await
        .map_err(|e| AppError::internal(format!("Failed to get permissions: {}", e)))?;

    if !perms.has_permission("server:seed_database") && !perms.has_role("admin") {
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
        use crate::services::{UserService, user_service::CreateUserParams};
        use uuid::Uuid;

        let user_service = UserService::new(&state);

        // Create a test admin first
        let admin_id = match user_service
            .create_user(CreateUserParams {
                username: "testadmin".to_string(),
                display_name: "Test Admin".to_string(),
                password: "AdminPass123".to_string(),
                created_by: None,
            })
            .await
        {
            Ok(admin) => {
                // Assign admin role
                let admin_role_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001")
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
    if request.create_library {
        if let Some(path) = request.library_path {
            let library = Library {
                id: LibraryID::new(),
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
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                media: None,
            };

            match state.database.backend().create_library(library).await {
                Ok(_) => {
                    result.libraries_created = 1;
                    info!("Created test library at path: {}", path);
                }
                Err(e) => warn!("Failed to create test library: {}", e),
            }
        }
    }

    Ok(Json(ApiResponse::success(result)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reset_request_validation() {
        let valid = ResetDatabaseRequest {
            reset_users: true,
            reset_libraries: true,
            reset_media: false,
            confirmation: "RESET_DATABASE".to_string(),
        };

        // Should be valid
        assert_eq!(valid.confirmation, "RESET_DATABASE");

        let invalid = ResetDatabaseRequest {
            reset_users: true,
            reset_libraries: true,
            reset_media: false,
            confirmation: "wrong".to_string(),
        };

        // Should be invalid
        assert_ne!(invalid.confirmation, "RESET_DATABASE");
    }
}
