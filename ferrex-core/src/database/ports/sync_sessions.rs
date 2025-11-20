use async_trait::async_trait;
use uuid::Uuid;

use crate::{Participant, PlaybackState, Result, SyncSession};

#[async_trait]
pub trait SyncSessionsRepository: Send + Sync {
    async fn create_sync_session(&self, session: &SyncSession) -> Result<()>;
    async fn get_sync_session_by_code(&self, room_code: &str) -> Result<Option<SyncSession>>;
    async fn get_sync_session(&self, id: Uuid) -> Result<Option<SyncSession>>;
    async fn update_sync_session_state(&self, id: Uuid, state: &PlaybackState) -> Result<()>;
    async fn update_sync_session(&self, id: Uuid, session: &SyncSession) -> Result<()>;
    async fn add_sync_participant(&self, session_id: Uuid, participant: &Participant)
    -> Result<()>;
    async fn remove_sync_participant(&self, session_id: Uuid, user_id: Uuid) -> Result<()>;
    async fn delete_sync_session(&self, id: Uuid) -> Result<()>;
    async fn end_sync_session(&self, id: Uuid) -> Result<()>;
    async fn cleanup_expired_sync_sessions(&self) -> Result<u32>;
}
