use std::time::Instant;

use ferrex_core::{
    database::repository_ports::catalog_events::{
        CatalogEventKind, CatalogEventRecord,
    },
    error::MediaError,
    types::{
        LibraryId, MediaEvent, MediaID, MovieBatchId, MovieReference, Series,
        SeriesID, events::MediaSseEventType,
    },
};
use tokio::sync::broadcast;

/// Catalog/media invalidation event for `/api/v1/events/media`.
///
/// This is intentionally separate from scan progress telemetry: scan lifecycle
/// frames use `/api/v1/scan/{id}/progress` and cannot be represented by this
/// type, so they cannot be accidentally published on the catalog stream.
#[derive(Debug, Clone, PartialEq)]
pub enum CatalogEvent {
    MovieAdded {
        movie: MovieReference,
    },
    MovieBatchFinalized {
        library_id: LibraryId,
        batch_id: MovieBatchId,
    },
    SeriesAdded {
        series: Series,
    },
    SeriesBundleFinalized {
        library_id: LibraryId,
        series_id: SeriesID,
    },
    MovieUpdated {
        movie: MovieReference,
    },
    SeriesUpdated {
        series: Series,
    },
    MediaDeleted {
        id: MediaID,
    },
}

impl CatalogEvent {
    pub fn sse_event_type(&self) -> MediaSseEventType {
        match self {
            Self::MovieAdded { .. } => MediaSseEventType::MovieAdded,
            Self::MovieBatchFinalized { .. } => {
                MediaSseEventType::MovieBatchFinalized
            }
            Self::SeriesAdded { .. } => MediaSseEventType::SeriesAdded,
            Self::SeriesBundleFinalized { .. } => {
                MediaSseEventType::SeriesBundleFinalized
            }
            Self::MovieUpdated { .. } => MediaSseEventType::MovieUpdated,
            Self::SeriesUpdated { .. } => MediaSseEventType::SeriesUpdated,
            Self::MediaDeleted { .. } => MediaSseEventType::MediaDeleted,
        }
    }

    pub fn into_media_event(self) -> MediaEvent {
        match self {
            Self::MovieAdded { movie } => MediaEvent::MovieAdded { movie },
            Self::MovieBatchFinalized {
                library_id,
                batch_id,
            } => MediaEvent::MovieBatchFinalized {
                library_id,
                batch_id,
            },
            Self::SeriesAdded { series } => MediaEvent::SeriesAdded { series },
            Self::SeriesBundleFinalized {
                library_id,
                series_id,
            } => MediaEvent::SeriesBundleFinalized {
                library_id,
                series_id,
            },
            Self::MovieUpdated { movie } => MediaEvent::MovieUpdated { movie },
            Self::SeriesUpdated { series } => {
                MediaEvent::SeriesUpdated { series }
            }
            Self::MediaDeleted { id } => MediaEvent::MediaDeleted { id },
        }
    }

    pub fn to_media_event(&self) -> MediaEvent {
        self.clone().into_media_event()
    }
}

#[derive(Debug, Clone)]
pub struct CatalogEventFrame {
    /// Durable catalog replay sequence. Live-only wake-up frames intentionally
    /// have no sequence and are not used to advance Last-Event-ID cursors.
    pub sequence: Option<u64>,
    pub emitted_at: Instant,
    pub event: CatalogEvent,
}

impl TryFrom<CatalogEventRecord> for CatalogEventFrame {
    type Error = MediaError;

    fn try_from(record: CatalogEventRecord) -> Result<Self, Self::Error> {
        let event = match record.kind {
            CatalogEventKind::MovieBatchFinalized => {
                let batch_id = record.movie_batch_id.ok_or_else(|| {
                    MediaError::Internal(format!(
                        "catalog event {} missing movie batch id",
                        record.sequence
                    ))
                })?;
                CatalogEvent::MovieBatchFinalized {
                    library_id: record.library_id,
                    batch_id,
                }
            }
            CatalogEventKind::SeriesBundleFinalized => {
                let series_id = record.entity_id.ok_or_else(|| {
                    MediaError::Internal(format!(
                        "catalog event {} missing series id",
                        record.sequence
                    ))
                })?;
                CatalogEvent::SeriesBundleFinalized {
                    library_id: record.library_id,
                    series_id: SeriesID(series_id),
                }
            }
        };

        Ok(Self {
            sequence: Some(record.sequence),
            emitted_at: Instant::now(),
            event,
        })
    }
}

