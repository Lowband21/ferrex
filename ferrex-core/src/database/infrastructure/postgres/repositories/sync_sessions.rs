use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json;
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::database::ports::sync_sessions::SyncSessionsRepository;
use crate::{
    error::{MediaError, Result},
    sync_session::{Participant, PlaybackState, SyncSession},
    types::util_types::MediaType,
};

#[derive(Clone, Debug)]
pub struct PostgresSyncSessionsRepository {
    pool: PgPool,
}

impl PostgresSyncSessionsRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[async_trait]
impl SyncSessionsRepository for PostgresSyncSessionsRepository {
    async fn create_sync_session(&self, session: &SyncSession) -> Result<()> {
        self.create_sync_session_internal(session).await
    }

    async fn get_sync_session_by_code(
        &self,
        room_code: &str,
    ) -> Result<Option<SyncSession>> {
        self.get_sync_session_by_code_internal(room_code).await
    }

    async fn get_sync_session(&self, id: Uuid) -> Result<Option<SyncSession>> {
        self.get_sync_session_internal(id).await
    }

    async fn update_sync_session_state(
        &self,
        id: Uuid,
        state: &PlaybackState,
    ) -> Result<()> {
        self.update_sync_session_state_internal(id, state).await
    }

    async fn update_sync_session(
        &self,
        id: Uuid,
        session: &SyncSession,
    ) -> Result<()> {
        self.update_sync_session_internal(id, session).await
    }

    async fn add_sync_participant(
        &self,
        session_id: Uuid,
        participant: &Participant,
    ) -> Result<()> {
        self.add_sync_participant_internal(session_id, participant)
            .await
    }

    async fn remove_sync_participant(
        &self,
        session_id: Uuid,
        user_id: Uuid,
    ) -> Result<()> {
        self.remove_sync_participant_internal(session_id, user_id)
            .await
    }

    async fn delete_sync_session(&self, id: Uuid) -> Result<()> {
        self.delete_sync_session_internal(id).await
    }

    async fn end_sync_session(&self, id: Uuid) -> Result<()> {
        self.delete_sync_session_internal(id).await
    }

    async fn cleanup_expired_sync_sessions(&self) -> Result<u32> {
        self.cleanup_expired_sync_sessions_internal().await
    }
}

