use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy)]
pub struct PlayerDiskImageCacheStatsSnapshot {
    pub touch_attempts: u64,
    pub touch_updates: u64,
    pub access_index_flushes: u64,
    pub access_index_flush_errors: u64,
    pub cleanup_runs: u64,
    pub cleanup_scanned_entries: u64,
    pub cleanup_removed_ttl: u64,
    pub cleanup_removed_size: u64,
    pub last_cleanup_duration_ms: u64,
}

#[derive(Debug, Default)]
pub struct PlayerDiskImageCacheStats {
    touch_attempts: AtomicU64,
    touch_updates: AtomicU64,
    access_index_flushes: AtomicU64,
    access_index_flush_errors: AtomicU64,
    cleanup_runs: AtomicU64,
    cleanup_scanned_entries: AtomicU64,
    cleanup_removed_ttl: AtomicU64,
    cleanup_removed_size: AtomicU64,
    last_cleanup_duration_ms: AtomicU64,
}

impl PlayerDiskImageCacheStats {
    pub fn on_touch_attempt(&self) {
        self.touch_attempts.fetch_add(1, Ordering::Relaxed);
    }

    pub fn on_touch_update(&self) {
        self.touch_updates.fetch_add(1, Ordering::Relaxed);
    }

    pub fn on_access_index_flush_ok(&self) {
        self.access_index_flushes.fetch_add(1, Ordering::Relaxed);
    }

    pub fn on_access_index_flush_err(&self) {
        self.access_index_flush_errors
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn on_cleanup_finished(
        &self,
        scanned_entries: u64,
        removed_ttl: u64,
        removed_size: u64,
        duration_ms: u64,
    ) {
        self.cleanup_runs.fetch_add(1, Ordering::Relaxed);
        self.cleanup_scanned_entries
            .fetch_add(scanned_entries, Ordering::Relaxed);
        self.cleanup_removed_ttl
            .fetch_add(removed_ttl, Ordering::Relaxed);
        self.cleanup_removed_size
            .fetch_add(removed_size, Ordering::Relaxed);
        self.last_cleanup_duration_ms
            .store(duration_ms, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> PlayerDiskImageCacheStatsSnapshot {
        PlayerDiskImageCacheStatsSnapshot {
            touch_attempts: self.touch_attempts.load(Ordering::Relaxed),
            touch_updates: self.touch_updates.load(Ordering::Relaxed),
            access_index_flushes: self
                .access_index_flushes
                .load(Ordering::Relaxed),
            access_index_flush_errors: self
                .access_index_flush_errors
                .load(Ordering::Relaxed),
            cleanup_runs: self.cleanup_runs.load(Ordering::Relaxed),
            cleanup_scanned_entries: self
                .cleanup_scanned_entries
                .load(Ordering::Relaxed),
            cleanup_removed_ttl: self
                .cleanup_removed_ttl
                .load(Ordering::Relaxed),
            cleanup_removed_size: self
                .cleanup_removed_size
                .load(Ordering::Relaxed),
            last_cleanup_duration_ms: self
                .last_cleanup_duration_ms
                .load(Ordering::Relaxed),
        }
    }
}
