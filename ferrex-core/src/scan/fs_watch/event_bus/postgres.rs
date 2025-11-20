use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, error, trace, warn};
use uuid::Uuid;

use super::{FileChangeCursor, FileChangeEventBus, FileChangeEventStream};
use crate::database::postgres::PostgresDatabase;
use crate::database::traits::{FileWatchEvent, FileWatchEventType};
use crate::error::{MediaError, Result};
use crate::types::ids::LibraryID;

const DEFAULT_FETCH_LIMIT: i64 = 256;
const DEFAULT_CHANNEL_CAPACITY: usize = 512;
const DEFAULT_POLL_INTERVAL_MS: u64 = 500;

#[derive(Clone, Debug)]
pub struct PostgresFileChangeEventBusConfig {
    pub fetch_limit: i64,
    pub channel_capacity: usize,
    pub poll_interval: Duration,
}

impl Default for PostgresFileChangeEventBusConfig {
    fn default() -> Self {
        Self {
            fetch_limit: DEFAULT_FETCH_LIMIT,
            channel_capacity: DEFAULT_CHANNEL_CAPACITY,
            poll_interval: Duration::from_millis(DEFAULT_POLL_INTERVAL_MS),
        }
    }
}

#[derive(Clone)]
pub struct PostgresFileChangeEventBus {
    pool: PgPool,
    config: PostgresFileChangeEventBusConfig,
}

impl PostgresFileChangeEventBus {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            config: PostgresFileChangeEventBusConfig::default(),
        }
    }

    pub fn with_config(pool: PgPool, config: PostgresFileChangeEventBusConfig) -> Self {
        Self { pool, config }
    }

    pub fn from_postgres(db: &PostgresDatabase) -> Self {
        Self::new(db.pool().clone())
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }
}

impl fmt::Debug for PostgresFileChangeEventBus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresFileChangeEventBus")
            .field("pool_size", &self.pool.size())
            .field("idle_connections", &self.pool.num_idle())
            .field("config", &self.config)
            .finish()
    }
}

fn event_type_to_str(kind: &FileWatchEventType) -> &'static str {
    match kind {
        FileWatchEventType::Created => "created",
        FileWatchEventType::Modified => "modified",
        FileWatchEventType::Deleted => "deleted",
        FileWatchEventType::Moved => "moved",
    }
}

fn str_to_event_type(raw: &str) -> Option<FileWatchEventType> {
    match raw {
        "created" => Some(FileWatchEventType::Created),
        "modified" => Some(FileWatchEventType::Modified),
        "deleted" => Some(FileWatchEventType::Deleted),
        "moved" => Some(FileWatchEventType::Moved),
        _ => None,
    }
}

#[derive(Debug, FromRow)]
struct FileWatchEventRow {
    id: Uuid,
    library_id: Uuid,
    event_type: String,
    file_path: String,
    old_path: Option<String>,
    file_size: Option<i64>,
    detected_at: DateTime<Utc>,
    processed: bool,
    processed_at: Option<DateTime<Utc>>,
    processing_attempts: i32,
    last_error: Option<String>,
}

impl FileWatchEventRow {
    fn into_event(self) -> Option<FileWatchEvent> {
        let event_type = str_to_event_type(&self.event_type)?;
        Some(FileWatchEvent {
            id: self.id,
            library_id: LibraryID(self.library_id),
            event_type,
            file_path: self.file_path,
            old_path: self.old_path,
            file_size: self.file_size,
            detected_at: self.detected_at,
            processed: self.processed,
            processed_at: self.processed_at,
            processing_attempts: self.processing_attempts,
            last_error: self.last_error,
        })
    }
}

async fn fetch_events_after(
    pool: &PgPool,
    library_id: LibraryID,
    last_detected_at: Option<DateTime<Utc>>,
    last_event_id: Option<Uuid>,
    limit: i64,
) -> Result<Vec<FileWatchEvent>> {
    let rows = sqlx::query_as::<_, FileWatchEventRow>(
        r#"
        SELECT
            id,
            library_id,
            event_type,
            file_path,
            old_path,
            file_size,
            detected_at,
            processed,
            processed_at,
            processing_attempts,
            last_error
        FROM file_watch_events
        WHERE library_id = $1
          AND (
                $2::timestamptz IS NULL
                OR detected_at > $2
                OR (detected_at = $2 AND id > $3)
          )
        ORDER BY detected_at ASC, id ASC
        LIMIT $4
        "#,
    )
    .bind(library_id.as_uuid())
    .bind(last_detected_at)
    .bind(last_event_id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|err| MediaError::Internal(format!("failed to fetch file watch events: {err}")))?;

    let mut events = Vec::with_capacity(rows.len());
    for row in rows {
        if let Some(event) = row.into_event() {
            events.push(event);
        } else {
            warn!("skipping file watch event with unknown type");
        }
    }

    Ok(events)
}

