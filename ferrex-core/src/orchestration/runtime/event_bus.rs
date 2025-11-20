use async_trait::async_trait;
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
}

impl InProcJobEventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        let (domain_sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            domain_sender,
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
