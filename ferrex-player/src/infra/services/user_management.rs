use anyhow::Result;
use async_trait::async_trait;
use ferrex_core::api::routes::{utils, v1};
use std::sync::Arc;
use uuid::Uuid;

use crate::infra::{
    api_client::ApiClient,
    api_types::{AdminUserInfo, CreateUserRequest, UpdateUserRequest},
};

// DTOs imported from ferrex-core::api::types::users_admin

#[async_trait]
pub trait UserAdminService: Send + Sync {
    /// List users for the admin panel
    async fn list_users(&self) -> Result<Vec<AdminUserInfo>>;

    /// Create a new user (admin)
    async fn create_user(
        &self,
        req: CreateUserRequest,
    ) -> Result<AdminUserInfo>;

    /// Update an existing user (admin)
    async fn update_user(
        &self,
        user_id: Uuid,
        req: UpdateUserRequest,
    ) -> Result<AdminUserInfo>;

    async fn delete_user(&self, user_id: Uuid) -> Result<()>;
}

#[derive(Clone)]
pub struct UserAdminApiAdapter {
    client: Arc<ApiClient>,
}

impl UserAdminApiAdapter {
    pub fn new(client: Arc<ApiClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl UserAdminService for UserAdminApiAdapter {
    async fn list_users(&self) -> Result<Vec<AdminUserInfo>> {
        // Server returns Vec<AdminUserInfo> at /api/v1/admin/users
        self.client.get(v1::admin::USERS).await
    }

    async fn create_user(
        &self,
        req: CreateUserRequest,
    ) -> Result<AdminUserInfo> {
        self.client.post(v1::admin::USERS, &req).await
    }

    async fn update_user(
        &self,
        user_id: Uuid,
        req: UpdateUserRequest,
    ) -> Result<AdminUserInfo> {
        let path = utils::replace_param(
            v1::admin::USER_ITEM,
            "{id}",
            user_id.to_string(),
        );
        self.client.put(&path, &req).await
    }

    async fn delete_user(&self, user_id: Uuid) -> Result<()> {
        let path = utils::replace_param(
            v1::admin::USER_ITEM,
            "{id}",
            user_id.to_string(),
        );
        let _: serde_json::Value = self.client.delete(&path).await?;
        Ok(())
    }
}
