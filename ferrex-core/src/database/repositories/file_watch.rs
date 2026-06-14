use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::database::repository_ports::file_watch::FileWatchEventRepository;
use crate::database::traits::{FileWatchEvent, FileWatchEventType};
use crate::error::{MediaError, Result};
use crate::types::ids::LibraryId;

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

fn event_type_to_str(kind: &FileWatchEventType) -> &'static str {
    match kind {
        FileWatchEventType::Created => "created",
        FileWatchEventType::Modified => "modified",
        FileWatchEventType::Deleted => "deleted",
        FileWatchEventType::Moved => "moved",
        FileWatchEventType::Overflow => "overflow",
    }
}

fn str_to_event_type(raw: &str) -> Option<FileWatchEventType> {
    match raw {
        "created" => Some(FileWatchEventType::Created),
        "modified" => Some(FileWatchEventType::Modified),
        "deleted" => Some(FileWatchEventType::Deleted),
        "moved" => Some(FileWatchEventType::Moved),
        "overflow" => Some(FileWatchEventType::Overflow),
        _ => None,
    }
}

#[derive(Debug)]
struct FileWatchEventRow {
    id: Uuid,
    event_version: i32,
    library_id: Uuid,
    library_root_id: i32,
    root_path: String,
    event_type: String,
    file_path: String,
    path_key: String,
    old_path: Option<String>,
    fingerprint: Option<String>,
    file_size: Option<i64>,
    file_modified_at: Option<DateTime<Utc>>,
    correlation_id: Option<Uuid>,
    idempotency_key: String,
    detected_at: DateTime<Utc>,
    processed: bool,
    processed_at: Option<DateTime<Utc>>,
    processing_attempts: i32,
    last_error: Option<String>,
}

fn row_to_event(row: FileWatchEventRow) -> Result<Option<FileWatchEvent>> {
    let Some(event_type) = str_to_event_type(&row.event_type) else {
        return Ok(None);
    };

    Ok(Some(FileWatchEvent {
        id: row.id,
        event_version: row.event_version,
        library_id: LibraryId(row.library_id),
        library_root_id: row.library_root_id,
        root_path: row.root_path,
        event_type,
        file_path: row.file_path,
        path_key: row.path_key,
        old_path: row.old_path,
        fingerprint: row.fingerprint,
        file_size: row.file_size,
        file_modified_at: row.file_modified_at,
        correlation_id: row.correlation_id,
        idempotency_key: row.idempotency_key,
        detected_at: row.detected_at,
        processed: row.processed,
        processed_at: row.processed_at,
        processing_attempts: row.processing_attempts,
        last_error: row.last_error,
    }))
}

#[async_trait]
impl FileWatchEventRepository for PostgresFileWatchRepository {
    async fn create_event(&self, event: &FileWatchEvent) -> Result<bool> {
        let result = sqlx::query!(
            r#"
            INSERT INTO file_watch_events (
                id,
                event_version,
                library_id,
                library_root_id,
                root_path,
                event_type,
                file_path,
                path_key,
                old_path,
                fingerprint,
                file_size,
                file_modified_at,
                correlation_id,
                idempotency_key,
                detected_at,
                processed,
                processed_at,
                processing_attempts,
                last_error
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
            ON CONFLICT (idempotency_key) DO NOTHING
            "#,
            event.id,
            event.event_version,
            event.library_id.as_uuid(),
            event.library_root_id,
            &event.root_path,
            event_type_to_str(&event.event_type),
            &event.file_path,
            &event.path_key,
            event.old_path.as_deref(),
            event.fingerprint.as_deref(),
            event.file_size,
            event.file_modified_at,
            event.correlation_id,
            &event.idempotency_key,
            event.detected_at,
            event.processed,
            event.processed_at,
            event.processing_attempts,
            event.last_error.as_deref()
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to create file watch event: {}",
                e
            ))
        })?;

        Ok(result.rows_affected() == 1)
    }

    async fn get_unprocessed_events(
        &self,
        library_id: LibraryId,
        limit: i32,
    ) -> Result<Vec<FileWatchEvent>> {
        let rows = sqlx::query_as!(
            FileWatchEventRow,
            r#"
            SELECT
                id,
                event_version,
                library_id,
                library_root_id,
                root_path,
                event_type,
                file_path,
                path_key,
                old_path,
                fingerprint,
                file_size,
                file_modified_at,
                correlation_id,
                idempotency_key,
                detected_at,
                processed,
                processed_at,
                processing_attempts,
                last_error
            FROM file_watch_events
            WHERE library_id = $1 AND processed = false
            ORDER BY detected_at ASC, id ASC
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
            if let Some(event) = row_to_event(row)? {
                events.push(event);
            }
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
              AND processed_at < NOW() - ($1::integer * INTERVAL '1 day')
            "#,
            days_to_keep
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to cleanup old events: {}", e))
        })?;

        Ok(result.rows_affected() as u32)
    }
}
