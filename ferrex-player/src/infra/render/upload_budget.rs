//! Dynamic texture upload budget system.
//!
//! Replaces the static `MAX_UPLOADS_PER_FRAME` cap with a time-based budget
//! that measures actual upload duration and uses EMA smoothing to predict
//! whether additional uploads will fit within the frame budget.

use std::time::{Duration, Instant};

use log::warn;

/// Configuration for the upload budget system.
#[derive(Debug, Clone)]
pub struct UploadBudgetConfig {
    /// Target frame duration (e.g., 8.33ms for 120Hz).
    pub frame_budget: Duration,
    /// Safety margin subtracted from budget to account for GPU execution latency.
    pub safety_margin: Duration,
    /// EMA smoothing factor (0.0-1.0). Higher = more responsive, lower = more stable.
    pub ema_alpha: f32,
    /// Initial upload time estimate used before any measurements.
    pub initial_estimate: Duration,
    /// Hard ceiling on uploads per frame (safety valve).
    pub max_uploads: u32,
}

impl Default for UploadBudgetConfig {
    fn default() -> Self {
        Self::for_hz(120)
    }
}

impl UploadBudgetConfig {
    /// Create a config targeting a specific refresh rate.
    ///
    /// # Examples
    /// ```
    /// use ferrex_player::infra::render::upload_budget::UploadBudgetConfig;
    /// let config = UploadBudgetConfig::for_hz(144); // 6.94ms budget
    /// ```
    pub fn for_hz(hz: u32) -> Self {
        Self {
            frame_budget: Duration::from_secs_f64(1.0 / hz as f64),
            safety_margin: Duration::from_millis(2),
            ema_alpha: 0.2,
            initial_estimate: Duration::from_millis(3),
            max_uploads: 256,
        }
    }

    /// Create a config with a specific frame budget duration.
    pub fn with_budget(frame_budget: Duration) -> Self {
        Self {
            frame_budget,
            safety_margin: Duration::from_millis(2),
            ema_alpha: 0.2,
            initial_estimate: Duration::from_millis(3),
            max_uploads: 256,
        }
    }
}

/// Time-based upload budget with EMA prediction.
///
/// Tracks upload timing within a frame and uses exponential moving average
/// to predict whether additional uploads will fit within the remaining budget.
#[derive(Debug)]
pub struct TimingBasedBudget {
    config: UploadBudgetConfig,
    /// When this frame's prepare phase started.
    frame_start: Instant,
    /// Total time consumed by uploads this frame.
    upload_time_this_frame: Duration,
    /// Number of uploads completed this frame.
    uploads_this_frame: u32,
    /// Exponential moving average of upload duration.
    avg_upload_time: Duration,
}

impl TimingBasedBudget {
    /// Create a new budget tracker with the given configuration.
    pub fn new(config: UploadBudgetConfig) -> Self {
        let avg_upload_time = config.initial_estimate;
        Self {
            config,
            frame_start: Instant::now(),
            upload_time_this_frame: Duration::ZERO,
            uploads_this_frame: 0,
            avg_upload_time,
        }
    }

    /// Reset per-frame counters. Call at the start of each frame's prepare phase.
    pub fn begin_frame(&mut self) {
        self.frame_start = Instant::now();
        self.upload_time_this_frame = Duration::ZERO;
        self.uploads_this_frame = 0;
    }

    /// Returns the remaining time budget for this frame.
    pub fn remaining_budget(&self) -> Duration {
        let elapsed = self.frame_start.elapsed();
        self.config
            .frame_budget
            .saturating_sub(elapsed)
            .saturating_sub(self.config.safety_margin)
    }

    /// Check if we can afford another upload based on timing and hard cap.
    ///
    /// Returns `true` if:
    /// - We haven't hit the hard upload cap
    /// - The remaining budget exceeds our predicted upload time
    pub fn can_upload(&self) -> bool {
        // Hard cap check
        if self.uploads_this_frame >= self.config.max_uploads {
            warn!(
                "Hard upload cap of {} reached, did you spend too much money on your computer?",
                self.config.max_uploads
            );
            return false;
        }

        // Always allow at least one upload per frame to make progress
        if self.uploads_this_frame == 0 {
            return true;
        }

        // Check if remaining budget exceeds our predicted upload time
        let under_budget = self.remaining_budget() > self.avg_upload_time;

        if !under_budget {
            warn!(
                "Per-frame texture upload budget reached with an average upload time of {:#?}",
                self.avg_upload_time
            )
        }

        under_budget
    }

