use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, Row, postgres::PgRow};

use crate::{
    database::repository_ports::catalog_events::{
        CatalogEventAppendResult, CatalogEventKind, CatalogEventRecord,
        CatalogEventsRepository, NewCatalogEvent,
    },
    error::{MediaError, Result},
    types::{LibraryId, MovieBatchId},
};

#[derive(Clone, Debug)]
pub struct PostgresCatalogEventsRepository {
    pool: PgPool,
}

impl PostgresCatalogEventsRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn fetch_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<CatalogEventRecord> {
        let row = sqlx::query(
            r#"
            SELECT
                sequence,
                event_kind,
                library_id,
                entity_kind,
                entity_id,
                movie_batch_id,
                payload,
                idempotency_key,
                occurred_at
            FROM catalog_events
            WHERE idempotency_key = $1
            "#,
        )
        .bind(idempotency_key)
        .fetch_one(&self.pool)
        .await
        .map_err(|err| {
            MediaError::Internal(format!(
                "Database query failed for catalog event idempotency lookup: {err}"
            ))
        })?;

        catalog_event_record_from_row(row)
    }
}

#[async_trait]
impl CatalogEventsRepository for PostgresCatalogEventsRepository {
    async fn append_idempotent(
        &self,
        event: NewCatalogEvent,
    ) -> Result<CatalogEventAppendResult> {
        let inserted = sqlx::query(
            r#"
            INSERT INTO catalog_events (
                event_kind,
                library_id,
                entity_kind,
                entity_id,
                movie_batch_id,
                payload,
                idempotency_key,
                occurred_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (idempotency_key) DO NOTHING
            RETURNING
                sequence,
                event_kind,
                library_id,
                entity_kind,
                entity_id,
                movie_batch_id,
                payload,
                idempotency_key,
                occurred_at
            "#,
        )
        .bind(event.kind.as_str())
        .bind(event.library_id.to_uuid())
        .bind(&event.entity_kind)
        .bind(event.entity_id)
        .bind(event.movie_batch_id.map(|batch_id| batch_id.as_i64()))
        .bind(event.payload)
        .bind(&event.idempotency_key)
        .bind(event.occurred_at)
        .fetch_optional(&self.pool)
        .await
        .map_err(|err| {
            MediaError::Internal(format!(
                "Database insert failed for catalog event append: {err}"
            ))
        })?;

        if let Some(row) = inserted {
            return Ok(CatalogEventAppendResult {
                record: catalog_event_record_from_row(row)?,
                inserted: true,
            });
        }

        let record = self
            .fetch_by_idempotency_key(&event.idempotency_key)
            .await?;
        Ok(CatalogEventAppendResult {
            record,
            inserted: false,
        })
    }

    async fn list_after_sequence(
        &self,
        sequence: u64,
    ) -> Result<Vec<CatalogEventRecord>> {
        let sequence = i64::try_from(sequence).unwrap_or(i64::MAX);
        let rows = sqlx::query(
            r#"
            SELECT
                sequence,
                event_kind,
                library_id,
                entity_kind,
                entity_id,
                movie_batch_id,
                payload,
                idempotency_key,
                occurred_at
            FROM catalog_events
            WHERE sequence > $1
            ORDER BY sequence ASC
            "#,
        )
        .bind(sequence)
        .fetch_all(&self.pool)
        .await
        .map_err(|err| {
            MediaError::Internal(format!(
                "Database query failed for catalog event sequence replay: {err}"
            ))
        })?;

        rows.into_iter()
            .map(catalog_event_record_from_row)
            .collect()
    }

    async fn list_since(
        &self,
        since: DateTime<Utc>,
    ) -> Result<Vec<CatalogEventRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT
                sequence,
                event_kind,
                library_id,
                entity_kind,
                entity_id,
                movie_batch_id,
                payload,
                idempotency_key,
                occurred_at
            FROM catalog_events
            WHERE occurred_at >= $1
            ORDER BY sequence ASC
            "#,
        )
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(|err| {
            MediaError::Internal(format!(
                "Database query failed for recent catalog event replay: {err}"
            ))
        })?;

        rows.into_iter()
            .map(catalog_event_record_from_row)
            .collect()
    }
}

fn catalog_event_record_from_row(row: PgRow) -> Result<CatalogEventRecord> {
    let sequence = row.try_get::<i64, _>("sequence").map_err(row_error)?;
    if sequence <= 0 {
        return Err(MediaError::Internal(format!(
            "Invalid catalog event sequence {sequence}"
        )));
    }

    let event_kind =
        row.try_get::<String, _>("event_kind").map_err(row_error)?;
    let kind = CatalogEventKind::try_from(event_kind.as_str())
        .map_err(MediaError::Internal)?;

    let movie_batch_id = row
        .try_get::<Option<i64>, _>("movie_batch_id")
        .map_err(row_error)?
        .map(|raw| {
            let raw = u32::try_from(raw).map_err(|_| {
                MediaError::Internal(format!(
                    "Invalid catalog event movie batch id {raw}"
                ))
            })?;
            MovieBatchId::new(raw).map_err(|err| {
                MediaError::Internal(format!(
                    "Invalid catalog event movie batch id {raw}: {err}"
                ))
            })
        })
        .transpose()?;

    Ok(CatalogEventRecord {
        sequence: sequence as u64,
        kind,
        library_id: LibraryId(row.try_get("library_id").map_err(row_error)?),
        entity_kind: row.try_get("entity_kind").map_err(row_error)?,
        entity_id: row.try_get("entity_id").map_err(row_error)?,
        movie_batch_id,
        payload: row.try_get::<Value, _>("payload").map_err(row_error)?,
        idempotency_key: row.try_get("idempotency_key").map_err(row_error)?,
        occurred_at: row.try_get("occurred_at").map_err(row_error)?,
    })
}

fn row_error(err: sqlx::Error) -> MediaError {
    MediaError::Internal(format!("Invalid catalog event row: {err}"))
}