impl PostgresSyncSessionsRepository {
    async fn create_sync_session_internal(
        &self,
        session: &SyncSession,
    ) -> Result<()> {
        let playback_state_json = serde_json::to_value(&session.state)
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to serialize PlaybackState: {}",
                    e
                ))
            })?;

        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!("Failed to start transaction: {}", e))
        })?;

        // Insert sync session
        sqlx::query!(
            r#"
            INSERT INTO sync_sessions (
                id, room_code, host_id, media_uuid, media_type, playback_state,
                created_at, expires_at, is_active
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, true)
            "#,
            session.id,
            session.room_code,
            session.host_id,
            session.media_id,
            session.media_type as i16,
            playback_state_json,
            DateTime::<Utc>::from_timestamp_millis(session.created_at)
                .ok_or_else(|| MediaError::Internal(
                    "Invalid timestamp".to_string()
                ))?,
            DateTime::<Utc>::from_timestamp_millis(session.expires_at)
                .ok_or_else(|| MediaError::Internal(
                    "Invalid timestamp".to_string()
                ))?
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            if let Some(db_err) = e.as_database_error()
                && db_err.constraint() == Some("sync_sessions_room_code_key")
            {
                return MediaError::Conflict(
                    "Room code already in use".to_string(),
                );
            }
            MediaError::Internal(format!(
                "Failed to create sync session: {}",
                e
            ))
        })?;

        // Add host as participant
        sqlx::query!(
            r#"
            INSERT INTO sync_participants (session_id, user_id, joined_at, last_ping, is_ready)
            VALUES ($1, $2, NOW(), NOW(), true)
            "#,
            session.id,
            session.host_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to add host participant: {}", e)))?;

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!("Failed to commit transaction: {}", e))
        })?;

        info!(
            "Created sync session {} with room code {}",
            session.id, session.room_code
        );
        Ok(())
    }

    async fn get_sync_session_by_code_internal(
        &self,
        room_code: &str,
    ) -> Result<Option<SyncSession>> {
        let row = sqlx::query!(
            r#"
            SELECT
                s.id, s.room_code, s.host_id, s.media_uuid, s.media_type, s.playback_state,
                s.created_at, s.expires_at,
                COALESCE(
                    json_agg(
                        json_build_object(
                            'user_id', p.user_id,
                            'display_name', u.display_name,
                            'joined_at', EXTRACT(EPOCH FROM p.joined_at)::BIGINT * 1000,
                            'last_ping', EXTRACT(EPOCH FROM p.last_ping)::BIGINT * 1000,
                            'is_ready', p.is_ready,
                            'latency_ms', p.latency_ms
                        )
                    ) FILTER (WHERE p.user_id IS NOT NULL),
                    '[]'::json
                ) as participants
            FROM sync_sessions s
            LEFT JOIN sync_participants p ON s.id = p.session_id
            LEFT JOIN users u ON p.user_id = u.id
            WHERE s.room_code = $1 AND s.is_active = true AND s.expires_at > NOW()
            GROUP BY s.id
            "#,
            room_code
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get sync session: {}", e)))?;

        if let Some(row) = row {
            let media_uuid: Uuid = row.media_uuid;
            let media_type = MediaType::from(row.media_type);

            let state: PlaybackState =
                serde_json::from_value(row.playback_state).map_err(|e| {
                    MediaError::Internal(format!(
                        "Failed to deserialize PlaybackState: {}",
                        e
                    ))
                })?;

            let participants: Vec<Participant> = serde_json::from_value(
                row.participants.unwrap_or(serde_json::json!([])),
            )
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to deserialize participants: {}",
                    e
                ))
            })?;

            Ok(Some(SyncSession {
                id: row.id,
                room_code: row.room_code,
                host_id: row.host_id,
                media_id: media_uuid,
                media_type,
                state,
                participants,
                created_at: row
                    .created_at
                    .unwrap_or_else(chrono::Utc::now)
                    .timestamp_millis(),
                expires_at: row.expires_at.timestamp_millis(),
            }))
        } else {
            Ok(None)
        }
    }

    async fn get_sync_session_internal(
        &self,
        id: Uuid,
    ) -> Result<Option<SyncSession>> {
        let row = sqlx::query!(
            r#"
            SELECT
                s.id, s.room_code, s.host_id, s.media_uuid, s.media_type, s.playback_state,
                s.created_at, s.expires_at,
                COALESCE(
                    json_agg(
                        json_build_object(
                            'user_id', p.user_id,
                            'display_name', u.display_name,
                            'joined_at', EXTRACT(EPOCH FROM p.joined_at)::BIGINT * 1000,
                            'last_ping', EXTRACT(EPOCH FROM p.last_ping)::BIGINT * 1000,
                            'is_ready', p.is_ready,
                            'latency_ms', p.latency_ms
                        )
                    ) FILTER (WHERE p.user_id IS NOT NULL),
                    '[]'::json
                ) as participants
            FROM sync_sessions s
            LEFT JOIN sync_participants p ON s.id = p.session_id
            LEFT JOIN users u ON p.user_id = u.id
            WHERE s.id = $1 AND s.is_active = true
            GROUP BY s.id
            "#,
            id
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get sync session: {}", e)))?;

        if let Some(row) = row {
            let media_uuid: Uuid = row.media_uuid;
            let media_type = MediaType::from(row.media_type);

            let state: PlaybackState =
                serde_json::from_value(row.playback_state).map_err(|e| {
                    MediaError::Internal(format!(
                        "Failed to deserialize PlaybackState: {}",
                        e
                    ))
                })?;

            let participants: Vec<Participant> = serde_json::from_value(
                row.participants.unwrap_or(serde_json::json!([])),
            )
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to deserialize participants: {}",
                    e
                ))
            })?;

            Ok(Some(SyncSession {
                id: row.id,
                room_code: row.room_code,
                host_id: row.host_id,
                media_id: media_uuid,
                media_type,
                state,
                participants,
                created_at: row
                    .created_at
                    .unwrap_or_else(chrono::Utc::now)
                    .timestamp_millis(),
                expires_at: row.expires_at.timestamp_millis(),
            }))
        } else {
            Ok(None)
        }
    }

    async fn update_sync_session_state_internal(
        &self,
        id: Uuid,
        state: &PlaybackState,
    ) -> Result<()> {
        let playback_state_json = serde_json::to_value(state).map_err(|e| {
            MediaError::Internal(format!(
                "Failed to serialize PlaybackState: {}",
                e
            ))
        })?;

        let result = sqlx::query!(
            r#"
            UPDATE sync_sessions
            SET playback_state = $2
            WHERE id = $1 AND is_active = true
            "#,
            id,
            playback_state_json
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to update sync session state: {}",
                e
            ))
        })?;

        if result.rows_affected() == 0 {
            return Err(MediaError::NotFound(
                "Sync session not found or inactive".to_string(),
            ));
        }

        Ok(())
    }

    async fn update_sync_session_internal(
        &self,
        id: Uuid,
        session: &SyncSession,
    ) -> Result<()> {
        // Serialize the playback state
        let playback_state_json = serde_json::to_value(&session.state)
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to serialize PlaybackState: {}",
                    e
                ))
            })?;

        // Update the session
        let result = sqlx::query!(
            r#"
            UPDATE sync_sessions
            SET room_code = $2,
                host_id = $3,
                media_uuid = $4,
                media_type = $5,
                playback_state = $6
            WHERE id = $1 AND is_active = true
            "#,
            id,
            session.room_code,
            session.host_id,
            session.media_id,
            session.media_type as i16,
            playback_state_json
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to update sync session: {}",
                e
            ))
        })?;

        if result.rows_affected() == 0 {
            return Err(MediaError::NotFound(
                "Sync session not found or inactive".to_string(),
            ));
        }

        // Update participants - first remove existing ones
        sqlx::query!("DELETE FROM sync_participants WHERE session_id = $1", id)
            .execute(self.pool())
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to remove participants: {}",
                    e
                ))
            })?;

        // Then add new participants
        for participant in &session.participants {
            self.add_sync_participant_internal(id, participant).await?;
        }

        Ok(())
    }

    async fn add_sync_participant_internal(
        &self,
        session_id: Uuid,
        participant: &Participant,
    ) -> Result<()> {
        let now = DateTime::<Utc>::from_timestamp_millis(
            chrono::Utc::now().timestamp_millis(),
        )
        .ok_or_else(|| MediaError::Internal("Invalid timestamp".to_string()))?;
        let last_ping =
            DateTime::<Utc>::from_timestamp_millis(participant.last_ping)
                .ok_or_else(|| {
                    MediaError::Internal("Invalid timestamp".to_string())
                })?;

        sqlx::query!(
            r#"
            INSERT INTO sync_participants (session_id, user_id, joined_at, last_ping, is_ready, latency_ms)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (session_id, user_id) DO UPDATE SET
                last_ping = EXCLUDED.last_ping,
                is_ready = EXCLUDED.is_ready,
                latency_ms = EXCLUDED.latency_ms
            "#,
            session_id,
            participant.user_id,
            now, // Use current time for joined_at
            last_ping,
            participant.is_ready,
            participant.latency_ms as i32
        )
        .execute(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to add participant: {}", e)))?;

        info!(
            "Added participant {} to sync session {}",
            participant.user_id, session_id
        );
        Ok(())
    }

    async fn remove_sync_participant_internal(
        &self,
        session_id: Uuid,
        user_id: Uuid,
    ) -> Result<()> {
        let result = sqlx::query!(
            r#"
            DELETE FROM sync_participants
            WHERE session_id = $1 AND user_id = $2
            "#,
            session_id,
            user_id
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to remove participant: {}", e))
        })?;

        if result.rows_affected() > 0 {
            info!(
                "Removed participant {} from sync session {}",
                user_id, session_id
            );
        }

        Ok(())
    }

    async fn delete_sync_session_internal(&self, id: Uuid) -> Result<()> {
        let result = sqlx::query!(
            r#"
            UPDATE sync_sessions
            SET is_active = false
            WHERE id = $1
            "#,
            id
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to delete sync session: {}",
                e
            ))
        })?;

        if result.rows_affected() > 0 {
            info!("Deactivated sync session {}", id);
        }

        Ok(())
    }

    async fn cleanup_expired_sync_sessions_internal(&self) -> Result<u32> {
        let result = sqlx::query!(
            r#"
            UPDATE sync_sessions
            SET is_active = false
            WHERE expires_at < NOW() AND is_active = true
            "#
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to cleanup expired sessions: {}",
                e
            ))
        })?;

        let count = result.rows_affected() as u32;
        if count > 0 {
            info!("Cleaned up {} expired sync sessions", count);
        }

        Ok(count)
    }
}
