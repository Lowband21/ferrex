use crate::{
    Result,
    database::{PostgresDatabase, ports::rbac::RbacRepository},
    rbac::{Permission, Role, UserPermissions},
};
use uuid::Uuid;

impl PostgresDatabase {
    pub async fn rbac_get_user_permissions(&self, user_id: Uuid) -> Result<UserPermissions> {
        self.rbac_repository().get_user_permissions(user_id).await
    }

    pub async fn rbac_get_all_roles(&self) -> Result<Vec<Role>> {
        self.rbac_repository().get_all_roles().await
    }

    pub async fn rbac_get_all_permissions(&self) -> Result<Vec<Permission>> {
        self.rbac_repository().get_all_permissions().await
    }

    pub async fn rbac_assign_user_role(
        &self,
        user_id: Uuid,
        role_id: Uuid,
        granted_by: Uuid,
    ) -> Result<()> {
        self.rbac_repository()
            .assign_user_role(user_id, role_id, granted_by)
            .await
    }

    pub async fn rbac_remove_user_role(&self, user_id: Uuid, role_id: Uuid) -> Result<()> {
        self.rbac_repository()
            .remove_user_role(user_id, role_id)
            .await
    }

    pub async fn rbac_remove_user_role_atomic(
        &self,
        user_id: Uuid,
        role_id: Uuid,
        check_last_admin: bool,
    ) -> Result<()> {
        self.rbac_repository()
            .remove_user_role_atomic(user_id, role_id, check_last_admin)
            .await
    }

    pub async fn rbac_override_user_permission(
        &self,
        user_id: Uuid,
        permission: &str,
        granted: bool,
        granted_by: Uuid,
        reason: Option<String>,
    ) -> Result<()> {
        self.rbac_repository()
            .override_user_permission(user_id, permission, granted, granted_by, reason)
            .await
    }
}
