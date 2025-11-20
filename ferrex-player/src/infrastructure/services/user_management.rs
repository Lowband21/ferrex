use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

use crate::infrastructure::api_client::ApiClient;

#[async_trait]
pub trait UserAdminService: Send + Sync {
    async fn list_users(&self) -> Result<Vec<ferrex_core::user::User>>;
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
    async fn list_users(&self) -> Result<Vec<ferrex_core::user::User>> {
        // Expect server to return Vec<User> at /api/admin/users
        self.client.get("/admin/users").await
    }

    async fn delete_user(&self, user_id: Uuid) -> Result<()> {
        let path = format!("/admin/users/{}", user_id);
        let _: serde_json::Value = self.client.delete(&path).await?;
        Ok(())
    }
}
