use std::{collections::HashMap, sync::Arc};

use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

use super::job::JobId;

#[derive(Clone, Default, Debug)]
pub struct CorrelationCache {
    inner: Arc<Mutex<HashMap<JobId, Uuid>>>,
}

impl CorrelationCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn remember(&self, job_id: JobId, correlation_id: Uuid) {
        let mut guard = self.inner.lock().await;
        guard.insert(job_id, correlation_id);
    }

    pub async fn remember_if_absent(
        &self,
        job_id: JobId,
        correlation_id: Uuid,
    ) {
        let mut guard = self.inner.lock().await;
        guard.entry(job_id).or_insert(correlation_id);
    }

    pub async fn fetch(&self, job_id: &JobId) -> Option<Uuid> {
        let guard = self.inner.lock().await;
        guard.get(job_id).copied()
    }

    pub async fn take(&self, job_id: &JobId) -> Option<Uuid> {
        let mut guard = self.inner.lock().await;
        guard.remove(job_id)
    }

    pub async fn fetch_or_generate(&self, job_id: JobId) -> Uuid {
        self.fetch_persisted_or_generate(job_id, None).await
    }

    pub async fn fetch_persisted_or_generate(
        &self,
        job_id: JobId,
        persisted: Option<Uuid>,
    ) -> Uuid {
        let mut guard = self.inner.lock().await;
        if let Some(existing) = guard.get(&job_id) {
            return *existing;
        }

        if let Some(correlation_id) = persisted {
            guard.insert(job_id, correlation_id);
            return correlation_id;
        }

        let fresh = Uuid::now_v7();
        info!(job_id = %job_id.0, "missing correlation id; generating new one");
        guard.insert(job_id, fresh);
        fresh
    }

    pub async fn take_or_generate(&self, job_id: JobId) -> Uuid {
        self.take_persisted_or_generate(job_id, None).await
    }

    pub async fn take_persisted_or_generate(
        &self,
        job_id: JobId,
        persisted: Option<Uuid>,
    ) -> Uuid {
        let mut guard = self.inner.lock().await;
        if let Some(existing) = guard.remove(&job_id) {
            return existing;
        }

        if let Some(correlation_id) = persisted {
            return correlation_id;
        }

        let fresh = Uuid::now_v7();
        warn!(job_id = %job_id.0, "missing correlation id during cleanup; generating new one");
        fresh
    }
}
