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
    use super::{
        CatalogEventAppendResult, CatalogEventKind, CatalogEventRecord,
        CatalogEventsRepository, NewCatalogEvent,
    };
    use crate::{
        error::Result,
        types::{LibraryId, MovieBatchId, SeriesID},
    };
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use std::sync::Mutex;
    use uuid::Uuid;

    #[derive(Default)]
    struct InMemoryCatalogEventsRepository {
        records: Mutex<Vec<CatalogEventRecord>>,
    }

    #[async_trait]
    impl CatalogEventsRepository for InMemoryCatalogEventsRepository {
        async fn append_idempotent(
            &self,
            event: NewCatalogEvent,
        ) -> Result<CatalogEventAppendResult> {
            let result = {
                let mut records = self.records.lock().expect(
                    "catalog event records mutex should not be poisoned",
                );

                if let Some(record) = records.iter().find(|record| {
                    record.idempotency_key == event.idempotency_key
                }) {
                    CatalogEventAppendResult {
                        record: record.clone(),
                        inserted: false,
                    }
                } else {
                    let record = CatalogEventRecord {
                        sequence: records.len() as u64 + 1,
                        kind: event.kind,
                        library_id: event.library_id,
                        entity_kind: event.entity_kind,
                        entity_id: event.entity_id,
                        movie_batch_id: event.movie_batch_id,
                        payload: event.payload,
                        idempotency_key: event.idempotency_key,
                        occurred_at: event.occurred_at,
                    };
                    records.push(record.clone());
                    CatalogEventAppendResult {
                        record,
                        inserted: true,
                    }
                }
            };

            Ok(result)
        }

        async fn list_after_sequence(
            &self,
            sequence: u64,
        ) -> Result<Vec<CatalogEventRecord>> {
            let records = self
                .records
                .lock()
                .expect("catalog event records mutex should not be poisoned")
                .iter()
                .filter(|record| record.sequence > sequence)
                .cloned()
                .collect();
            Ok(records)
        }

        async fn list_since(
            &self,
            since: DateTime<Utc>,
        ) -> Result<Vec<CatalogEventRecord>> {
            let records = self
                .records
                .lock()
                .expect("catalog event records mutex should not be poisoned")
                .iter()
                .filter(|record| record.occurred_at >= since)
                .cloned()
                .collect();
            Ok(records)
        }
    }

    #[tokio::test]
    async fn durable_catalog_outbox_replay_returns_missed_finalizations_in_order()
    -> Result<()> {
        let repo = InMemoryCatalogEventsRepository::default();
        let library_id = LibraryId(Uuid::from_u128(1));
        let already_seen_batch = MovieBatchId::new(1).unwrap();
        let missed_batch = MovieBatchId::new(2).unwrap();
        let missed_series_id = SeriesID(Uuid::from_u128(3));

        let cursor = repo
            .append_idempotent(NewCatalogEvent::movie_batch_finalized(
                library_id,
                already_seen_batch,
                1,
            ))
            .await?
            .record
            .sequence;

        let missed_movie = repo
            .append_idempotent(NewCatalogEvent::movie_batch_finalized(
                library_id,
                missed_batch,
                2,
            ))
            .await?;
        assert!(missed_movie.inserted);

        let duplicate_movie = repo
            .append_idempotent(NewCatalogEvent::movie_batch_finalized(
                library_id,
                missed_batch,
                2,
            ))
            .await?;
        assert!(!duplicate_movie.inserted);
        assert_eq!(
            duplicate_movie.record.sequence,
            missed_movie.record.sequence
        );

        let missed_series = repo
            .append_idempotent(NewCatalogEvent::series_bundle_finalized(
                library_id,
                missed_series_id,
                1,
            ))
            .await?;
        assert!(missed_series.inserted);

        let replay = repo.list_after_sequence(cursor).await?;
        assert_eq!(replay.len(), 2);
        assert_eq!(
            replay
                .iter()
                .map(|record| record.sequence)
                .collect::<Vec<_>>(),
            vec![missed_movie.record.sequence, missed_series.record.sequence]
        );
        assert_eq!(replay[0].kind, CatalogEventKind::MovieBatchFinalized);
        assert_eq!(replay[0].movie_batch_id, Some(missed_batch));
        assert_eq!(replay[1].kind, CatalogEventKind::SeriesBundleFinalized);
        assert_eq!(replay[1].entity_id, Some(missed_series_id.to_uuid()));
        assert!(
            repo.list_after_sequence(missed_series.record.sequence)
                .await?
                .is_empty()
        );

        Ok(())
    }

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
