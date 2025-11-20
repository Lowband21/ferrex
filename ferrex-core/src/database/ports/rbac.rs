use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    Result,
    rbac::{Permission, Role, UserPermissions},
};

#[async_trait]
pub trait RbacRepository: Send + Sync {
    async fn get_user_permissions(&self, user_id: Uuid) -> Result<UserPermissions>;
    async fn get_all_roles(&self) -> Result<Vec<Role>>;
    async fn get_all_permissions(&self) -> Result<Vec<Permission>>;
    async fn assign_user_role(&self, user_id: Uuid, role_id: Uuid, granted_by: Uuid) -> Result<()>;
    async fn remove_user_role(&self, user_id: Uuid, role_id: Uuid) -> Result<()>;
    async fn remove_user_role_atomic(
        &self,
        user_id: Uuid,
        role_id: Uuid,
        check_last_admin: bool,
    ) -> Result<()>;
    async fn override_user_permission(
        &self,
        user_id: Uuid,
        permission: &str,
        granted: bool,
        granted_by: Uuid,
        reason: Option<String>,
    ) -> Result<()>;

    async fn get_admin_count(&self, exclude_user_id: Option<Uuid>) -> Result<usize>;
    async fn user_has_role(&self, user_id: Uuid, role_name: &str) -> Result<bool>;
    async fn get_users_with_role(&self, role_name: &str) -> Result<Vec<Uuid>>;
}
