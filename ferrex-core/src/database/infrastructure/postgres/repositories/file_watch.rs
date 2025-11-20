use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use crate::database::ports::file_watch::FileWatchEventRepository;
use crate::database::traits::{FileWatchEvent, FileWatchEventType};
use crate::error::{MediaError, Result};
use crate::types::ids::LibraryID;

#[derive(Clone, Debug)]
pub struct PostgresFileWatchRepository {
    pool: PgPool,
}

impl PostgresFileWatchRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[async_trait]
impl FileWatchEventRepository for PostgresFileWatchRepository {
    async fn create_event(&self, event: &FileWatchEvent) -> Result<()> {
        let event_type_str = format!("{:?}", event.event_type).to_lowercase();

        sqlx::query!(
            r#"
            INSERT INTO file_watch_events (
                id, library_id, event_type, file_path, old_path, file_size,
                detected_at, processed, processed_at, processing_attempts, last_error
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
            event.id,
            event.library_id.as_uuid(),
            event_type_str,
            event.file_path,
            event.old_path,
            event.file_size,
            event.detected_at,
            event.processed,
            event.processed_at,
            event.processing_attempts,
            event.last_error
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to create file watch event: {}",
                e
            ))
        })?;

        Ok(())
    }

    async fn get_unprocessed_events(
        &self,
        library_id: LibraryID,
        limit: i32,
    ) -> Result<Vec<FileWatchEvent>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, library_id, event_type, file_path, old_path, file_size,
                   detected_at, processed, processed_at, processing_attempts, last_error
            FROM file_watch_events
            WHERE library_id = $1 AND processed = false
            ORDER BY detected_at ASC
            LIMIT $2
            "#,
            library_id.as_uuid(),
            limit as i64
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to get unprocessed events: {}",
                e
            ))
        })?;

        let mut events = Vec::with_capacity(rows.len());
        for row in rows {
            let event_type = match row.event_type.as_str() {
                "created" => FileWatchEventType::Created,
                "modified" => FileWatchEventType::Modified,
                "deleted" => FileWatchEventType::Deleted,
                "moved" => FileWatchEventType::Moved,
                _ => continue,
            };

            events.push(FileWatchEvent {
                id: row.id,
                library_id: LibraryID(row.library_id),
                event_type,
                file_path: row.file_path,
                old_path: row.old_path,
                file_size: row.file_size,
                detected_at: row.detected_at,
                processed: row.processed,
                processed_at: row.processed_at,
                processing_attempts: row.processing_attempts,
                last_error: row.last_error,
            });
        }

        Ok(events)
    }

    async fn mark_processed(&self, event_id: Uuid) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE file_watch_events
            SET processed = true, processed_at = NOW()
            WHERE id = $1
            "#,
            event_id
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to mark event processed: {}",
                e
            ))
        })?;

        Ok(())
    }

    async fn cleanup_processed(&self, days_to_keep: i32) -> Result<u32> {
        let result = sqlx::query!(
            r#"
            DELETE FROM file_watch_events
            WHERE processed = true
              AND processed_at < NOW() - CAST($1 || ' days' AS INTERVAL)
            "#,
            days_to_keep.to_string()
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to cleanup old events: {}", e))
        })?;

        Ok(result.rows_affected() as u32)
    }
}
