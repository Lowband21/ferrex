use async_trait::async_trait;
use std::fmt;
use tokio::sync::broadcast;

use crate::domain::scan::orchestration::events::{
    JobEvent, JobEventPublisher, ScanEvent, ScanEventPublisher,
};
use crate::error::Result;

/// Lightweight in-process event bus that fans out orchestrator notifications to
/// observers inside the runtime. This keeps the wiring flexible while we decide
/// how and when to plug in an external message broker.
pub struct InProcJobEventBus {
    sender: broadcast::Sender<JobEvent>,
    scan_sender: broadcast::Sender<ScanEvent>,
    job_channel_capacity: usize,
    scan_channel_capacity: usize,
}

impl fmt::Debug for InProcJobEventBus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InProcJobEventBus")
            .field("job_channel_capacity", &self.job_channel_capacity)
            .field("job_subscribers", &self.sender.receiver_count())
            .field("scan_channel_capacity", &self.scan_channel_capacity)
            .field("scan_subscribers", &self.scan_sender.receiver_count())
            .finish()
    }
}

impl InProcJobEventBus {
    pub fn new(capacity: usize) -> Self {
        Self::with_capacities(capacity, capacity)
    }

    /// Build the job lifecycle and scan-domain streams with independent
    /// capacities. Job lifecycle notifications are recoverable from the
    /// durable queue, while scan-domain events include live catalog
    /// projections and therefore need enough headroom for normal scan bursts.
    pub fn with_capacities(job_capacity: usize, scan_capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(job_capacity);
        let (scan_sender, _) = broadcast::channel(scan_capacity);
        Self {
            sender,
            scan_sender,
            job_channel_capacity: job_capacity,
            scan_channel_capacity: scan_capacity,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<JobEvent> {
        self.sender.subscribe()
    }

    pub fn subscribe_scan(&self) -> broadcast::Receiver<ScanEvent> {
        self.scan_sender.subscribe()
    }
}

#[async_trait]
impl JobEventPublisher for InProcJobEventBus {
    async fn publish(&self, event: JobEvent) -> Result<()> {
        let _ = self.sender.send(event);
        Ok(())
    }
}

#[async_trait]
impl ScanEventPublisher for InProcJobEventBus {
    async fn publish_scan_event(&self, event: ScanEvent) -> Result<()> {
        let _ = self.scan_sender.send(event);
        Ok(())
    }
}

pub trait JobEventStream {
    fn subscribe_jobs(&self) -> broadcast::Receiver<JobEvent>;
}

impl JobEventStream for InProcJobEventBus {
    fn subscribe_jobs(&self) -> broadcast::Receiver<JobEvent> {
        self.subscribe()
    }
}

// Stream trait for scan events so generic runtimes can subscribe without
// depending on the concrete InProcJobEventBus type.
pub trait ScanEventStream {
    fn subscribe_scan(&self) -> broadcast::Receiver<ScanEvent>;
}

impl ScanEventStream for InProcJobEventBus {
    fn subscribe_scan(&self) -> broadcast::Receiver<ScanEvent> {
        self.subscribe_scan()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::scan::orchestration::events::{EventMeta, JobEventPayload},
        types::LibraryId,
    };
    use chrono::Utc;

    #[tokio::test]
    async fn job_burst_above_256_reports_lag_to_reconciling_consumers() {
        let bus = InProcJobEventBus::new(256);
        let mut receiver = bus.subscribe();
        let library_id = LibraryId::new();

        for sequence in 0..300 {
            bus.publish(JobEvent {
                meta: EventMeta::new(
                    None,
                    library_id,
                    format!("burst:{sequence}"),
                    None,
                ),
                payload: JobEventPayload::ThroughputTick {
                    queue_depths: Vec::new(),
                    sampled_at: Utc::now(),
                },
            })
            .await
            .expect("event publish succeeds");
        }

        match receiver.recv().await {
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                assert!(skipped >= 44);
            }
            other => panic!("expected lag after 300 events, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn production_sized_job_stream_buffers_bursts_above_256() {
        let bus = InProcJobEventBus::with_capacities(32_768, 8192);
        let mut receiver = bus.subscribe();
        let library_id = LibraryId::new();

        for sequence in 0..300 {
            bus.publish(JobEvent {
                meta: EventMeta::new(
                    None,
                    library_id,
                    format!("burst:{sequence}"),
                    None,
                ),
                payload: JobEventPayload::ThroughputTick {
                    queue_depths: Vec::new(),
                    sampled_at: Utc::now(),
                },
            })
            .await
            .expect("event publish succeeds");
        }

        for expected in 0..300 {
            let event = receiver
                .recv()
                .await
                .expect("production-sized job stream retains the burst");
            assert_eq!(event.meta.idempotency_key, format!("burst:{expected}"));
        }
    }

    #[tokio::test]
    async fn scan_stream_capacity_is_independent_from_job_reconciliation_stream()
     {
        let bus = InProcJobEventBus::with_capacities(8, 512);
        let mut job_receiver = bus.subscribe();
        let mut scan_receiver = bus.subscribe_scan();
        let library_id = LibraryId::new();

        for sequence in 0..300 {
            bus.publish(JobEvent {
                meta: EventMeta::new(
                    None,
                    library_id,
                    format!("burst:{sequence}"),
                    None,
                ),
                payload: JobEventPayload::ThroughputTick {
                    queue_depths: Vec::new(),
                    sampled_at: Utc::now(),
                },
            })
            .await
            .expect("job event publish succeeds");

            bus.publish_scan_event(ScanEvent::SeedCompleted(
                crate::domain::scan::orchestration::events::ScanSeedSummary {
                    library_id,
                    correlation_id: None,
                    mode: crate::domain::scan::orchestration::events::ScanSeedMode::Bulk,
                    queued_folders: sequence,
                    enrolled_job_ids: Vec::new(),
                    completed_at: Utc::now(),
                },
            ))
            .await
            .expect("scan event publish succeeds");
        }

        assert!(matches!(
            job_receiver.recv().await,
            Err(broadcast::error::RecvError::Lagged(_))
        ));

        for expected in 0..300 {
            let event = scan_receiver
                .recv()
                .await
                .expect("scan-domain burst remains buffered");
            let ScanEvent::SeedCompleted(summary) = event else {
                panic!("expected seed-completed scan event");
            };
            assert_eq!(summary.queued_folders, expected);
        }
    }
}
