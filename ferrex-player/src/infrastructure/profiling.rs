//! High-performance profiling infrastructure using the `profiling` crate
//!
//! This module provides a thin abstraction over the `profiling` crate,
//! which itself abstracts over multiple profiling backends (puffin, tracy, etc).
//!
//! Features:
//! - Zero-cost when profiling features are disabled
//! - Multiple backend support (puffin, tracy, tracing)
//! - Frame timing statistics
//! - Memory usage tracking

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[cfg(feature = "profiling-stats")]
use hdrhistogram::Histogram;

#[cfg(feature = "profiling-stats")]
use parking_lot::RwLock;

/// Multi-tier profiling system with feature-gated backends
pub struct Profiler {
    enabled: AtomicBool,

    #[cfg(feature = "profiling-stats")]
    frame_counter: AtomicU64,

    #[cfg(feature = "profiling-stats")]
    frame_times: RwLock<Histogram<u64>>,

    #[cfg(feature = "profiling-stats")]
    last_report: RwLock<Instant>,

    #[cfg(feature = "memory-stats")]
    baseline_memory: AtomicU64,

    #[cfg(feature = "memory-stats")]
    peak_memory: AtomicU64,
}

impl Profiler {
    pub fn new() -> Arc<Self> {
        // Initialize puffin if enabled
        #[cfg(feature = "profile-with-puffin")]
        {
            puffin::set_scopes_on(true);
            log::info!("Puffin profiling enabled");

            // Start puffin server if puffin_http is available
            #[cfg(any(
                feature = "puffin-server",
                feature = "profile-with-puffin"
            ))]
            {
                let server_addr = "127.0.0.1:8585";
                let puffin_server =
                    puffin_http::Server::new(server_addr).unwrap();

                // Set the profiler callback to send data to the server
                puffin::set_scopes_on(true);

                log::info!("Puffin data server started on {}", server_addr);
                log::info!(
                    "To view profiling data, run: puffin_viewer --url {}",
                    server_addr
                );

                // Keep server alive for the entire application lifetime
                std::mem::forget(puffin_server);
            }
        }

        // Initialize tracy if enabled - no special setup needed
        #[cfg(feature = "profile-with-tracy")]
        {
            log::info!("Tracy profiling enabled");
        }

        #[cfg(feature = "memory-stats")]
        let initial_memory = memory_stats::memory_stats()
            .map(|s| s.physical_mem)
            .unwrap_or(0);

        Arc::new(Self {
            enabled: AtomicBool::new(true),

            #[cfg(feature = "profiling-stats")]
            frame_counter: AtomicU64::new(0),

            #[cfg(feature = "profiling-stats")]
            frame_times: RwLock::new(
                Histogram::new_with_bounds(1, 1_000_000, 3)
                    .expect("Failed to create histogram"),
            ),

            #[cfg(feature = "profiling-stats")]
            last_report: RwLock::new(Instant::now()),

            #[cfg(feature = "memory-stats")]
            baseline_memory: AtomicU64::new(initial_memory as u64),

            #[cfg(feature = "memory-stats")]
            peak_memory: AtomicU64::new(initial_memory as u64),
        })
    }

    /// Enable or disable profiling at runtime
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);

        #[cfg(feature = "profile-with-puffin")]
        puffin::set_scopes_on(enabled);
    }

    /// Check if profiling is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Mark the beginning of a new frame
    pub fn begin_frame(&self) {
        // Call the profiling crate's finish_frame macro for puffin
        // This tells puffin that the previous frame has ended and a new one begins
        profiling::finish_frame!();

        #[cfg(feature = "profiling-stats")]
        {
            self.frame_counter.fetch_add(1, Ordering::Relaxed);
        }

        #[cfg(feature = "memory-stats")]
        {
            self.update_memory_stats();
        }
    }

    /// Record frame timing
    #[cfg(feature = "profiling-stats")]
    pub fn record_frame_time(&self, duration: Duration) {
        let micros = duration.as_micros() as u64;

        if let Some(mut histogram) = self.frame_times.try_write() {
            let _ = histogram.record(micros);

            // Report statistics every second
            let mut last_report = self.last_report.write();
            if last_report.elapsed() > Duration::from_secs(1) {
                self.report_statistics(&histogram);
                *last_report = Instant::now();
            }
        }
    }

    /// Report frame statistics
    #[cfg(feature = "profiling-stats")]
    fn report_statistics(&self, histogram: &Histogram<u64>) {
        let frame_count = self.frame_counter.load(Ordering::Relaxed);

        log::info!(
            "Frame stats - Count: {}, P50: {}μs, P95: {}μs, P99: {}μs, Max: {}μs",
            frame_count,
            histogram.value_at_percentile(50.0),
            histogram.value_at_percentile(95.0),
            histogram.value_at_percentile(99.0),
            histogram.max()
        );

        // Check for frame budget violations
        let p95 = histogram.value_at_percentile(95.0);
        if p95 > 16_667 {
            // 16.67ms for 60fps
            log::warn!(
                "Frame budget violation: P95 = {}μs (target: 16667μs)",
                p95
            );
        }
    }

    /// Get current frame statistics
    #[cfg(feature = "profiling-stats")]
    pub fn get_frame_stats(&self) -> FrameStats {
        let histogram = self.frame_times.read();

        FrameStats {
            count: self.frame_counter.load(Ordering::Relaxed),
            p50_micros: histogram.value_at_percentile(50.0),
            p95_micros: histogram.value_at_percentile(95.0),
            p99_micros: histogram.value_at_percentile(99.0),
            max_micros: histogram.max(),
        }
    }

    /// Update memory statistics
    #[cfg(feature = "memory-stats")]
    pub fn update_memory_stats(&self) {
        if let Some(stats) = memory_stats::memory_stats() {
            let current = stats.physical_mem as u64;

            // Update peak memory if needed
            let mut peak = self.peak_memory.load(Ordering::Relaxed);
            while current > peak {
                match self.peak_memory.compare_exchange_weak(
                    peak,
                    current,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(actual) => peak = actual,
                }
            }
        }
    }

    /// Get current memory statistics
    #[cfg(feature = "memory-stats")]
    pub fn get_memory_stats(&self) -> MemoryStats {
        let stats = memory_stats::memory_stats();
        let baseline = self.baseline_memory.load(Ordering::Relaxed);
        let peak = self.peak_memory.load(Ordering::Relaxed);

        MemoryStats {
            current_bytes: stats
                .as_ref()
                .map(|s| s.physical_mem as u64)
                .unwrap_or(0),
            peak_bytes: peak,
            baseline_bytes: baseline,
            virtual_bytes: stats
                .as_ref()
                .map(|s| s.virtual_mem as u64)
                .unwrap_or(0),
        }
    }
}

