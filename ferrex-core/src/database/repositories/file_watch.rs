use async_trait::async_trait;
use sqlx::{PgPool, Row, postgres::PgRow};
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

fn row_to_event(row: PgRow) -> Result<Option<FileWatchEvent>> {
    let event_type_raw: String = row.try_get("event_type").map_err(|err| {
        MediaError::Internal(format!(
            "Failed to decode file watch event type: {err}"
        ))
    })?;
    let Some(event_type) = str_to_event_type(&event_type_raw) else {
        return Ok(None);
    };

    Ok(Some(FileWatchEvent {
        id: row.try_get("id").map_err(|err| {
            MediaError::Internal(format!(
                "Failed to decode file watch event id: {err}"
            ))
        })?,
        event_version: row.try_get("event_version").map_err(|err| {
            MediaError::Internal(format!(
                "Failed to decode file watch event version: {err}"
            ))
        })?,
        library_id: LibraryId(row.try_get("library_id").map_err(|err| {
            MediaError::Internal(format!(
                "Failed to decode file watch library id: {err}"
            ))
        })?),
        library_root_id: row.try_get("library_root_id").map_err(|err| {
            MediaError::Internal(format!(
                "Failed to decode file watch root id: {err}"
            ))
        })?,
        root_path: row.try_get("root_path").map_err(|err| {
            MediaError::Internal(format!(
                "Failed to decode file watch root path: {err}"
            ))
        })?,
        event_type,
        file_path: row.try_get("file_path").map_err(|err| {
            MediaError::Internal(format!(
                "Failed to decode file watch file path: {err}"
            ))
        })?,
        path_key: row.try_get("path_key").map_err(|err| {
            MediaError::Internal(format!(
                "Failed to decode file watch path key: {err}"
            ))
        })?,
        old_path: row.try_get("old_path").map_err(|err| {
            MediaError::Internal(format!(
                "Failed to decode file watch old path: {err}"
            ))
        })?,
        fingerprint: row.try_get("fingerprint").map_err(|err| {
            MediaError::Internal(format!(
                "Failed to decode file watch fingerprint: {err}"
            ))
        })?,
        file_size: row.try_get("file_size").map_err(|err| {
            MediaError::Internal(format!(
                "Failed to decode file watch file size: {err}"
            ))
        })?,
        file_modified_at: row.try_get("file_modified_at").map_err(|err| {
            MediaError::Internal(format!(
                "Failed to decode file watch modified time: {err}"
            ))
        })?,
        correlation_id: row.try_get("correlation_id").map_err(|err| {
            MediaError::Internal(format!(
                "Failed to decode file watch correlation id: {err}"
            ))
        })?,
        idempotency_key: row.try_get("idempotency_key").map_err(|err| {
            MediaError::Internal(format!(
                "Failed to decode file watch idempotency key: {err}"
            ))
        })?,
        detected_at: row.try_get("detected_at").map_err(|err| {
            MediaError::Internal(format!(
                "Failed to decode file watch detected timestamp: {err}"
            ))
        })?,
        processed: row.try_get("processed").map_err(|err| {
            MediaError::Internal(format!(
                "Failed to decode file watch processed flag: {err}"
            ))
        })?,
        processed_at: row.try_get("processed_at").map_err(|err| {
            MediaError::Internal(format!(
                "Failed to decode file watch processed timestamp: {err}"
            ))
        })?,
        processing_attempts: row.try_get("processing_attempts").map_err(
            |err| {
                MediaError::Internal(format!(
                    "Failed to decode file watch attempts: {err}"
                ))
            },
        )?,
        last_error: row.try_get("last_error").map_err(|err| {
            MediaError::Internal(format!(
                "Failed to decode file watch last error: {err}"
            ))
        })?,
    }))
}

const FILE_WATCH_SELECT: &str = r#"
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
"#;

#[async_trait]
impl FileWatchEventRepository for PostgresFileWatchRepository {
    async fn create_event(&self, event: &FileWatchEvent) -> Result<bool> {
        let result = sqlx::query(
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
        )
        .bind(event.id)
        .bind(event.event_version)
        .bind(event.library_id.as_uuid())
        .bind(event.library_root_id)
        .bind(&event.root_path)
        .bind(event_type_to_str(&event.event_type))
        .bind(&event.file_path)
        .bind(&event.path_key)
        .bind(event.old_path.as_deref())
        .bind(event.fingerprint.as_deref())
        .bind(event.file_size)
        .bind(event.file_modified_at)
        .bind(event.correlation_id)
        .bind(&event.idempotency_key)
        .bind(event.detected_at)
        .bind(event.processed)
        .bind(event.processed_at)
        .bind(event.processing_attempts)
        .bind(event.last_error.as_deref())
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
        let sql = format!(
            "{FILE_WATCH_SELECT} WHERE library_id = $1 AND processed = false ORDER BY detected_at ASC, id ASC LIMIT $2"
        );
        let rows = sqlx::query(&sql)
            .bind(library_id.as_uuid())
            .bind(limit as i64)
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
        sqlx::query(
            r#"
            UPDATE file_watch_events
            SET processed = true, processed_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(event_id)
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
        let result = sqlx::query(
            r#"
            DELETE FROM file_watch_events
            WHERE processed = true
              AND processed_at < NOW() - CAST($1 || ' days' AS INTERVAL)
            "#,
        )
        .bind(days_to_keep.to_string())
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to cleanup old events: {}", e))
        })?;

        Ok(result.rows_affected() as u32)
    }
}
