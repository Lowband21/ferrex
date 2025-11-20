use async_trait::async_trait;
use std::collections::HashMap;
use uuid::Uuid;

use crate::error::Result;

/// Lightweight watch metrics used by ranking/sorting.
#[derive(Debug, Clone, Copy)]
pub struct ProgressEntry {
    pub ratio: f32,
    pub last_watched: i64,
}

/// Read-only port for bulk watch metrics needed by `indices` and other
/// ranking modules. This is separated from the mutable watch status port
/// to allow independent implementation and caching strategies.
#[async_trait]
pub trait WatchMetricsReadPort: Send + Sync {
    async fn load_progress_map(&self, user_id: Uuid) -> Result<HashMap<Uuid, ProgressEntry>>;

    async fn load_completed_map(&self, user_id: Uuid) -> Result<HashMap<Uuid, i64>>;
}