#[derive(Debug)]
pub struct CatalogEventBus {
    tx: broadcast::Sender<CatalogEventFrame>,
}

impl CatalogEventBus {
    pub fn new(broadcast_capacity: usize) -> Self {
        let broadcast_capacity = broadcast_capacity.max(1);
        let (tx, _rx) = broadcast::channel(broadcast_capacity);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CatalogEventFrame> {
        self.tx.subscribe()
    }

    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
    }

    /// Publish a live-only wake-up frame that is not replayable.
    pub fn publish(&self, event: CatalogEvent) -> CatalogEventFrame {
        let frame = CatalogEventFrame {
            sequence: None,
            emitted_at: Instant::now(),
            event,
        };

        let _ = self.tx.send(frame.clone());
        frame
    }

    /// Bridge a committed durable outbox row to live subscribers.
    pub fn publish_record(
        &self,
        record: CatalogEventRecord,
    ) -> Result<CatalogEventFrame, MediaError> {
        let frame = CatalogEventFrame::try_from(record)?;
        let _ = self.tx.send(frame.clone());
        Ok(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::{CatalogEvent, CatalogEventBus};
    use chrono::Utc;
    use ferrex_core::{
        database::repository_ports::catalog_events::{
            CatalogEventKind, CatalogEventRecord,
        },
        types::{LibraryId, MediaID, MovieBatchId, MovieID, SeriesID},
    };
    use serde_json::json;
    use tokio::sync::broadcast::error::TryRecvError;
    use uuid::Uuid;

    fn movie_batch_record(sequence: u64) -> CatalogEventRecord {
        let library_id = LibraryId(Uuid::from_u128(1));
        let batch_id = MovieBatchId::new(2).unwrap();
        CatalogEventRecord {
            sequence,
            kind: CatalogEventKind::MovieBatchFinalized,
            library_id,
            entity_kind: "movie_batch".to_string(),
            entity_id: None,
            movie_batch_id: Some(batch_id),
            payload: json!({
                "library_id": library_id.to_uuid(),
                "batch_id": batch_id.as_u32(),
                "version": 1,
            }),
            idempotency_key: format!(
                "catalog:movie_batch_finalized:{library_id}:{batch_id}:1"
            ),
            occurred_at: Utc::now(),
        }
    }

    #[test]
    fn durable_catalog_records_reach_subscribers_with_replay_sequence() {
        let bus = CatalogEventBus::new(8);
        let mut receiver = bus.subscribe();

        let published = bus
            .publish_record(movie_batch_record(42))
            .expect("record should map to catalog frame");

        let received = receiver
            .try_recv()
            .expect("subscriber should receive catalog finalization");
        assert_eq!(published.sequence, Some(42));
        assert_eq!(received.sequence, Some(42));
        assert!(matches!(
            received.event,
            CatalogEvent::MovieBatchFinalized { .. }
        ));
    }

    #[test]
    fn live_catalog_wakeups_do_not_advance_replay_sequence() {
        let bus = CatalogEventBus::new(8);
        let library_id = LibraryId(Uuid::from_u128(1));
        let mut receiver = bus.subscribe();

        let published = bus.publish(CatalogEvent::MovieBatchFinalized {
            library_id,
            batch_id: MovieBatchId::new(2).unwrap(),
        });

        let received = receiver
            .try_recv()
            .expect("subscriber should receive live wake-up");
        assert_eq!(published.sequence, None);
        assert_eq!(received.sequence, None);
    }

    #[test]
    fn catalog_events_never_use_scan_sse_names() {
        let library_id = LibraryId(Uuid::from_u128(1));
        let events = [
            CatalogEvent::MovieBatchFinalized {
                library_id,
                batch_id: MovieBatchId::new(2).unwrap(),
            },
            CatalogEvent::SeriesBundleFinalized {
                library_id,
                series_id: SeriesID(Uuid::from_u128(3)),
            },
            CatalogEvent::MediaDeleted {
                id: MediaID::Movie(MovieID(Uuid::from_u128(4))),
            },
        ];

        for event in events {
            let name = event.sse_event_type().event_name();
            assert!(
                name.starts_with("media."),
                "catalog event unexpectedly used scan SSE name: {name}"
            );
        }
    }

    #[test]
    fn empty_catalog_bus_has_no_scan_frames() {
        let bus = CatalogEventBus::new(8);
        let mut receiver = bus.subscribe();

        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
    }
}
