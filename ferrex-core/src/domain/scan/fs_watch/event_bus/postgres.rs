use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, error, trace, warn};
use uuid::Uuid;

use super::{FileChangeCursor, FileChangeEventBus, FileChangeEventStream};
use crate::database::postgres::PostgresDatabase;
use crate::database::traits::{FileWatchEvent, FileWatchEventType};
use crate::error::{MediaError, Result};
use crate::types::ids::LibraryId;

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

    pub fn with_config(
        pool: PgPool,
        config: PostgresFileChangeEventBusConfig,
    ) -> Self {
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

fn decode_row(row: FileWatchEventRow) -> Result<Option<FileWatchEvent>> {
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

async fn fetch_events_after(
    pool: &PgPool,
    library_id: LibraryId,
    last_detected_at: Option<DateTime<Utc>>,
    last_event_id: Option<Uuid>,
    limit: i64,
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
        WHERE library_id = $1
          AND (
                $2::timestamptz IS NULL
                OR detected_at > $2
                OR (detected_at = $2 AND id > $3)
          )
        ORDER BY detected_at ASC, id ASC
        LIMIT $4
        "#,
        library_id.as_uuid(),
        last_detected_at,
        last_event_id,
        limit
    )
    .fetch_all(pool)
    .await
    .map_err(|err| {
        MediaError::Internal(format!(
            "failed to fetch file watch events: {err}"
        ))
    })?;

    let mut events = Vec::with_capacity(rows.len());
    for row in rows {
        if let Some(event) = decode_row(row)? {
            events.push(event);
        } else {
            warn!("skipping file watch event with unknown type");
        }
    }

    Ok(events)
}

async fn fetch_event(
    pool: &PgPool,
    event_id: Uuid,
) -> Result<Option<(LibraryId, DateTime<Utc>)>> {
    let row = sqlx::query!(
        "SELECT library_id, detected_at FROM file_watch_events WHERE id = $1",
        event_id
    )
    .fetch_optional(pool)
    .await
    .map_err(|err| {
        MediaError::Internal(format!(
            "failed to load file watch event by id: {err}"
        ))
    })?;

    Ok(row.map(|row| (LibraryId(row.library_id), row.detected_at)))
}

async fn upsert_cursor(pool: &PgPool, cursor: &FileChangeCursor) -> Result<()> {
    sqlx::query!(
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
        &cursor.group,
        cursor.library_id.as_uuid(),
        cursor.last_event_id,
        cursor.last_detected_at
    )
    .execute(pool)
    .await
    .map_err(|err| {
        MediaError::Internal(format!(
            "failed to upsert file watch cursor: {err}"
        ))
    })?;

    Ok(())
}

async fn load_cursor(
    pool: &PgPool,
    group: &str,
    library_id: LibraryId,
) -> Result<Option<FileChangeCursor>> {
    let row = sqlx::query!(
        r#"
        SELECT group_name, library_id, last_event_id, last_detected_at
        FROM file_watch_consumer_offsets
        WHERE group_name = $1 AND library_id = $2
        "#,
        group,
        library_id.as_uuid()
    )
    .fetch_optional(pool)
    .await
    .map_err(|err| {
        MediaError::Internal(format!("failed to load file watch cursor: {err}"))
    })?;

    Ok(row.map(|record| FileChangeCursor {
        group: record.group_name,
        library_id: LibraryId(record.library_id),
        last_event_id: record.last_event_id,
        last_detected_at: record.last_detected_at,
    }))
}

async fn set_processed(pool: &PgPool, event_id: Uuid) -> Result<()> {
    sqlx::query!(
        r#"
        UPDATE file_watch_events
        SET processed = true,
            processed_at = NOW()
        WHERE id = $1
        "#,
        event_id
    )
    .execute(pool)
    .await
    .map_err(|err| {
        MediaError::Internal(format!(
            "failed to mark file watch event processed: {err}"
        ))
    })?;

    Ok(())
}

async fn cleanup_old_events(pool: &PgPool, days_to_keep: i32) -> Result<u32> {
    let affected = sqlx::query!(
        r#"
        DELETE FROM file_watch_events
        WHERE processed = true
          AND processed_at IS NOT NULL
          AND processed_at < NOW() - ($1::integer * INTERVAL '1 day')
        "#,
        days_to_keep
    )
    .execute(pool)
    .await
    .map_err(|err| {
        MediaError::Internal(format!(
            "failed to clean up file watch events: {err}"
        ))
    })?
    .rows_affected();

    Ok(affected as u32)
}

#[async_trait]
impl FileChangeEventBus for PostgresFileChangeEventBus {
    async fn publish(&self, event: FileWatchEvent) -> Result<bool> {
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
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
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
        .map_err(|err| {
            MediaError::Internal(format!(
                "failed to persist file watch event: {err}"
            ))
        })?;

        Ok(result.rows_affected() == 1)
    }

    async fn subscribe(
        &self,
        group: &str,
        library_id: LibraryId,
    ) -> Result<FileChangeEventStream> {
        let cursor = load_cursor(self.pool(), group, library_id).await?;
        let initial_detected_at =
            cursor.as_ref().and_then(|cursor| cursor.last_detected_at);
        let initial_event_id =
            cursor.as_ref().and_then(|cursor| cursor.last_event_id);

        let (tx, rx) = mpsc::channel(self.config.channel_capacity);
        let pool = self.pool.clone();
        let group = group.to_owned();
        let poll_interval = self.config.poll_interval;
        let fetch_limit = self.config.fetch_limit;

        tokio::spawn(async move {
            let sender = tx;
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
        let Some((library_id, detected_at)) =
            fetch_event(self.pool(), event_id).await?
        else {
            return Err(MediaError::NotFound(
                "file watch event not found".into(),
            ));
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
        library_id: LibraryId,
    ) -> Result<Option<FileChangeCursor>> {
        load_cursor(self.pool(), group, library_id).await
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
        .map_err(|err| {
            MediaError::Internal(format!(
                "failed to load unprocessed file watch events: {err}"
            ))
        })?;

        let mut events = Vec::with_capacity(rows.len());
        for row in rows {
            if let Some(event) = decode_row(row)? {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MIGRATOR;
    use futures::StreamExt;

    async fn maybe_pool() -> Option<PgPool> {
        let database_url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!(
                    "skipping Postgres file-watch event-bus test; DATABASE_URL is not set"
                );
                return None;
            }
        };

        let pool = match PgPool::connect(&database_url).await {
            Ok(pool) => pool,
            Err(err) => {
                eprintln!(
                    "skipping Postgres file-watch event-bus test; connect failed: {err}"
                );
                return None;
            }
        };

        if let Err(err) = MIGRATOR.run(&pool).await {
            eprintln!(
                "skipping Postgres file-watch event-bus test; migrations failed: {err}"
            );
            return None;
        }

        let has_idempotency_key = sqlx::query_scalar!(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM information_schema.columns
                WHERE table_schema = 'public'
                  AND table_name = 'file_watch_events'
                  AND column_name = 'idempotency_key'
            ) AS "exists!"
            "#
        )
        .fetch_one(&pool)
        .await
        .unwrap_or(false);

        if !has_idempotency_key {
            eprintln!(
                "skipping Postgres file-watch event-bus test; durable schema columns are unavailable"
            );
            return None;
        }

        Some(pool)
    }

    async fn insert_library(
        pool: &PgPool,
        library_id: LibraryId,
    ) -> Result<()> {
        let library_name = format!("file-watch-test-{library_id}");
        let paths = vec!["/tmp/ferrex-watch-test".to_string()];
        sqlx::query!(
            r#"
            INSERT INTO libraries (id, name, library_type, paths)
            VALUES ($1, $2, 'movies', $3)
            ON CONFLICT (id) DO NOTHING
            "#,
            library_id.as_uuid(),
            library_name,
            &paths
        )
        .execute(pool)
        .await
        .map_err(|err| {
            MediaError::Internal(format!(
                "failed to insert test library: {err}"
            ))
        })?;
        Ok(())
    }

    fn event(
        library_id: LibraryId,
        event_type: FileWatchEventType,
        suffix: &str,
    ) -> FileWatchEvent {
        let id = Uuid::now_v7();
        FileWatchEvent {
            id,
            event_version: 1,
            library_id,
            library_root_id: 0,
            root_path: "/tmp/ferrex-watch-test".into(),
            event_type,
            file_path: format!("/tmp/ferrex-watch-test/{suffix}"),
            path_key: format!("/tmp/ferrex-watch-test/{suffix}"),
            old_path: None,
            fingerprint: Some(format!("fingerprint-{suffix}")),
            file_size: Some(12),
            file_modified_at: Some(Utc::now()),
            correlation_id: Some(Uuid::now_v7()),
            idempotency_key: format!("test-{library_id}-{suffix}"),
            detected_at: Utc::now(),
            processed: false,
            processed_at: None,
            processing_attempts: 0,
            last_error: None,
        }
    }

    #[tokio::test]
    async fn postgres_event_bus_publish_replay_ack_duplicate_and_cleanup()
    -> Result<()> {
        let Some(pool) = maybe_pool().await else {
            return Ok(());
        };
        let library_id = LibraryId::new();
        insert_library(&pool, library_id).await?;
        let bus = PostgresFileChangeEventBus::with_config(
            pool.clone(),
            PostgresFileChangeEventBusConfig {
                fetch_limit: 8,
                channel_capacity: 8,
                poll_interval: Duration::from_millis(10),
            },
        );

        let created = event(library_id, FileWatchEventType::Created, "a.mkv");
        assert!(bus.publish(created.clone()).await?);
        assert!(!bus.publish(created.clone()).await?);

        let overflow = event(library_id, FileWatchEventType::Overflow, "root");
        assert!(bus.publish(overflow.clone()).await?);

        let replay = bus.get_unprocessed_events(library_id, 8).await?;
        assert_eq!(replay.len(), 2);
        assert_eq!(replay[0].id, created.id);
        assert_eq!(replay[1].event_type, FileWatchEventType::Overflow);

        let mut stream = bus.subscribe("scan-watch-test", library_id).await?;
        let streamed =
            tokio::time::timeout(Duration::from_secs(2), stream.next())
                .await
                .expect("streamed event")
                .expect("stream item");
        assert_eq!(streamed.id, created.id);

        bus.ack("scan-watch-test", created.id).await?;
        let cursor = bus
            .get_cursor("scan-watch-test", library_id)
            .await?
            .expect("cursor");
        assert_eq!(cursor.last_event_id, Some(created.id));

        let old = FileWatchEvent {
            id: Uuid::now_v7(),
            idempotency_key: format!("old-{library_id}"),
            detected_at: Utc::now() - chrono::Duration::days(3),
            processed: true,
            processed_at: Some(Utc::now() - chrono::Duration::days(2)),
            ..event(library_id, FileWatchEventType::Modified, "old.mkv")
        };
        assert!(bus.publish(old.clone()).await?);
        let removed = bus.cleanup_retention(1).await?;
        assert!(removed >= 1);

        sqlx::query!(
            "DELETE FROM libraries WHERE id = $1",
            library_id.as_uuid()
        )
        .execute(&pool)
        .await
        .map_err(|err| {
            MediaError::Internal(format!(
                "failed to clean up test library: {err}"
            ))
        })?;

        Ok(())
    }
}