/// Frame timing statistics
#[cfg(feature = "profiling-stats")]
#[derive(Debug, Clone)]
pub struct FrameStats {
    pub count: u64,
    pub p50_micros: u64,
    pub p95_micros: u64,
    pub p99_micros: u64,
    pub max_micros: u64,
}

/// Memory statistics
#[cfg(feature = "memory-stats")]
#[derive(Debug, Clone)]
pub struct MemoryStats {
    pub current_bytes: u64,
    pub peak_bytes: u64,
    pub baseline_bytes: u64,
    pub virtual_bytes: u64,
}

// =============================================================================
// Global Profiler Instance
// =============================================================================

lazy_static::lazy_static! {
    pub static ref PROFILER: Arc<Profiler> = Profiler::new();
}

/// Initialize the profiling system
pub fn init() {
    // Force lazy_static initialization
    let _ = &*PROFILER;

    // Register main thread with profiling backends
    profiling::register_thread!("Main Thread");

    // For puffin, we need to ensure scopes are enabled
    #[cfg(feature = "profile-with-puffin")]
    puffin::set_scopes_on(true);

    log::info!(
        "Profiling system initialized with features: {}",
        [
            #[cfg(feature = "profile-with-puffin")]
            "puffin",
            #[cfg(feature = "profile-with-tracy")]
            "tracy",
            #[cfg(feature = "profile-with-tracing")]
            "tracing",
            #[cfg(feature = "profiling-stats")]
            "stats",
        ]
        .join(", ")
    );
}

/// Shutdown the profiling system and export data
pub fn shutdown() {
    #[cfg(feature = "profiling-stats")]
    {
        let stats = PROFILER.get_frame_stats();
        log::info!("Final frame statistics:");
        log::info!("  Total frames: {}", stats.count);
        log::info!("  P50: {}ms", stats.p50_micros as f64 / 1000.0);
        log::info!("  P95: {}ms", stats.p95_micros as f64 / 1000.0);
        log::info!("  P99: {}ms", stats.p99_micros as f64 / 1000.0);
        log::info!("  Max: {}ms", stats.max_micros as f64 / 1000.0);
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profiler_creation() {
        let profiler = Profiler::new();
        assert!(profiler.is_enabled());
    }

    #[test]
    fn test_profiler_toggle() {
        let profiler = Profiler::new();
        profiler.set_enabled(false);
        assert!(!profiler.is_enabled());
        profiler.set_enabled(true);
        assert!(profiler.is_enabled());
    }

    #[test]
    fn test_profiling_macros() {
        // Test that profiling macros compile
        profiling::scope!("test_operation");

        #[profiling::function]
        fn test_function() {
            // Some work
            std::thread::sleep(Duration::from_millis(1));
        }

        test_function();
    }
}
