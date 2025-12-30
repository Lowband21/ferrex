use serde::{Deserialize, Serialize};

/// Ready-queue depths for scan-related workers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanQueueDepths {
    pub folder_scan: usize,
    pub analyze: usize,
    pub metadata: usize,
    pub index: usize,
    pub image_fetch: usize,
}

/// Top-level scanner metrics for admin surfaces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanMetrics {
    pub queue_depths: ScanQueueDepths,
    pub active_scans: usize,
}

/// Minimal, feature-agnostic view of orchestrator configuration for admin surfaces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanConfig {
    pub orchestrator: OrchestratorConfigView,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorConfigView {
    pub queue: QueueConfigView,
    pub retry: RetryConfigView,
    pub metadata_limits: MetadataLimitsView,
    pub bulk_mode: BulkModeView,
    pub lease: LeaseConfigView,
    pub watch: WatchConfigView,
    pub budget: BudgetConfigView,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueConfigView {
    pub max_parallel_scans: usize,
    pub max_parallel_series_resolve: usize,
    pub max_parallel_analyses: usize,
    pub max_parallel_metadata: usize,
    pub max_parallel_index: usize,
    pub max_parallel_image_fetch: usize,
    pub max_parallel_scans_per_device: usize,
    pub default_library_cap: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfigView {
    pub max_attempts: u16,
    pub backoff_base_ms: u64,
    pub backoff_max_ms: u64,
    pub fast_retry_attempts: u16,
    pub fast_retry_factor: f32,
    pub heavy_library_attempt_threshold: u16,
    pub heavy_library_slowdown_factor: f32,
    pub jitter_ratio: f32,
    pub jitter_min_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataLimitsView {
    pub max_concurrency: usize,
    pub max_qps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkModeView {
    pub speedup_factor: f32,
    pub maintenance_partition_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseConfigView {
    pub lease_ttl_secs: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchConfigView {
    pub debounce_window_ms: u64,
    pub max_batch_events: usize,
    pub poll_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfigView {
    pub library_scan_limit: usize,
}
