use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row, postgres::PgRow};
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

fn decode_row(row: PgRow) -> Result<Option<FileWatchEvent>> {
    let event_type_raw: String = row.try_get("event_type").map_err(|err| {
        MediaError::Internal(format!(
            "failed to decode file watch event type: {err}"
        ))
    })?;
    let Some(event_type) = str_to_event_type(&event_type_raw) else {
        return Ok(None);
    };

    Ok(Some(FileWatchEvent {
        id: row.try_get("id").map_err(|err| {
            MediaError::Internal(format!(
                "failed to decode file watch event id: {err}"
            ))
        })?,
        event_version: row.try_get("event_version").map_err(|err| {
            MediaError::Internal(format!(
                "failed to decode file watch event version: {err}"
            ))
        })?,
        library_id: LibraryId(row.try_get("library_id").map_err(|err| {
            MediaError::Internal(format!(
                "failed to decode file watch library id: {err}"
            ))
        })?),
        library_root_id: row.try_get("library_root_id").map_err(|err| {
            MediaError::Internal(format!(
                "failed to decode file watch root id: {err}"
            ))
        })?,
        root_path: row.try_get("root_path").map_err(|err| {
            MediaError::Internal(format!(
                "failed to decode file watch root path: {err}"
            ))
        })?,
        event_type,
        file_path: row.try_get("file_path").map_err(|err| {
            MediaError::Internal(format!(
                "failed to decode file watch file path: {err}"
            ))
        })?,
        path_key: row.try_get("path_key").map_err(|err| {
            MediaError::Internal(format!(
                "failed to decode file watch path key: {err}"
            ))
        })?,
        old_path: row.try_get("old_path").map_err(|err| {
            MediaError::Internal(format!(
                "failed to decode file watch old path: {err}"
            ))
        })?,
        fingerprint: row.try_get("fingerprint").map_err(|err| {
            MediaError::Internal(format!(
                "failed to decode file watch fingerprint: {err}"
            ))
        })?,
        file_size: row.try_get("file_size").map_err(|err| {
            MediaError::Internal(format!(
                "failed to decode file watch file size: {err}"
            ))
        })?,
        file_modified_at: row.try_get("file_modified_at").map_err(|err| {
            MediaError::Internal(format!(
                "failed to decode file watch modified time: {err}"
            ))
        })?,
        correlation_id: row.try_get("correlation_id").map_err(|err| {
            MediaError::Internal(format!(
                "failed to decode file watch correlation id: {err}"
            ))
        })?,
        idempotency_key: row.try_get("idempotency_key").map_err(|err| {
            MediaError::Internal(format!(
                "failed to decode file watch idempotency key: {err}"
            ))
        })?,
        detected_at: row.try_get("detected_at").map_err(|err| {
            MediaError::Internal(format!(
                "failed to decode file watch detected timestamp: {err}"
            ))
        })?,
        processed: row.try_get("processed").map_err(|err| {
            MediaError::Internal(format!(
                "failed to decode file watch processed flag: {err}"
            ))
        })?,
        processed_at: row.try_get("processed_at").map_err(|err| {
            MediaError::Internal(format!(
                "failed to decode file watch processed timestamp: {err}"
            ))
        })?,
        processing_attempts: row.try_get("processing_attempts").map_err(
            |err| {
                MediaError::Internal(format!(
                    "failed to decode file watch attempts: {err}"
                ))
            },
        )?,
        last_error: row.try_get("last_error").map_err(|err| {
            MediaError::Internal(format!(
                "failed to decode file watch last error: {err}"
            ))
        })?,
    }))
}

async fn fetch_events_after(
    pool: &PgPool,
    library_id: LibraryId,
    last_detected_at: Option<DateTime<Utc>>,
    last_event_id: Option<Uuid>,
    limit: i64,
) -> Result<Vec<FileWatchEvent>> {
    let sql = format!(
        "{FILE_WATCH_SELECT} WHERE library_id = $1 AND ($2::timestamptz IS NULL OR detected_at > $2 OR (detected_at = $2 AND id > $3)) ORDER BY detected_at ASC, id ASC LIMIT $4"
    );
    let rows = sqlx::query(&sql)
        .bind(library_id.as_uuid())
        .bind(last_detected_at)
        .bind(last_event_id)
        .bind(limit)
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
    let row = sqlx::query(
        "SELECT library_id, detected_at FROM file_watch_events WHERE id = $1",
    )
    .bind(event_id)
    .fetch_optional(pool)
    .await
    .map_err(|err| {
        MediaError::Internal(format!(
            "failed to load file watch event by id: {err}"
        ))
    })?;

    row.map(|row| {
        let library_id =
            row.try_get("library_id").map(LibraryId).map_err(|err| {
                MediaError::Internal(format!(
                    "failed to decode file watch event library id: {err}"
                ))
            })?;
        let detected_at = row.try_get("detected_at").map_err(|err| {
            MediaError::Internal(format!(
                "failed to decode file watch event timestamp: {err}"
            ))
        })?;
        Ok((library_id, detected_at))
    })
    .transpose()
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
    let row = sqlx::query(
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
    .map_err(|err| {
        MediaError::Internal(format!("failed to load file watch cursor: {err}"))
    })?;

    row.map(|record| {
        Ok(FileChangeCursor {
            group: record.try_get("group_name").map_err(|err| {
                MediaError::Internal(format!(
                    "failed to decode file watch cursor group: {err}"
                ))
            })?,
            library_id: LibraryId(record.try_get("library_id").map_err(
                |err| {
                    MediaError::Internal(format!(
                        "failed to decode file watch cursor library id: {err}"
                    ))
                },
            )?),
            last_event_id: record.try_get("last_event_id").map_err(|err| {
                MediaError::Internal(format!(
                    "failed to decode file watch cursor event id: {err}"
                ))
            })?,
            last_detected_at: record.try_get("last_detected_at").map_err(
                |err| {
                    MediaError::Internal(format!(
                        "failed to decode file watch cursor timestamp: {err}"
                    ))
                },
            )?,
        })
    })
    .transpose()
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
        MediaError::Internal(format!(
            "failed to mark file watch event processed: {err}"
        ))
    })?;

    Ok(())
}

async fn cleanup_old_events(pool: &PgPool, days_to_keep: i32) -> Result<u32> {
    let affected = sqlx::query(
        r#"
        DELETE FROM file_watch_events
        WHERE processed = true
          AND processed_at IS NOT NULL
          AND processed_at < NOW() - ($1 || ' days')::interval
        "#,
    )
    .bind(days_to_keep.to_string())
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
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
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
        let sql = format!(
            "{FILE_WATCH_SELECT} WHERE library_id = $1 AND processed = false ORDER BY detected_at ASC, id ASC LIMIT $2"
        );
        let rows = sqlx::query(&sql)
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

        let has_idempotency_key = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM information_schema.columns
                WHERE table_schema = 'public'
                  AND table_name = 'file_watch_events'
                  AND column_name = 'idempotency_key'
            )
            "#,
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
        sqlx::query(
            r#"
            INSERT INTO libraries (id, name, library_type, paths)
            VALUES ($1, $2, 'movies', $3)
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(library_id.as_uuid())
        .bind(format!("file-watch-test-{library_id}"))
        .bind(vec!["/tmp/ferrex-watch-test".to_string()])
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

        sqlx::query("DELETE FROM libraries WHERE id = $1")
            .bind(library_id.as_uuid())
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
