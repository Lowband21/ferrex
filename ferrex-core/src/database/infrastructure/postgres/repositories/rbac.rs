use async_trait::async_trait;
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use uuid::Uuid;

use crate::database::ports::rbac::RbacRepository;
use crate::domain::users::rbac::{
    Permission, PermissionCategory, Role, UserPermissions,
};
use crate::error::{MediaError, Result};

/// PostgreSQL-backed implementation of RBAC repository operations.
#[derive(Clone, Debug)]
pub struct PostgresRbacRepository {
    pool: PgPool,
}

impl PostgresRbacRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[async_trait]
impl RbacRepository for PostgresRbacRepository {
    /// Get all permissions for a user (from roles + overrides)
    async fn get_user_permissions(
        &self,
        user_id: Uuid,
    ) -> Result<UserPermissions> {
        // First, get all roles for the user
        let roles: Vec<Role> = sqlx::query_as!(
            Role,
            r#"
            SELECT r.id, r.name, r.description, r.is_system as "is_system!",
                   EXTRACT(EPOCH FROM r.created_at)::BIGINT as "created_at!"
            FROM roles r
            INNER JOIN user_roles ur ON r.id = ur.role_id
            WHERE ur.user_id = $1
            ORDER BY r.name
            "#,
            user_id
        )
        .fetch_all(self.pool())
        .await?;

        // Get all permissions from roles
        let role_permissions: Vec<(String, bool)> = sqlx::query!(
            r#"
            SELECT DISTINCT p.name, true as granted
            FROM permissions p
            INNER JOIN role_permissions rp ON p.id = rp.permission_id
            INNER JOIN user_roles ur ON rp.role_id = ur.role_id
            WHERE ur.user_id = $1
            "#,
            user_id
        )
        .fetch_all(self.pool())
        .await?
        .into_iter()
        .map(|row| (row.name, row.granted.unwrap_or(true)))
        .collect();

        // Get user-specific permission overrides
        let user_overrides: Vec<(String, bool)> = sqlx::query!(
            r#"
            SELECT p.name, up.granted
            FROM user_permissions up
            INNER JOIN permissions p ON up.permission_id = p.id
            WHERE up.user_id = $1
            "#,
            user_id
        )
        .fetch_all(self.pool())
        .await?
        .into_iter()
        .map(|row| (row.name, row.granted))
        .collect();

        // Build the final permissions map
        let mut permissions = HashMap::new();

        // First add all role permissions
        for (name, granted) in role_permissions {
            permissions.insert(name, granted);
        }

        // Then apply user overrides (these take precedence)
        for (name, granted) in user_overrides {
            permissions.insert(name, granted);
        }

        Ok(UserPermissions {
            user_id,
            roles,
            permissions,
            permission_details: None, // We'll populate this if needed
        })
    }

    async fn get_all_roles(&self) -> Result<Vec<Role>> {
        let roles = sqlx::query_as!(
            Role,
            r#"
            SELECT id, name, description, is_system as "is_system!",
                   EXTRACT(EPOCH FROM created_at)::BIGINT as "created_at!"
            FROM roles
            ORDER BY name
            "#
        )
        .fetch_all(self.pool())
        .await?;

        Ok(roles)
    }

    async fn get_all_permissions(&self) -> Result<Vec<Permission>> {
        let permissions = sqlx::query_as!(
            Permission,
            r#"
            SELECT id, name, category, description
            FROM permissions
            ORDER BY category, name
            "#
        )
        .fetch_all(self.pool())
        .await?;

        Ok(permissions)
    }

    async fn assign_user_role(
        &self,
        user_id: Uuid,
        role_id: Uuid,
        granted_by: Uuid,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO user_roles (user_id, role_id, granted_by, granted_at)
            VALUES ($1, $2, $3, NOW())
            ON CONFLICT (user_id, role_id) DO NOTHING
            "#,
            user_id,
            role_id,
            granted_by
        )
        .execute(self.pool())
        .await?;

