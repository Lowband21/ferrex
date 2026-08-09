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
        let (sender, _) = broadcast::channel(capacity);
        let (scan_sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            scan_sender,
            job_channel_capacity: capacity,
            scan_channel_capacity: capacity,
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
}
