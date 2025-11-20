use async_trait::async_trait;
use std::fmt;
use tokio::sync::broadcast;

use crate::Result;
use crate::orchestration::events::{
    DomainEvent, DomainEventPublisher, JobEvent, JobEventPublisher,
};

/// Lightweight in-process event bus that fans out orchestrator notifications to
/// observers inside the runtime. This keeps the wiring flexible while we decide
/// how and when to plug in an external message broker.
pub struct InProcJobEventBus {
    sender: broadcast::Sender<JobEvent>,
    domain_sender: broadcast::Sender<DomainEvent>,
    job_channel_capacity: usize,
    domain_channel_capacity: usize,
}

impl fmt::Debug for InProcJobEventBus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InProcJobEventBus")
            .field("job_channel_capacity", &self.job_channel_capacity)
            .field("job_subscribers", &self.sender.receiver_count())
            .field("domain_channel_capacity", &self.domain_channel_capacity)
            .field("domain_subscribers", &self.domain_sender.receiver_count())
            .finish()
    }
}

impl InProcJobEventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        let (domain_sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            domain_sender,
            job_channel_capacity: capacity,
            domain_channel_capacity: capacity,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<JobEvent> {
        self.sender.subscribe()
    }

    pub fn subscribe_domain(&self) -> broadcast::Receiver<DomainEvent> {
        self.domain_sender.subscribe()
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
impl DomainEventPublisher for InProcJobEventBus {
    async fn publish_domain(&self, event: DomainEvent) -> Result<()> {
        let _ = self.domain_sender.send(event);
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

// Stream trait for domain events so generic runtimes can subscribe without
// depending on the concrete InProcJobEventBus type.
pub trait DomainEventStream {
    fn subscribe_domain(&self) -> broadcast::Receiver<DomainEvent>;
}

impl DomainEventStream for InProcJobEventBus {
    fn subscribe_domain(&self) -> broadcast::Receiver<DomainEvent> {
        self.subscribe_domain()
    }
}