        Ok(())
    }

    async fn remove_user_role(
        &self,
        user_id: Uuid,
        role_id: Uuid,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            DELETE FROM user_roles
            WHERE user_id = $1 AND role_id = $2
            "#,
            user_id,
            role_id
        )
        .execute(self.pool())
        .await?;

        Ok(())
    }

    /// Remove user role with atomic check for last admin
    async fn remove_user_role_atomic(
        &self,
        user_id: Uuid,
        role_id: Uuid,
        check_last_admin: bool,
    ) -> Result<()> {
        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!("Failed to start transaction: {}", e))
        })?;

        if check_last_admin {
            // Check if this is the admin role
            let is_admin_role: bool = sqlx::query_scalar!(
                r#"
                SELECT EXISTS(
                    SELECT 1 FROM roles
                    WHERE id = $1 AND name = 'admin'
                ) as "exists!"
                "#,
                role_id
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to check if admin role: {}",
                    e
                ))
            })?;

            if is_admin_role {
                // Lock all admin users except the one whose role is being removed
                let admin_users: Vec<Uuid> = sqlx::query_scalar!(
                    r#"
                    SELECT u.id
                    FROM users u
                    INNER JOIN user_roles ur ON u.id = ur.user_id
                    INNER JOIN roles r ON ur.role_id = r.id
                    WHERE r.name = 'admin'
                    AND u.id != $1
                    FOR UPDATE OF u
                    "#,
                    user_id
                )
                .fetch_all(&mut *tx)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!(
                        "Failed to lock admin users: {}",
                        e
                    ))
                })?;

                if admin_users.is_empty() {
                    tx.rollback().await.map_err(|e| {
                        MediaError::Internal(format!(
                            "Failed to rollback transaction: {}",
                            e
                        ))
                    })?;
                    return Err(MediaError::Conflict(
                        "Cannot remove admin role from the last admin"
                            .to_string(),
                    ));
                }
            }
        }

        // Remove the role
        let result = sqlx::query!(
            r#"
            DELETE FROM user_roles
            WHERE user_id = $1 AND role_id = $2
            "#,
            user_id,
            role_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to remove user role: {}", e))
        })?;

        if result.rows_affected() == 0 {
            tx.rollback().await.map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to rollback transaction: {}",
                    e
                ))
            })?;
            return Err(MediaError::NotFound(
                "User role assignment not found".to_string(),
            ));
        }

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!("Failed to commit transaction: {}", e))
        })?;

        tracing::info!(
            "Atomically removed role {} from user {} with last admin check: {}",
            role_id,
            user_id,
            check_last_admin
        );
        Ok(())
    }

    async fn override_user_permission(
        &self,
        user_id: Uuid,
        permission: &str,
        granted: bool,
        granted_by: Uuid,
        reason: Option<String>,
    ) -> Result<()> {
        // First get the permission ID
        let permission_id = sqlx::query!(
            r#"
            SELECT id FROM permissions WHERE name = $1
            "#,
            permission
        )
        .fetch_optional(self.pool())
        .await?
        .ok_or_else(|| {
            MediaError::NotFound(format!(
                "Permission '{}' not found",
                permission
            ))
        })?
        .id;

        // Insert or update the override
        sqlx::query!(
            r#"
            INSERT INTO user_permissions (user_id, permission_id, granted, granted_by, granted_at, reason)
            VALUES ($1, $2, $3, $4, NOW(), $5)
            ON CONFLICT (user_id, permission_id)
            DO UPDATE SET
                granted = EXCLUDED.granted,
                granted_by = EXCLUDED.granted_by,
                granted_at = EXCLUDED.granted_at,
                reason = EXCLUDED.reason
            "#,
            user_id,
            permission_id,
            granted,
            granted_by,
            reason
        )
        .execute(self.pool())
        .await?;

        Ok(())
    }

    async fn get_admin_count(
        &self,
        exclude_user_id: Option<Uuid>,
    ) -> Result<usize> {
        let count = if let Some(exclude_id) = exclude_user_id {
            sqlx::query_scalar!(
                r#"
                SELECT COUNT(DISTINCT u.id) as "count!"
                FROM users u
                INNER JOIN user_roles ur ON u.id = ur.user_id
                INNER JOIN roles r ON ur.role_id = r.id
                WHERE r.name = 'admin'
                AND u.id != $1
                "#,
                exclude_id
            )
            .fetch_one(self.pool())
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Failed to count admins: {}", e))
            })?
        } else {
            sqlx::query_scalar!(
                r#"
                SELECT COUNT(DISTINCT u.id) as "count!"
                FROM users u
                INNER JOIN user_roles ur ON u.id = ur.user_id
                INNER JOIN roles r ON ur.role_id = r.id
                WHERE r.name = 'admin'
                "#
            )
            .fetch_one(self.pool())
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Failed to count admins: {}", e))
            })?
        };

        Ok(count as usize)
    }

    async fn user_has_role(
        &self,
        user_id: Uuid,
        role_name: &str,
    ) -> Result<bool> {
        let has_role = sqlx::query_scalar!(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM user_roles ur
                INNER JOIN roles r ON ur.role_id = r.id
                WHERE ur.user_id = $1 AND r.name = $2
            ) as "exists!"
            "#,
            user_id,
            role_name
        )
        .fetch_one(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to check user role: {}", e))
        })?;

        Ok(has_role)
    }

    async fn get_users_with_role(&self, role_name: &str) -> Result<Vec<Uuid>> {
        let user_ids = sqlx::query_scalar!(
            r#"
            SELECT u.id
            FROM users u
            INNER JOIN user_roles ur ON u.id = ur.user_id
            INNER JOIN roles r ON ur.role_id = r.id
            WHERE r.name = $1
            "#,
            role_name
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to get users with role: {}",
                e
            ))
        })?;

        Ok(user_ids)
    }

    async fn upsert_role(
        &self,
        role_id: Uuid,
        name: &str,
        description: &str,
        is_system: bool,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO roles (id, name, description, is_system)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (name) DO UPDATE
            SET description = EXCLUDED.description,
                is_system = EXCLUDED.is_system
            "#,
        )
        .bind(role_id)
        .bind(name)
        .bind(description)
        .bind(is_system)
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to upsert role '{}': {}",
                name, e
            ))
        })?;

        Ok(())
    }

    async fn upsert_permission(
        &self,
        name: &str,
        category: PermissionCategory,
        description: &str,
    ) -> Result<Uuid> {
        let row = sqlx::query(
            r#"
            INSERT INTO permissions (name, category, description)
            VALUES ($1, $2, $3)
            ON CONFLICT (name) DO UPDATE
            SET category = EXCLUDED.category,
                description = EXCLUDED.description
            RETURNING id
            "#,
        )
        .bind(name)
        .bind(category.as_str())
        .bind(description)
        .fetch_one(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to upsert permission '{}': {}",
                name, e
            ))
        })?;

        Ok(row.try_get("id")?)
    }

    async fn assign_permission_to_role(
        &self,
        role_id: Uuid,
        permission_id: Uuid,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO role_permissions (role_id, permission_id)
            VALUES ($1, $2)
            ON CONFLICT (role_id, permission_id) DO NOTHING
            "#,
        )
        .bind(role_id)
        .bind(permission_id)
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to assign permission {} to role {}: {}",
                permission_id, role_id, e
            ))
        })?;

        Ok(())
    }
}