async fn fetch_event(pool: &PgPool, event_id: Uuid) -> Result<Option<(LibraryID, DateTime<Utc>)>> {
    let result = sqlx::query_as::<_, (Uuid, DateTime<Utc>)>(
        "SELECT library_id, detected_at FROM file_watch_events WHERE id = $1",
    )
    .bind(event_id)
    .fetch_optional(pool)
    .await
    .map_err(|err| MediaError::Internal(format!("failed to load file watch event by id: {err}")))?;

    Ok(result.map(|(library, detected_at)| (LibraryID(library), detected_at)))
}

async fn upsert_cursor(pool: &PgPool, cursor: &FileChangeCursor) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO file_watch_consumer_offsets (
            group_name,
            library_id,
            last_event_id,
            last_detected_at,
            updated_at
        ) VALUES ($1, $2, $3, $4, NOW())
        ON CONFLICT (group_name, library_id)
        DO UPDATE SET
            last_event_id = EXCLUDED.last_event_id,
            last_detected_at = EXCLUDED.last_detected_at,
            updated_at = NOW()
        "#,
    )
    .bind(&cursor.group)
    .bind(cursor.library_id.as_uuid())
    .bind(cursor.last_event_id)
    .bind(cursor.last_detected_at)
    .execute(pool)
    .await
    .map_err(|err| MediaError::Internal(format!("failed to upsert file watch cursor: {err}")))?;

    Ok(())
}

async fn load_cursor(
    pool: &PgPool,
    group: &str,
    library_id: LibraryID,
) -> Result<Option<FileChangeCursor>> {
    let row = sqlx::query_as::<_, (String, Uuid, Option<Uuid>, Option<DateTime<Utc>>)>(
        r#"
        SELECT group_name, library_id, last_event_id, last_detected_at
        FROM file_watch_consumer_offsets
        WHERE group_name = $1 AND library_id = $2
        "#,
    )
    .bind(group)
    .bind(library_id.as_uuid())
    .fetch_optional(pool)
    .await
    .map_err(|err| MediaError::Internal(format!("failed to load file watch cursor: {err}")))?;

    Ok(row.map(
        |(stored_group, stored_library, last_event_id, last_detected_at)| FileChangeCursor {
            group: stored_group,
            library_id: LibraryID(stored_library),
            last_event_id,
            last_detected_at,
        },
    ))
}

async fn set_processed(pool: &PgPool, event_id: Uuid) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE file_watch_events
        SET processed = true,
            processed_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(event_id)
    .execute(pool)
    .await
    .map_err(|err| {
        MediaError::Internal(format!("failed to mark file watch event processed: {err}"))
    })?;

    Ok(())
}

async fn cleanup_old_events(pool: &PgPool, days_to_keep: i32) -> Result<u32> {
    let affected = sqlx::query(
        r#"
        DELETE FROM file_watch_events
        WHERE detected_at < NOW() - ($1 || ' days')::interval
        "#,
    )
    .bind(days_to_keep.to_string())
    .execute(pool)
    .await
    .map_err(|err| MediaError::Internal(format!("failed to clean up file watch events: {err}")))?
    .rows_affected();

    Ok(affected as u32)
}

#[async_trait]
impl FileChangeEventBus for PostgresFileChangeEventBus {
    async fn publish(&self, event: FileWatchEvent) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO file_watch_events (
                id,
                library_id,
                event_type,
                file_path,
                old_path,
                file_size,
                detected_at,
                processed,
                processed_at,
                processing_attempts,
                last_error
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(event.id)
        .bind(event.library_id.as_uuid())
        .bind(event_type_to_str(&event.event_type))
        .bind(&event.file_path)
        .bind(&event.old_path)
        .bind(event.file_size)
        .bind(event.detected_at)
        .bind(event.processed)
        .bind(event.processed_at)
        .bind(event.processing_attempts)
        .bind(&event.last_error)
        .execute(self.pool())
        .await
        .map_err(|err| {
            MediaError::Internal(format!("failed to persist file watch event: {err}"))
        })?;

        Ok(())
    }

