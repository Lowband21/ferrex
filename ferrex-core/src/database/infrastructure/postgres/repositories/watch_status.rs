use async_trait::async_trait;
use chrono::Utc;
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use tracing::info;
use uuid::Uuid;

use crate::database::ports::watch_status::WatchStatusRepository;
use crate::{
    error::{MediaError, Result},
    watch_status::{InProgressItem, UpdateProgressRequest, UserWatchState},
};

#[derive(Clone, Debug)]
pub struct PostgresWatchStatusRepository {
    pool: PgPool,
}

impl PostgresWatchStatusRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[async_trait]
impl WatchStatusRepository for PostgresWatchStatusRepository {
    async fn update_watch_progress(
        &self,
        user_id: Uuid,
        progress: &UpdateProgressRequest,
    ) -> Result<()> {
        let now = Utc::now().timestamp_millis();

        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!("Failed to start transaction: {}", e))
        })?;

        // Update or insert watch progress
        sqlx::query!(
            r#"
            INSERT INTO user_watch_progress (
                user_id, media_uuid, media_type, position, duration, last_watched, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $6)
            ON CONFLICT (user_id, media_uuid) DO UPDATE SET
                media_type = EXCLUDED.media_type,
                position = EXCLUDED.position,
                duration = EXCLUDED.duration,
                last_watched = EXCLUDED.last_watched,
                updated_at = EXCLUDED.updated_at
            "#,
            user_id,
            progress.media_id,
            progress.media_type as i16,
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
                "Media {} ({}) is {}% complete, marking as completed",
                progress.media_id,
                progress.media_type,
                (completion_ratio * 100.0) as i32
            );

            sqlx::query!(
                r#"
                INSERT INTO user_completed_media (user_id, media_uuid, media_type, completed_at)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (user_id, media_uuid) DO NOTHING
                "#,
                user_id,
                progress.media_id,
                progress.media_type as i16,
                now
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to mark as completed: {}", e)))?;

            // Remove from in-progress
            sqlx::query!(
                r#"
                DELETE FROM user_watch_progress
                WHERE user_id = $1 AND media_uuid = $2
                "#,
                user_id,
                progress.media_id
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to remove from in-progress: {}",
                    e
                ))
            })?;
        }

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!("Failed to commit transaction: {}", e))
        })?;

        Ok(())
    }

    async fn get_user_watch_state(
        &self,
        user_id: Uuid,
    ) -> Result<UserWatchState> {
        // Get in-progress items
        let progress_rows = sqlx::query!(
            r#"
            SELECT media_uuid, position, duration, last_watched
            FROM user_watch_progress
            WHERE user_id = $1
            ORDER BY last_watched DESC
            "#,
            user_id
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to get watch progress: {}", e))
        })?;

        let mut in_progress = HashMap::new();
        for row in progress_rows {
            in_progress.insert(
                row.media_uuid,
                InProgressItem {
                    media_id: row.media_uuid,
                    position: row.position,
                    duration: row.duration,
                    last_watched: row.last_watched,
                },
            );
        }

        // Get completed items
        let completed_rows = sqlx::query!(
            r#"
            SELECT media_uuid
            FROM user_completed_media
            WHERE user_id = $1
            "#,
            user_id
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to get completed media: {}",
                e
            ))
        })?;

        let mut completed = HashSet::new();
        for row in completed_rows {
            completed.insert(row.media_uuid);
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

    async fn get_continue_watching(
        &self,
        user_id: Uuid,
        limit: usize,
    ) -> Result<Vec<InProgressItem>> {
        let rows = sqlx::query!(
            r#"
            SELECT media_uuid, position, duration, last_watched
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
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to get continue watching: {}",
                e
            ))
        })?;

        let mut items = Vec::new();
        for row in rows {
            items.push(InProgressItem {
                media_id: row.media_uuid,
                position: row.position,
                duration: row.duration,
                last_watched: row.last_watched,
            });
        }

        Ok(items)
    }

    async fn clear_watch_progress(
        &self,
        user_id: Uuid,
        media_id: &Uuid,
    ) -> Result<()> {
        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!("Failed to start transaction: {}", e))
        })?;

        // Remove from progress
        let progress_result = sqlx::query!(
            r#"
            DELETE FROM user_watch_progress
            WHERE user_id = $1 AND media_uuid = $2
            "#,
            user_id,
            media_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to clear watch progress: {}",
                e
            ))
        })?;

        // Remove from completed
        let completed_result = sqlx::query!(
            r#"
            DELETE FROM user_completed_media
            WHERE user_id = $1 AND media_uuid = $2
            "#,
            user_id,
            media_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to clear completed status: {}",
                e
            ))
        })?;

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!("Failed to commit transaction: {}", e))
        })?;

        info!(
            "Cleared watch progress for user {} media {}: {} progress, {} completed removed",
            user_id,
            media_id,
            progress_result.rows_affected(),
            completed_result.rows_affected()
        );

        Ok(())
    }

    async fn is_media_completed(
        &self,
        user_id: Uuid,
        media_id: &Uuid,
    ) -> Result<bool> {
        let exists = sqlx::query!(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM user_completed_media
                WHERE user_id = $1 AND media_uuid = $2
            ) as "exists!"
            "#,
            user_id,
            media_id
        )
        .fetch_one(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to check completion status: {}",
                e
            ))
        })?;

        Ok(exists.exists)
    }
}
