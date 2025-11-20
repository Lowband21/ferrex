use async_trait::async_trait;
use uuid::Uuid;

use crate::error::Result;
use crate::user::User;

// User management and credentials (authentication-adjacent) repository
#[async_trait]
pub trait UsersRepository: Send + Sync {
    async fn create_user_with_password(&self, user: &User, password_hash: &str) -> Result<()>;
    async fn get_user_by_id(&self, id: Uuid) -> Result<Option<User>>;
    async fn get_user_by_username(&self, username: &str) -> Result<Option<User>>;
    async fn get_all_users(&self) -> Result<Vec<User>>;
    async fn update_user(&self, user: &User) -> Result<()>;
    async fn delete_user(&self, id: Uuid) -> Result<()>;
    async fn delete_user_atomic(&self, user_id: Uuid, check_last_admin: bool) -> Result<()>;

    async fn get_user_password_hash(&self, user_id: Uuid) -> Result<Option<String>>;
    async fn update_user_password(&self, user_id: Uuid, password_hash: &str) -> Result<()>;
}