    async fn subscribe(&self, group: &str, library_id: LibraryID) -> Result<FileChangeEventStream> {
        let cursor = load_cursor(self.pool(), group, library_id).await?;
        let initial_detected_at = cursor.as_ref().and_then(|cursor| cursor.last_detected_at);
        let initial_event_id = cursor.as_ref().and_then(|cursor| cursor.last_event_id);

        let (tx, rx) = mpsc::channel(self.config.channel_capacity);
        let pool = self.pool.clone();
        let group = group.to_owned();
        let poll_interval = self.config.poll_interval;
        let fetch_limit = self.config.fetch_limit;

        tokio::spawn(async move {
            let mut sender = tx;
            let mut last_detected_at = initial_detected_at;
            let mut last_event_id = initial_event_id;

            loop {
                if sender.is_closed() {
                    trace!(group = %group, library = %library_id, "file change stream dropped; stopping poll loop");
                    break;
                }

                match fetch_events_after(
                    &pool,
                    library_id,
                    last_detected_at,
                    last_event_id,
                    fetch_limit,
                )
                .await
                {
                    Ok(batch) if batch.is_empty() => {
                        sleep(poll_interval).await;
                    }
                    Ok(batch) => {
                        debug!(count = batch.len(), group = %group, library = %library_id, "delivering file watch events");
                        for event in batch {
                            last_detected_at = Some(event.detected_at);
                            last_event_id = Some(event.id);
                            if sender.send(event).await.is_err() {
                                trace!(group = %group, library = %library_id, "receiver dropped while streaming file watch events");
                                return;
                            }
                        }
                    }
                    Err(err) => {
                        error!(group = %group, library = %library_id, error = %err, "file watch polling failed");
                        sleep(poll_interval).await;
                    }
                }
            }
        });

        Ok(Box::pin(ReceiverStream::new(rx)))
    }

    async fn ack(&self, group: &str, event_id: Uuid) -> Result<()> {
        let Some((library_id, detected_at)) = fetch_event(self.pool(), event_id).await? else {
            return Err(MediaError::NotFound("file watch event not found".into()));
        };

        let cursor = FileChangeCursor {
            group: group.to_owned(),
            library_id,
            last_event_id: Some(event_id),
            last_detected_at: Some(detected_at),
        };
        upsert_cursor(self.pool(), &cursor).await?;
        set_processed(self.pool(), event_id).await
    }

    async fn commit_cursor(&self, cursor: FileChangeCursor) -> Result<()> {
        upsert_cursor(self.pool(), &cursor).await
    }

    async fn get_cursor(
        &self,
        group: &str,
        library_id: LibraryID,
    ) -> Result<Option<FileChangeCursor>> {
        load_cursor(self.pool(), group, library_id).await
    }

    async fn get_unprocessed_events(
        &self,
        library_id: LibraryID,
        limit: i32,
    ) -> Result<Vec<FileWatchEvent>> {
        let rows = sqlx::query_as::<_, FileWatchEventRow>(
            r#"
            SELECT
                id,
                library_id,
                event_type,
                file_path,
                old_path,
                file_size,
                detected_at,
                processed,
                processed_at,
                processing_attempts,
                last_error
            FROM file_watch_events
            WHERE library_id = $1 AND processed = false
            ORDER BY detected_at ASC
            LIMIT $2
            "#,
        )
        .bind(library_id.as_uuid())
        .bind(limit as i64)
        .fetch_all(self.pool())
        .await
        .map_err(|err| {
            MediaError::Internal(format!(
                "failed to load unprocessed file watch events: {err}"
            ))
        })?;

        let mut events = Vec::with_capacity(rows.len());
        for row in rows {
            if let Some(event) = row.into_event() {
                events.push(event);
            } else {
                warn!("skipping file watch event with unknown type");
            }
        }

        Ok(events)
    }

    async fn mark_processed(&self, event_id: Uuid) -> Result<()> {
        set_processed(self.pool(), event_id).await
    }

    async fn cleanup_retention(&self, days_to_keep: i32) -> Result<u32> {
        cleanup_old_events(self.pool(), days_to_keep).await
    }
}
