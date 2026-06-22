use chrono::{DateTime, Utc};
use ferrex_model::{MediaID, scan::scanner::settings};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Ready-queue depths for scan-related workers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanQueueDepths {
    pub folder_scan: usize,
    pub analyze: usize,
    pub metadata: usize,
    pub index: usize,
    pub image_fetch: usize,
    #[serde(default)]
    pub transcript_extract: usize,
}

/// Safe recent transcript extraction failure row for admin surfaces. This view
/// exposes IDs, bounded error excerpts, and retry timing only; it never includes
/// transcript text or local filesystem paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptRecentFailureView {
    pub library_id: ferrex_model::LibraryId,
    pub media_id: MediaID,
    pub media_file_id: Uuid,
    pub status: String,
    pub source_count: i32,
    pub segment_count: i32,
    pub attempt_count: i32,
    pub last_error_excerpt: Option<String>,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub last_run_correlation_id: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
}

/// Bounded transcript extraction status included in scan metrics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TranscriptScanStatusView {
    pub queue_depth: usize,
    #[serde(default)]
    pub recent_failures: Vec<TranscriptRecentFailureView>,
}

/// Top-level scanner metrics for admin surfaces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanMetrics {
    pub queue_depths: ScanQueueDepths,
    pub active_scans: usize,
    #[serde(default)]
    pub incremental: IncrementalScanStatusView,
    #[serde(default)]
    pub transcripts: TranscriptScanStatusView,
}

/// Minimal, feature-agnostic view of orchestrator configuration for admin surfaces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanConfig {
    pub orchestrator: OrchestratorConfigView,
    #[serde(default)]
    pub incremental_policy: IncrementalScanPolicyView,
}

/// Effective incremental-scanning policy exposed to operators.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncrementalScanPolicyView {
    pub default_auto_scan: bool,
    pub default_watch_for_changes: bool,
    pub default_scan_interval_minutes: u32,
    pub watch_strategy: String,
    pub poll_interval_ms: u64,
    pub debounce_window_ms: u64,
    pub max_batch_events: usize,
    pub maintenance_enabled: bool,
    pub maintenance_tick_interval_ms: u64,
    pub maintenance_max_jobs_per_library: usize,
    pub maintenance_max_root_entries_per_library: usize,
    pub media_extensions: Vec<String>,
    pub ignored_extensions: Vec<String>,
    pub ignored_path_patterns: Vec<String>,
}

impl Default for IncrementalScanPolicyView {
    fn default() -> Self {
        Self {
            default_auto_scan: true,
            default_watch_for_changes: true,
            default_scan_interval_minutes: 60,
            watch_strategy: "auto".to_string(),
            poll_interval_ms: 30_000,
            debounce_window_ms: 250,
            max_batch_events: 8192,
            maintenance_enabled: true,
            maintenance_tick_interval_ms: 60_000,
            maintenance_max_jobs_per_library: 128,
            maintenance_max_root_entries_per_library: 512,
            media_extensions: settings::default_video_file_extensions_vec(),
            ignored_extensions: Vec::new(),
            ignored_path_patterns: Vec::new(),
        }
    }
}

/// Runtime health counters for incremental scan infrastructure.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IncrementalScanStatusView {
    pub enabled_libraries: usize,
    pub auto_scan_libraries: usize,
    pub watch_enabled_libraries: usize,
    pub registered_watch_libraries: usize,
    pub active_watch_libraries: usize,
    pub initializing_watch_libraries: usize,
    pub registered_watch_roots: usize,
    pub active_watch_roots: usize,
    pub watcher_error_count: u64,
    pub last_watcher_error: Option<String>,
    pub replay_pending_events: u64,
    pub replay_lag_ms: Option<u64>,
    pub overflow_events: u64,
    pub stale_cursor_libraries: u64,
    pub stale_cursors: u64,
    pub oldest_cursor_staleness_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorConfigView {
    pub queue: QueueConfigView,
    pub retry: RetryConfigView,
    pub metadata_limits: MetadataLimitsView,
    #[serde(default)]
    pub transcript_indexing: TranscriptIndexingConfigView,
    pub bulk_mode: BulkModeView,
    pub maintenance: MaintenanceConfigView,
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
    #[serde(default)]
    pub max_parallel_transcript_extract: usize,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TranscriptIndexingConfigView {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkModeView {
    pub speedup_factor: f32,
    pub maintenance_partition_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceConfigView {
    pub enabled: bool,
    pub tick_interval_ms: u64,
    pub max_jobs_per_library: usize,
    pub max_root_entries_per_library: usize,
    pub error_backoff_ms: u64,
    pub run_stall_timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseConfigView {
    pub lease_ttl_secs: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchConfigView {
    pub debounce_window_ms: u64,
    pub max_batch_events: usize,
    pub strategy: String,
    pub poll_interval_ms: u64,
    pub poll_backoff_max_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfigView {
    pub library_scan_limit: usize,
    #[serde(default)]
    pub media_analysis_limit: usize,
    #[serde(default)]
    pub metadata_limit: usize,
    #[serde(default)]
    pub indexing_limit: usize,
    #[serde(default)]
    pub image_fetch_limit: usize,
    #[serde(default)]
    pub transcript_extraction_limit: usize,
}
