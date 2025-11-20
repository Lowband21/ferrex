use anyhow::Result;
use async_trait::async_trait;
use ferrex_core::{
    api_routes::{utils, v1},
    player_prelude::User,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::infrastructure::api_client::ApiClient;

#[async_trait]
pub trait UserAdminService: Send + Sync {
    async fn list_users(&self) -> Result<Vec<User>>;
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
    async fn list_users(&self) -> Result<Vec<User>> {
        // Expect server to return Vec<User> at /api/admin/users
        self.client.get(v1::admin::USERS).await
    }

    async fn delete_user(&self, user_id: Uuid) -> Result<()> {
        let path = utils::replace_param(v1::admin::USER_ITEM, "{id}", user_id.to_string());
        let _: serde_json::Value = self.client.delete(&path).await?;
        Ok(())
    }
}
