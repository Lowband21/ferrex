use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::types::ids::LibraryId;

/// Global knobs that tune orchestrator behaviour.
///
/// All fields carry defaults so existing deployments can progressively adopt
/// new scheduling features without supplying a full configuration payload.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct OrchestratorConfig {
    /// Queue sizing, fairness weights, and per-library overrides.
    pub queue: QueueConfig,
    /// Priority weights used by the scheduler when rotating buckets.
    pub priority_weights: PriorityWeights,
    /// Retry/backoff policy shared by all workers.
    pub retry: RetryConfig,
    /// Limits for metadata enrichment workers.
    pub metadata_limits: MetadataLimits,
    /// Bulk maintenance tuning settings.
    pub bulk_mode: BulkModeTuning,
    /// Lease defaults (TTL, renewal thresholds, housekeeping cadence).
    pub lease: LeaseConfig,
    /// Global concurrency budget configuration for actor workloads.
    pub budget: super::budget::BudgetConfig,
    /// Filesystem watch debounce and batching configuration.
    pub watch: WatchConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueueConfig {
    /// Maximum worker concurrency per queue. These values drive worker pool sizes.
    pub max_parallel_scans: usize,
    pub max_parallel_analyses: usize,
    pub max_parallel_metadata: usize,
    pub max_parallel_index: usize,
    pub max_parallel_image_fetch: usize,
    /// Per-device cap for scan workers touching the same mount.
    pub max_parallel_scans_per_device: usize,
    /// High watermark for queued jobs. Beyond this we start coalescing low priority work.
    pub high_watermark: usize,
    /// Critical watermark for queued jobs. Beyond this P2/P3 work is merged instead of enqueued.
    pub critical_watermark: usize,
    /// Sliding window (milliseconds) for aggregating duplicate work items.
    pub coalesce_window_ms: u64,
    /// Default maximum in-flight leases allowed per library.
    pub default_library_cap: usize,
    /// Default scheduling weight assigned to libraries without overrides.
    pub default_library_weight: u32,
    /// Optional per-library overrides.
    #[serde(default)]
    pub library_overrides: HashMap<LibraryId, LibraryQueuePolicy>,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            max_parallel_scans: 6,
            max_parallel_analyses: 4,
            max_parallel_metadata: 4,
            max_parallel_index: 1,
            max_parallel_image_fetch: 8,
            max_parallel_scans_per_device: 16,
            high_watermark: 10_000,
            critical_watermark: 20_000,
            coalesce_window_ms: 100,
            default_library_cap: 32,
            default_library_weight: 1,
            library_overrides: HashMap::new(),
        }
    }
}

/// Library-specific overrides for queue fairness.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct LibraryQueuePolicy {
    /// Optional in-flight cap; falls back to `default_library_cap` when missing.
    pub max_inflight: Option<usize>,
    /// Optional scheduling weight multiplier; falls back to `default_library_weight`.
    pub weight: Option<u32>,
}

/// Lease/heartbeat tuning for worker tasks.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct LeaseConfig {
    /// Default TTL for job leases (seconds)
    pub lease_ttl_secs: i64,
    /// Renew when remaining TTL drops below this fraction of the original TTL (e.g. 0.5)
    pub renew_at_fraction: f32,
    /// Minimum margin before expiry to trigger a renewal regardless of fraction (ms)
    pub renew_min_margin_ms: u64,
    /// Housekeeping cadence for scanning expired leases (ms)
    pub housekeeper_interval_ms: u64,
}

impl Default for LeaseConfig {
    fn default() -> Self {
        Self {
            lease_ttl_secs: 30,
            renew_at_fraction: 0.5,
            renew_min_margin_ms: 2_000,
            housekeeper_interval_ms: 15_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PriorityWeights {
    pub p0: u8,
    pub p1: u8,
    pub p2: u8,
    pub p3: u8,
}

impl Default for PriorityWeights {
    fn default() -> Self {
        Self {
            p0: 8,
            p1: 4,
            p2: 2,
            p3: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max_attempts: u16,
    pub backoff_base_ms: u64,
    pub backoff_max_ms: u64,
    /// Attempts that should receive the "fast retry" treatment for user-visible scans.
    pub fast_retry_attempts: u16,
    /// Multiplier applied to base delay while in the fast retry window.
    pub fast_retry_factor: f32,
    /// When a library accumulates this many retry-heavy jobs we slow the whole queue.
    pub heavy_library_attempt_threshold: u16,
    /// Slowdown multiplier applied when a library is considered under stress.
    pub heavy_library_slowdown_factor: f32,
    /// Percentage-based jitter to spread out retries.
    pub jitter_ratio: f32,
    /// Minimum jitter in milliseconds so tiny jobs still randomise a bit.
    pub jitter_min_ms: u64,
}

impl RetryConfig {
    pub fn backoff_base(&self) -> core::time::Duration {
        core::time::Duration::from_millis(self.backoff_base_ms)
    }

    pub fn backoff_max(&self) -> core::time::Duration {
        core::time::Duration::from_millis(self.backoff_max_ms)
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            backoff_base_ms: 2_000,
            backoff_max_ms: 5 * 60 * 1_000,
            fast_retry_attempts: 2,
            fast_retry_factor: 0.35,
            heavy_library_attempt_threshold: 4,
            heavy_library_slowdown_factor: 1.8,
            jitter_ratio: 0.25,
            jitter_min_ms: 250,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct MetadataLimits {
    pub max_concurrency: usize,
    pub max_qps: u32,
}

impl Default for MetadataLimits {
    fn default() -> Self {
        Self {
            max_concurrency: 4,
            max_qps: 100,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BulkModeTuning {
    pub speedup_factor: f32,
    pub maintenance_partition_count: usize,
}

impl Default for BulkModeTuning {
    fn default() -> Self {
        Self {
            speedup_factor: 1.2,
            maintenance_partition_count: 8,
        }
    }
}

/// Tuning controls for filesystem watch coalescing.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WatchConfig {
    /// Debounce window in milliseconds.
    pub debounce_window_ms: u64,
    /// Maximum number of events to flush in a single batch.
    pub max_batch_events: usize,
    /// Polling cadence in milliseconds for filesystems without native watchers.
    #[serde(default = "WatchConfig::default_poll_interval_ms")]
    pub poll_interval_ms: u64,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            debounce_window_ms: 250,
            max_batch_events: 1024,
            poll_interval_ms: Self::default_poll_interval_ms(),
        }
    }
}

impl WatchConfig {
    const fn default_poll_interval_ms() -> u64 {
        30_000
    }
}
