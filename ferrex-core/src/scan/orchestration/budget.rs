//! Budget management for controlling concurrent work across the orchestrator.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{fmt, sync::Arc};

use crate::error::Result;
use crate::types::ids::LibraryId;

/// Different types of workloads that can be budget-limited
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum WorkloadType {
    LibraryScan,
    MediaAnalysis,
    MetadataEnrichment,
    Indexing,
    ImageFetch,
}

/// A token representing acquired budget for a specific workload
#[derive(Debug)]
pub struct BudgetToken {
    pub workload: WorkloadType,
    pub library_id: LibraryId,
    pub acquired_at: chrono::DateTime<chrono::Utc>,
}

/// Configuration for workload limits
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BudgetConfig {
    pub library_scan_limit: usize, // Default 1 - one library scan at a time
    pub media_analysis_limit: usize, // Default low to avoid disk overload
    pub metadata_limit: usize,     // Default 2 * CPU count
    pub indexing_limit: usize,     // Default moderate
    pub image_fetch_limit: usize,  // Poster/backdrop workers
}

impl Default for BudgetConfig {
    fn default() -> Self {
        let cpu_count = num_cpus::get();
        Self {
            library_scan_limit: 1,
            media_analysis_limit: 4,
            metadata_limit: cpu_count * 2,
            indexing_limit: cpu_count,
            image_fetch_limit: 4,
        }
    }
}

/// Trait for managing workload budgets across the orchestrator
#[async_trait]
pub trait WorkloadBudget: Send + Sync {
    /// Try to acquire a budget token for the given workload
    async fn try_acquire(
        &self,
        workload: WorkloadType,
        library_id: LibraryId,
    ) -> Result<Option<Arc<BudgetToken>>>;

    /// Acquire a budget token, waiting if necessary
    async fn acquire(
        &self,
        workload: WorkloadType,
        library_id: LibraryId,
    ) -> Result<Arc<BudgetToken>>;

    /// Release a budget token back to the pool
    async fn release(&self, token: Arc<BudgetToken>) -> Result<()>;

    /// Get current budget utilization for a workload type
    async fn utilization(
        &self,
        workload: WorkloadType,
    ) -> Result<(usize, usize)>;

    /// Check if budget is available without acquiring
    async fn has_budget(&self, workload: WorkloadType) -> Result<bool>;
}

/// Default in-memory implementation of WorkloadBudget
pub struct InMemoryBudget {
    config: BudgetConfig,
    state: Arc<tokio::sync::Mutex<BudgetState>>,
}

impl fmt::Debug for InMemoryBudget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("InMemoryBudget");
        debug.field("config", &self.config);

        match self.state.try_lock() {
            Ok(state) => {
                debug
                    .field("library_scans", &state.library_scans)
                    .field("media_analyses", &state.media_analyses)
                    .field("metadata_jobs", &state.metadata_jobs)
                    .field("indexing_jobs", &state.indexing_jobs)
                    .field("image_jobs", &state.image_jobs);
            }
            Err(_) => {
                debug.field("state", &"<locked>");
            }
        }

        debug.finish()
    }
}

#[derive(Debug, Default)]
struct BudgetState {
    library_scans: usize,
    media_analyses: usize,
    metadata_jobs: usize,
    indexing_jobs: usize,
    image_jobs: usize,
}

impl InMemoryBudget {
    pub fn new(config: BudgetConfig) -> Self {
        Self {
            config,
            state: Arc::new(tokio::sync::Mutex::new(BudgetState::default())),
        }
    }
}

#[async_trait]
impl WorkloadBudget for InMemoryBudget {
    async fn try_acquire(
        &self,
        workload: WorkloadType,
        library_id: LibraryId,
    ) -> Result<Option<Arc<BudgetToken>>> {
        let mut state = self.state.lock().await;

        let (current, limit) = match workload {
            WorkloadType::LibraryScan => {
                (&mut state.library_scans, self.config.library_scan_limit)
            }
            WorkloadType::MediaAnalysis => {
                (&mut state.media_analyses, self.config.media_analysis_limit)
            }
            WorkloadType::MetadataEnrichment => {
                (&mut state.metadata_jobs, self.config.metadata_limit)
            }
            WorkloadType::Indexing => {
                (&mut state.indexing_jobs, self.config.indexing_limit)
            }
            WorkloadType::ImageFetch => {
                (&mut state.image_jobs, self.config.image_fetch_limit)
            }
        };

        if *current < limit {
            *current += 1;
            Ok(Some(Arc::new(BudgetToken {
                workload,
                library_id,
                acquired_at: chrono::Utc::now(),
            })))
        } else {
            Ok(None)
        }
    }

    async fn acquire(
        &self,
        workload: WorkloadType,
        library_id: LibraryId,
    ) -> Result<Arc<BudgetToken>> {
        // For now, just keep trying with a small delay
        loop {
            if let Some(token) = self.try_acquire(workload, library_id).await? {
                return Ok(token);
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    async fn release(&self, token: Arc<BudgetToken>) -> Result<()> {
        let mut state = self.state.lock().await;

        match token.workload {
            WorkloadType::LibraryScan => {
                state.library_scans = state.library_scans.saturating_sub(1)
            }
            WorkloadType::MediaAnalysis => {
                state.media_analyses = state.media_analyses.saturating_sub(1)
            }
            WorkloadType::MetadataEnrichment => {
                state.metadata_jobs = state.metadata_jobs.saturating_sub(1)
            }
            WorkloadType::Indexing => {
                state.indexing_jobs = state.indexing_jobs.saturating_sub(1)
            }
            WorkloadType::ImageFetch => {
                state.image_jobs = state.image_jobs.saturating_sub(1)
            }
        }

        Ok(())
    }

    async fn utilization(
        &self,
        workload: WorkloadType,
    ) -> Result<(usize, usize)> {
        let state = self.state.lock().await;

        let (current, limit) = match workload {
            WorkloadType::LibraryScan => {
                (state.library_scans, self.config.library_scan_limit)
            }
            WorkloadType::MediaAnalysis => {
                (state.media_analyses, self.config.media_analysis_limit)
            }
            WorkloadType::MetadataEnrichment => {
                (state.metadata_jobs, self.config.metadata_limit)
            }
            WorkloadType::Indexing => {
                (state.indexing_jobs, self.config.indexing_limit)
            }
            WorkloadType::ImageFetch => {
                (state.image_jobs, self.config.image_fetch_limit)
            }
        };

        Ok((current, limit))
    }

    async fn has_budget(&self, workload: WorkloadType) -> Result<bool> {
        let (current, limit) = self.utilization(workload).await?;
        Ok(current < limit)
    }
}
