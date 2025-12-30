use crate::database::PostgresDatabase;
use crate::database::repository_ports::sync_sessions::SyncSessionsRepository;
use crate::error::Result;
use crate::sync_session::{Participant, PlaybackState, SyncSession};
use uuid::Uuid;

impl PostgresDatabase {
    pub async fn create_sync_session(
        &self,
        session: &SyncSession,
    ) -> Result<()> {
        self.sync_sessions_repository()
            .create_sync_session(session)
            .await
    }

    pub async fn get_sync_session_by_code(
        &self,
        room_code: &str,
    ) -> Result<Option<SyncSession>> {
        self.sync_sessions_repository()
            .get_sync_session_by_code(room_code)
            .await
    }

    pub async fn get_sync_session(
        &self,
        id: Uuid,
    ) -> Result<Option<SyncSession>> {
        self.sync_sessions_repository().get_sync_session(id).await
    }

    pub async fn update_sync_session_state(
        &self,
        id: Uuid,
        state: &PlaybackState,
    ) -> Result<()> {
        self.sync_sessions_repository()
            .update_sync_session_state(id, state)
            .await
    }

    pub async fn update_sync_session(
        &self,
        id: Uuid,
        session: &SyncSession,
    ) -> Result<()> {
        self.sync_sessions_repository()
            .update_sync_session(id, session)
            .await
    }

    pub async fn add_sync_participant(
        &self,
        session_id: Uuid,
        participant: &Participant,
    ) -> Result<()> {
        self.sync_sessions_repository()
            .add_sync_participant(session_id, participant)
            .await
    }

    pub async fn remove_sync_participant(
        &self,
        session_id: Uuid,
        user_id: Uuid,
    ) -> Result<()> {
        self.sync_sessions_repository()
            .remove_sync_participant(session_id, user_id)
            .await
    }

    pub async fn delete_sync_session(&self, id: Uuid) -> Result<()> {
        self.sync_sessions_repository()
            .delete_sync_session(id)
            .await
    }

    pub async fn end_sync_session(&self, id: Uuid) -> Result<()> {
        self.sync_sessions_repository().end_sync_session(id).await
    }

    pub async fn cleanup_expired_sync_sessions(&self) -> Result<u32> {
        self.sync_sessions_repository()
            .cleanup_expired_sync_sessions()
            .await
    }
}