    /// Record a completed upload and update the EMA.
    ///
    /// Call this immediately after `upload_raster()` returns, passing the
    /// duration measured via `Instant::now()` before and after the call.
    pub fn record_upload(&mut self, duration: Duration) {
        self.uploads_this_frame += 1;
        self.upload_time_this_frame += duration;

        // EMA update: avg = alpha * new + (1-alpha) * old
        let alpha = self.config.ema_alpha;
        let new_us = duration.as_micros() as f64;
        let old_us = self.avg_upload_time.as_micros() as f64;
        let updated_us =
            (alpha as f64 * new_us + (1.0 - alpha as f64) * old_us) as u64;

        // Floor at 100 microseconds to prevent degenerate estimates
        self.avg_upload_time = Duration::from_micros(updated_us.max(100));
    }

    /// Called at end of frame for logging/metrics.
    pub fn end_frame(&mut self) {
        // Log stats if debug logging is enabled
        #[cfg(debug_assertions)]
        if log::log_enabled!(log::Level::Debug)
            && self.upload_time_this_frame > self.config.frame_budget / 2
        {
            let remaining_budget = self.remaining_budget();
            if self.remaining_budget() < self.config.frame_budget / 10 {
                log::warn!(
                    "Ended frame with less than 10% of upload budget remaining:"
                );
                log::warn!(
                    "UploadBudget: {} uploads in {:?}, avg={:?}, remaining={:?}",
                    self.uploads_this_frame,
                    self.upload_time_this_frame,
                    self.avg_upload_time,
                    remaining_budget,
                );
            } else {
                log::debug!(
                    "UploadBudget: {} uploads in {:?}, avg={:?}, remaining={:?}",
                    self.uploads_this_frame,
                    self.upload_time_this_frame,
                    self.avg_upload_time,
                    remaining_budget,
                );
            }
        }
    }

    // --- Accessors for debugging/profiling ---

    /// Number of uploads completed this frame.
    pub fn uploads_this_frame(&self) -> u32 {
        self.uploads_this_frame
    }

    /// Current EMA of upload duration.
    pub fn avg_upload_time(&self) -> Duration {
        self.avg_upload_time
    }

    /// Total time spent uploading this frame.
    pub fn upload_time_this_frame(&self) -> Duration {
        self.upload_time_this_frame
    }

    /// The configuration this budget is using.
    pub fn config(&self) -> &UploadBudgetConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_for_hz() {
        let config = UploadBudgetConfig::for_hz(120);
        // 120Hz = 8.333...ms per frame
        assert!(config.frame_budget.as_micros() >= 8333);
        assert!(config.frame_budget.as_micros() <= 8334);

        let config = UploadBudgetConfig::for_hz(60);
        // 60Hz = 16.666...ms per frame
        assert!(config.frame_budget.as_micros() >= 16666);
        assert!(config.frame_budget.as_micros() <= 16667);
    }

    #[test]
    fn test_first_upload_always_allowed() {
        let config = UploadBudgetConfig::for_hz(120);
        let budget = TimingBasedBudget::new(config);
        assert!(budget.can_upload());
    }

    #[test]
    fn test_hard_cap_enforced() {
        let config = UploadBudgetConfig {
            max_uploads: 2,
            ..UploadBudgetConfig::for_hz(120)
        };
        let mut budget = TimingBasedBudget::new(config);

        // Record 2 uploads
        budget.record_upload(Duration::from_micros(100));
        budget.record_upload(Duration::from_micros(100));

        // Should be blocked by hard cap
        assert!(!budget.can_upload());
    }

    #[test]
    fn test_ema_updates() {
        let config = UploadBudgetConfig {
            ema_alpha: 0.5, // 50% weight to make math easier
            initial_estimate: Duration::from_millis(2),
            ..UploadBudgetConfig::for_hz(120)
        };
        let mut budget = TimingBasedBudget::new(config);

        // Initial estimate is 2ms
        assert_eq!(budget.avg_upload_time().as_millis(), 2);

        // Record a 4ms upload: new avg = 0.5 * 4ms + 0.5 * 2ms = 3ms
        budget.record_upload(Duration::from_millis(4));
        assert_eq!(budget.avg_upload_time().as_millis(), 3);
    }

    #[test]
    fn test_begin_frame_resets_counters() {
        let config = UploadBudgetConfig::for_hz(120);
        let mut budget = TimingBasedBudget::new(config);

        budget.record_upload(Duration::from_millis(1));
        budget.record_upload(Duration::from_millis(1));
        assert_eq!(budget.uploads_this_frame(), 2);

        budget.begin_frame();
        assert_eq!(budget.uploads_this_frame(), 0);
        assert_eq!(budget.upload_time_this_frame(), Duration::ZERO);
    }
}
