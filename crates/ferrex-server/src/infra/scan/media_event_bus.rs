use std::{
    collections::VecDeque,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use ferrex_core::types::MediaEvent;
use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub struct MediaEventFrame {
    pub sequence: u64,
    pub emitted_at: Instant,
    pub event: MediaEvent,
}

#[derive(Debug)]
pub struct MediaEventBus {
    tx: broadcast::Sender<MediaEventFrame>,
    history: Mutex<VecDeque<MediaEventFrame>>,
    history_capacity: usize,
    sequence: AtomicU64,
}

impl MediaEventBus {
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

    pub fn subscribe(&self) -> broadcast::Receiver<MediaEventFrame> {
        self.tx.subscribe()
    }

    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
    }

    pub fn publish(&self, event: MediaEvent) -> MediaEventFrame {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let frame = MediaEventFrame {
            sequence,
            emitted_at: Instant::now(),
            event,
        };

        if Self::should_record_history(&frame.event) {
            let mut guard = self
                .history
                .lock()
                .expect("media event history mutex poisoned");
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
    ) -> Vec<MediaEventFrame> {
        let guard = self
            .history
            .lock()
            .expect("media event history mutex poisoned");
        guard
            .iter()
            .filter(|frame| frame.sequence > sequence)
            .cloned()
            .collect()
    }

    pub fn history_since_instant(
        &self,
        since: Instant,
    ) -> Vec<MediaEventFrame> {
        let guard = self
            .history
            .lock()
            .expect("media event history mutex poisoned");
        guard
            .iter()
            .filter(|frame| frame.emitted_at >= since)
            .cloned()
            .collect()
    }

    fn should_record_history(event: &MediaEvent) -> bool {
        matches!(
            event,
            MediaEvent::MovieBatchFinalized { .. }
                | MediaEvent::SeriesBundleFinalized { .. }
                | MediaEvent::MediaDeleted { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::MediaEventBus;
    use ferrex_core::types::{
        LibraryId, MediaEvent, MovieBatchId, ScanEventMetadata, SeriesID,
    };
    use std::time::Duration;
    use uuid::Uuid;

    #[test]
    fn records_only_selected_events_in_history() {
        let bus = MediaEventBus::new(8, 8);
        let library_id = LibraryId(Uuid::from_u128(1));

        bus.publish(MediaEvent::MovieBatchFinalized {
            library_id,
            batch_id: MovieBatchId(2),
        });
        bus.publish(MediaEvent::SeriesBundleFinalized {
            library_id,
            series_id: SeriesID(Uuid::from_u128(3)),
        });
        bus.publish(MediaEvent::ScanCompleted {
            scan_id: Uuid::from_u128(99),
            metadata: ScanEventMetadata {
                version: "1".to_string(),
                correlation_id: Uuid::from_u128(100),
                idempotency_key: "noop".to_string(),
                library_id,
            },
        });

        let history = bus.history_since_sequence(0);
        assert_eq!(history.len(), 2);
        assert!(matches!(
            history[0].event,
            MediaEvent::MovieBatchFinalized { .. }
        ));
        assert!(matches!(
            history[1].event,
            MediaEvent::SeriesBundleFinalized { .. }
        ));
    }

    #[test]
    fn history_since_instant_filters_frames() {
        let bus = MediaEventBus::new(8, 8);
        let library_id = LibraryId(Uuid::from_u128(1));

        bus.publish(MediaEvent::SeriesBundleFinalized {
            library_id,
            series_id: SeriesID(Uuid::from_u128(3)),
        });
        let cutoff = std::time::Instant::now();
        std::thread::sleep(Duration::from_millis(5));
        bus.publish(MediaEvent::SeriesBundleFinalized {
            library_id,
            series_id: SeriesID(Uuid::from_u128(4)),
        });

        let history = bus.history_since_instant(cutoff);
        assert_eq!(history.len(), 1);
        assert!(matches!(
            history[0].event,
            MediaEvent::SeriesBundleFinalized { .. }
        ));
    }
}
