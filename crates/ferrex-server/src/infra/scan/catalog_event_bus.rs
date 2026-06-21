use std::{
    collections::VecDeque,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use ferrex_core::types::{
    LibraryId, MediaEvent, MediaID, MovieBatchId, MovieReference, Series,
    SeriesID, events::MediaSseEventType,
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

    fn should_record_history(&self) -> bool {
        matches!(
            self,
            Self::MovieBatchFinalized { .. }
                | Self::SeriesBundleFinalized { .. }
                | Self::MediaDeleted { .. }
        )
    }
}

#[derive(Debug, Clone)]
pub struct CatalogEventFrame {
    pub sequence: u64,
    pub emitted_at: Instant,
    pub event: CatalogEvent,
}

#[derive(Debug)]
pub struct CatalogEventBus {
    tx: broadcast::Sender<CatalogEventFrame>,
    history: Mutex<VecDeque<CatalogEventFrame>>,
    history_capacity: usize,
    sequence: AtomicU64,
}

impl CatalogEventBus {
    pub fn new(history_capacity: usize, broadcast_capacity: usize) -> Self {
        let history_capacity = history_capacity.max(1);
        let broadcast_capacity = broadcast_capacity.max(1);
        let (tx, _rx) = broadcast::channel(broadcast_capacity);
        Self {
            tx,
            history: Mutex::new(VecDeque::with_capacity(history_capacity)),
            history_capacity,
            sequence: AtomicU64::new(0),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CatalogEventFrame> {
        self.tx.subscribe()
    }

    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
    }

    pub fn publish(&self, event: CatalogEvent) -> CatalogEventFrame {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let frame = CatalogEventFrame {
            sequence,
            emitted_at: Instant::now(),
            event,
        };

        if frame.event.should_record_history() {
            let mut guard = self
                .history
                .lock()
                .expect("catalog event history mutex poisoned");
            if guard.len() == self.history_capacity {
                guard.pop_front();
            }
            guard.push_back(frame.clone());
        }

        let _ = self.tx.send(frame.clone());
        frame
    }

    pub fn history_since_sequence(
        &self,
        sequence: u64,
    ) -> Vec<CatalogEventFrame> {
        let guard = self
            .history
            .lock()
            .expect("catalog event history mutex poisoned");
        guard
            .iter()
            .filter(|frame| frame.sequence > sequence)
            .cloned()
            .collect()
    }

    pub fn history_since_instant(
        &self,
        since: Instant,
    ) -> Vec<CatalogEventFrame> {
        let guard = self
            .history
            .lock()
            .expect("catalog event history mutex poisoned");
        guard
            .iter()
            .filter(|frame| frame.emitted_at >= since)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{CatalogEvent, CatalogEventBus};
    use ferrex_core::types::{
        LibraryId, MediaID, MovieBatchId, MovieID, SeriesID,
    };
    use std::time::Duration;
    use tokio::sync::broadcast::error::TryRecvError;
    use uuid::Uuid;

    #[test]
    fn records_only_catalog_invalidation_events_in_history() {
        let bus = CatalogEventBus::new(8, 8);
        let library_id = LibraryId(Uuid::from_u128(1));

        bus.publish(CatalogEvent::MovieBatchFinalized {
            library_id,
            batch_id: MovieBatchId(2),
        });
        bus.publish(CatalogEvent::SeriesBundleFinalized {
            library_id,
            series_id: SeriesID(Uuid::from_u128(3)),
        });
        bus.publish(CatalogEvent::MediaDeleted {
            id: MediaID::Movie(MovieID(Uuid::from_u128(4))),
        });

        let history = bus.history_since_sequence(0);
        assert_eq!(history.len(), 3);
        assert!(matches!(
            history[0].event,
            CatalogEvent::MovieBatchFinalized { .. }
        ));
        assert!(matches!(
            history[1].event,
            CatalogEvent::SeriesBundleFinalized { .. }
        ));
        assert!(matches!(
            history[2].event,
            CatalogEvent::MediaDeleted { .. }
        ));
    }

    #[test]
    fn catalog_finalization_events_reach_subscribers() {
        let bus = CatalogEventBus::new(8, 8);
        let library_id = LibraryId(Uuid::from_u128(1));
        let mut receiver = bus.subscribe();

        let published = bus.publish(CatalogEvent::MovieBatchFinalized {
            library_id,
            batch_id: MovieBatchId(2),
        });

        let received = receiver
            .try_recv()
            .expect("subscriber should receive catalog finalization");
        assert_eq!(received.sequence, published.sequence);
        assert!(matches!(
            received.event,
            CatalogEvent::MovieBatchFinalized { .. }
        ));
    }

    #[test]
    fn catalog_events_never_use_scan_sse_names() {
        let library_id = LibraryId(Uuid::from_u128(1));
        let events = [
            CatalogEvent::MovieBatchFinalized {
                library_id,
                batch_id: MovieBatchId(2),
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
    fn history_since_instant_filters_frames() {
        let bus = CatalogEventBus::new(8, 8);
        let library_id = LibraryId(Uuid::from_u128(1));

        bus.publish(CatalogEvent::SeriesBundleFinalized {
            library_id,
            series_id: SeriesID(Uuid::from_u128(3)),
        });
        let cutoff = std::time::Instant::now();
        std::thread::sleep(Duration::from_millis(5));
        bus.publish(CatalogEvent::SeriesBundleFinalized {
            library_id,
            series_id: SeriesID(Uuid::from_u128(4)),
        });

        let history = bus.history_since_instant(cutoff);
        assert_eq!(history.len(), 1);
        assert!(matches!(
            history[0].event,
            CatalogEvent::SeriesBundleFinalized { .. }
        ));
    }

    #[test]
    fn empty_catalog_bus_has_no_scan_frames() {
        let bus = CatalogEventBus::new(8, 8);
        let mut receiver = bus.subscribe();

        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
    }
}
