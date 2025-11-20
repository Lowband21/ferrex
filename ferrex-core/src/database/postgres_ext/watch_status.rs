use crate::database::PostgresDatabase;
use crate::{
    api_types::MediaId, InProgressItem, MediaError, Result, UpdateProgressRequest, UserWatchState,
};
use serde_json;
use std::collections::{HashMap, HashSet};
use tracing::info;
use uuid::Uuid;

/// Watch status tracking extensions for PostgresDatabase
impl PostgresDatabase {
    pub async fn update_watch_progress(
        &self,
        user_id: Uuid,
        progress: &UpdateProgressRequest,
    ) -> Result<()> {
        let media_id_json = serde_json::to_value(&progress.media_id)
            .map_err(|e| MediaError::Internal(format!("Failed to serialize MediaId: {}", e)))?;

        let now = chrono::Utc::now().timestamp_millis();

        let mut tx = self
            .pool()
            .begin()
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to start transaction: {}", e)))?;

        // Update or insert watch progress
        sqlx::query!(
            r#"
            INSERT INTO user_watch_progress (
                user_id, media_id_json, position, duration, last_watched, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $5)
            ON CONFLICT (user_id, media_id_json) DO UPDATE SET
                position = EXCLUDED.position,
                duration = EXCLUDED.duration,
                last_watched = EXCLUDED.last_watched,
                updated_at = EXCLUDED.updated_at
            "#,
            user_id,
            media_id_json,
            progress.position,
            progress.duration,
            now
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to update watch progress: {}", e)))?;

        // Check if we should mark as completed (>95% watched)
        let completion_ratio = progress.position / progress.duration;
        if completion_ratio > 0.95 {
            info!(
                "Media {} is {}% complete, marking as completed",
                serde_json::to_string(&progress.media_id).unwrap_or_default(),
                (completion_ratio * 100.0) as i32
            );

            sqlx::query!(
                r#"
                INSERT INTO user_completed_media (user_id, media_id_json, completed_at)
                VALUES ($1, $2, $3)
                ON CONFLICT (user_id, media_id_json) DO NOTHING
                "#,
                user_id,
                media_id_json,
                now
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to mark as completed: {}", e)))?;

            // Remove from in-progress
            sqlx::query!(
                r#"
                DELETE FROM user_watch_progress
                WHERE user_id = $1 AND media_id_json = $2
                "#,
                user_id,
                media_id_json
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Failed to remove from in-progress: {}", e))
            })?;
        }

        tx.commit()
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to commit transaction: {}", e)))?;

        Ok(())
    }

    pub async fn get_user_watch_state(&self, user_id: Uuid) -> Result<UserWatchState> {
        // Get in-progress items
        let progress_rows = sqlx::query!(
            r#"
            SELECT media_id_json, position, duration, last_watched
            FROM user_watch_progress
            WHERE user_id = $1
            ORDER BY last_watched DESC
            "#,
            user_id
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get watch progress: {}", e)))?;

        let mut in_progress = HashMap::new();
        for row in progress_rows {
            let media_id: MediaId = serde_json::from_value(row.media_id_json).map_err(|e| {
                MediaError::Internal(format!("Failed to deserialize MediaId: {}", e))
            })?;

            in_progress.insert(
                media_id,
                InProgressItem {
                    media_id,
                    position: row.position,
                    duration: row.duration,
                    last_watched: row.last_watched,
                },
            );
        }

        // Get completed items
        let completed_rows = sqlx::query!(
            r#"
            SELECT media_id_json
            FROM user_completed_media
            WHERE user_id = $1
            "#,
            user_id
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get completed media: {}", e)))?;

        let mut completed = HashSet::new();
        for row in completed_rows {
            let media_id: MediaId = serde_json::from_value(row.media_id_json).map_err(|e| {
                MediaError::Internal(format!("Failed to deserialize MediaId: {}", e))
            })?;
            completed.insert(media_id);
        }

        info!(
            "User {} has {} in-progress and {} completed items",
            user_id,
            in_progress.len(),
            completed.len()
        );

        Ok(UserWatchState {
            in_progress,
            completed,
        })
    }

    pub async fn get_continue_watching(
        &self,
        user_id: Uuid,
        limit: usize,
    ) -> Result<Vec<InProgressItem>> {
        let rows = sqlx::query!(
            r#"
            SELECT media_id_json, position, duration, last_watched
            FROM user_watch_progress
            WHERE user_id = $1
            ORDER BY last_watched DESC
            LIMIT $2
            "#,
            user_id,
            limit as i64
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get continue watching: {}", e)))?;

        let mut items = Vec::new();
        for row in rows {
            let media_id: MediaId = serde_json::from_value(row.media_id_json).map_err(|e| {
                MediaError::Internal(format!("Failed to deserialize MediaId: {}", e))
            })?;

            items.push(InProgressItem {
                media_id,
                position: row.position,
                duration: row.duration,
                last_watched: row.last_watched,
            });
        }

        Ok(items)
    }

    pub async fn clear_watch_progress(&self, user_id: Uuid, media_id: &MediaId) -> Result<()> {
        let media_id_json = serde_json::to_value(media_id)
            .map_err(|e| MediaError::Internal(format!("Failed to serialize MediaId: {}", e)))?;

        let mut tx = self
            .pool()
            .begin()
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to start transaction: {}", e)))?;

        // Remove from progress
        let progress_result = sqlx::query!(
            r#"
            DELETE FROM user_watch_progress
            WHERE user_id = $1 AND media_id_json = $2
            "#,
            user_id,
            media_id_json
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to clear watch progress: {}", e)))?;

        // Remove from completed
        let completed_result = sqlx::query!(
            r#"
            DELETE FROM user_completed_media
            WHERE user_id = $1 AND media_id_json = $2
            "#,
            user_id,
            media_id_json
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to clear completed status: {}", e)))?;

        tx.commit()
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to commit transaction: {}", e)))?;

        info!(
            "Cleared watch progress for user {} media {}: {} progress, {} completed removed",
            user_id,
            serde_json::to_string(media_id).unwrap_or_default(),
            progress_result.rows_affected(),
            completed_result.rows_affected()
        );

        Ok(())
    }

    pub async fn is_media_completed(&self, user_id: Uuid, media_id: &MediaId) -> Result<bool> {
        let media_id_json = serde_json::to_value(media_id)
            .map_err(|e| MediaError::Internal(format!("Failed to serialize MediaId: {}", e)))?;

        let exists = sqlx::query!(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM user_completed_media
                WHERE user_id = $1 AND media_id_json = $2
            ) as "exists!"
            "#,
            user_id,
            media_id_json
        )
        .fetch_one(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to check completion status: {}", e)))?;

        Ok(exists.exists)
    }
}
