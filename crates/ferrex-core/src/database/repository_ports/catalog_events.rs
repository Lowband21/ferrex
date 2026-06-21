use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    error::Result,
    types::{LibraryId, MovieBatchId, SeriesID},
};

/// Durable catalog/media event kinds replayed by `/api/v1/events/media`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CatalogEventKind {
    MovieBatchFinalized,
    SeriesBundleFinalized,
}

impl CatalogEventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MovieBatchFinalized => "movie_batch_finalized",
            Self::SeriesBundleFinalized => "series_bundle_finalized",
        }
    }
}

impl TryFrom<&str> for CatalogEventKind {
    type Error = String;

    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        match value {
            "movie_batch_finalized" => Ok(Self::MovieBatchFinalized),
            "series_bundle_finalized" => Ok(Self::SeriesBundleFinalized),
            other => Err(format!("unknown catalog event kind: {other}")),
        }
    }
}

/// New idempotent event append request for the durable catalog outbox.
#[derive(Debug, Clone, PartialEq)]
pub struct NewCatalogEvent {
    pub kind: CatalogEventKind,
    pub library_id: LibraryId,
    pub entity_kind: String,
    pub entity_id: Option<Uuid>,
    pub movie_batch_id: Option<MovieBatchId>,
    pub payload: Value,
    pub idempotency_key: String,
    pub occurred_at: DateTime<Utc>,
}

impl NewCatalogEvent {
    pub fn movie_batch_finalized(
        library_id: LibraryId,
        batch_id: MovieBatchId,
        version: u64,
    ) -> Self {
        Self {
            kind: CatalogEventKind::MovieBatchFinalized,
            library_id,
            entity_kind: "movie_batch".to_string(),
            entity_id: None,
            movie_batch_id: Some(batch_id),
            payload: json!({
                "library_id": library_id.to_uuid(),
                "batch_id": batch_id.as_u32(),
                "version": version,
            }),
            idempotency_key: format!(
                "catalog:movie_batch_finalized:{library_id}:{batch_id}:{version}"
            ),
            occurred_at: Utc::now(),
        }
    }

    pub fn series_bundle_finalized(
        library_id: LibraryId,
        series_id: SeriesID,
        version: u64,
    ) -> Self {
        Self {
            kind: CatalogEventKind::SeriesBundleFinalized,
            library_id,
            entity_kind: "series".to_string(),
            entity_id: Some(series_id.to_uuid()),
            movie_batch_id: None,
            payload: json!({
                "library_id": library_id.to_uuid(),
                "series_id": series_id.to_uuid(),
                "version": version,
            }),
            idempotency_key: format!(
                "catalog:series_bundle_finalized:{library_id}:{series_id}:{version}"
            ),
            occurred_at: Utc::now(),
        }
    }
}

/// Durable catalog event row as stored in `catalog_events`.
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogEventRecord {
    pub sequence: u64,
    pub kind: CatalogEventKind,
    pub library_id: LibraryId,
    pub entity_kind: String,
    pub entity_id: Option<Uuid>,
    pub movie_batch_id: Option<MovieBatchId>,
    pub payload: Value,
    pub idempotency_key: String,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CatalogEventAppendResult {
    pub record: CatalogEventRecord,
    pub inserted: bool,
}

#[async_trait]
pub trait CatalogEventsRepository: Send + Sync {
    /// Append an event using its idempotency key, returning the inserted row or
    /// the already-existing row when a retry/duplicate reaches this path.
    async fn append_idempotent(
        &self,
        event: NewCatalogEvent,
    ) -> Result<CatalogEventAppendResult>;

    /// Replay durable catalog events strictly after the caller's last sequence.
    async fn list_after_sequence(
        &self,
        sequence: u64,
    ) -> Result<Vec<CatalogEventRecord>>;

    /// Replay recent durable catalog events for clients that connect without a cursor.
    async fn list_since(
        &self,
        since: DateTime<Utc>,
    ) -> Result<Vec<CatalogEventRecord>>;
}

#[cfg(test)]
mod tests {
    use super::NewCatalogEvent;
    use crate::types::{LibraryId, MovieBatchId, SeriesID};
    use uuid::Uuid;

    #[test]
    fn movie_batch_idempotency_is_stable_per_version() {
        let library_id = LibraryId(Uuid::from_u128(1));
        let batch_id = MovieBatchId::new(7).unwrap();

        let first =
            NewCatalogEvent::movie_batch_finalized(library_id, batch_id, 3);
        let duplicate =
            NewCatalogEvent::movie_batch_finalized(library_id, batch_id, 3);
        let changed =
            NewCatalogEvent::movie_batch_finalized(library_id, batch_id, 4);

        assert_eq!(first.idempotency_key, duplicate.idempotency_key);
        assert_ne!(first.idempotency_key, changed.idempotency_key);
        assert_eq!(first.movie_batch_id, Some(batch_id));
        assert!(first.entity_id.is_none());
    }

    #[test]
    fn series_bundle_idempotency_is_stable_per_version() {
        let library_id = LibraryId(Uuid::from_u128(1));
        let series_id = SeriesID(Uuid::from_u128(2));

        let first =
            NewCatalogEvent::series_bundle_finalized(library_id, series_id, 9);
        let duplicate =
            NewCatalogEvent::series_bundle_finalized(library_id, series_id, 9);
        let changed =
            NewCatalogEvent::series_bundle_finalized(library_id, series_id, 10);

        assert_eq!(first.idempotency_key, duplicate.idempotency_key);
        assert_ne!(first.idempotency_key, changed.idempotency_key);
        assert_eq!(first.entity_id, Some(series_id.to_uuid()));
        assert!(first.movie_batch_id.is_none());
    }
}
