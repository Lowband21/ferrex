use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Maximum number of performance samples to keep in history
const MAX_HISTORY_SIZE: usize = 100;

/// Performance metrics for a single search execution
#[derive(Debug, Clone)]
pub struct SearchPerformanceMetrics {
    pub strategy: super::types::SearchStrategy,
    pub query_length: usize,
    pub field_count: usize,
    pub execution_time: Duration,
    pub result_count: usize,
    pub success: bool,
    pub network_latency: Option<Duration>,
    pub timestamp: Instant,
}

/// Rolling performance history for search operations
#[derive(Debug, Clone)]
pub struct PerformanceHistory {
    client_metrics: VecDeque<SearchPerformanceMetrics>,
    server_metrics: VecDeque<SearchPerformanceMetrics>,
    hybrid_metrics: VecDeque<SearchPerformanceMetrics>,
    network_latency_samples: VecDeque<Duration>,
}

impl Default for PerformanceHistory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl PerformanceHistory {
    pub fn new() -> Self {
        Self {
            client_metrics: VecDeque::with_capacity(MAX_HISTORY_SIZE),
            server_metrics: VecDeque::with_capacity(MAX_HISTORY_SIZE),
            hybrid_metrics: VecDeque::with_capacity(MAX_HISTORY_SIZE),
            network_latency_samples: VecDeque::with_capacity(MAX_HISTORY_SIZE),
        }
    }

    /// Add a performance metric to history
    pub fn add_metric(&mut self, metric: SearchPerformanceMetrics) {
        let queue = match metric.strategy {
            super::types::SearchStrategy::Client => &mut self.client_metrics,
            super::types::SearchStrategy::Server => &mut self.server_metrics,
            super::types::SearchStrategy::Hybrid => &mut self.hybrid_metrics,
        };

        if queue.len() >= MAX_HISTORY_SIZE {
            queue.pop_front();
        }

        if let Some(latency) = metric.network_latency {
            if self.network_latency_samples.len() >= MAX_HISTORY_SIZE {
                self.network_latency_samples.pop_front();
            }
            self.network_latency_samples.push_back(latency);
        }

        queue.push_back(metric);
    }

    /// Get average execution time for a strategy
    pub fn get_average_execution_time(
        &self,
        strategy: super::types::SearchStrategy,
    ) -> Option<Duration> {
        let metrics = match strategy {
            super::types::SearchStrategy::Client => &self.client_metrics,
            super::types::SearchStrategy::Server => &self.server_metrics,
            super::types::SearchStrategy::Hybrid => &self.hybrid_metrics,
        };

        if metrics.is_empty() {
            return None;
        }

        let total: Duration = metrics
            .iter()
            .filter(|m| m.success)
            .map(|m| m.execution_time)
            .sum();

        let count = metrics.iter().filter(|m| m.success).count();

        if count == 0 {
            None
        } else {
            Some(total / count as u32)
        }
    }

    /// Get success rate for a strategy
    pub fn get_success_rate(&self, strategy: super::types::SearchStrategy) -> f32 {
        let metrics = match strategy {
            super::types::SearchStrategy::Client => &self.client_metrics,
            super::types::SearchStrategy::Server => &self.server_metrics,
            super::types::SearchStrategy::Hybrid => &self.hybrid_metrics,
        };

        if metrics.is_empty() {
            return 0.0;
        }

        let successful = metrics.iter().filter(|m| m.success).count();
        successful as f32 / metrics.len() as f32
    }

    /// Get average network latency
    pub fn get_average_network_latency(&self) -> Option<Duration> {
        if self.network_latency_samples.is_empty() {
            return None;
        }

        let total: Duration = self.network_latency_samples.iter().sum();
        Some(total / self.network_latency_samples.len() as u32)
    }

    /// Get 95th percentile execution time for a strategy
    pub fn get_p95_execution_time(
        &self,
        strategy: super::types::SearchStrategy,
    ) -> Option<Duration> {
        let metrics = match strategy {
            super::types::SearchStrategy::Client => &self.client_metrics,
            super::types::SearchStrategy::Server => &self.server_metrics,
            super::types::SearchStrategy::Hybrid => &self.hybrid_metrics,
        };

        let mut times: Vec<Duration> = metrics
            .iter()
            .filter(|m| m.success)
            .map(|m| m.execution_time)
            .collect();

        if times.is_empty() {
            return None;
        }

        times.sort();
        let index = (times.len() as f32 * 0.95) as usize;
        times.get(index.min(times.len() - 1)).cloned()
    }

    /// Check if recent performance indicates issues
    pub fn has_recent_failures(
        &self,
        strategy: super::types::SearchStrategy,
        threshold: f32,
    ) -> bool {
        let metrics = match strategy {
            super::types::SearchStrategy::Client => &self.client_metrics,
            super::types::SearchStrategy::Server => &self.server_metrics,
            super::types::SearchStrategy::Hybrid => &self.hybrid_metrics,
        };

        let recent_count = 5.min(metrics.len());
        if recent_count == 0 {
            return false;
        }

        let recent_failures = metrics
            .iter()
            .rev()
            .take(recent_count)
            .filter(|m| !m.success)
            .count();

        recent_failures as f32 / recent_count as f32 > threshold
    }
}

/// Network quality monitor
#[derive(Debug, Clone)]
pub struct NetworkMonitor {
    last_check: Option<Instant>,
    is_online: bool,
    connection_quality: ConnectionQuality,
    recent_errors: VecDeque<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConnectionQuality {
    Excellent, // < 50ms latency, no errors
    Good,      // < 150ms latency, few errors
    Fair,      // < 500ms latency, some errors
    Poor,      // > 500ms latency or many errors
    Offline,   // No connection
}

impl NetworkMonitor {
    pub fn new() -> Self {
        Self {
            last_check: None,
            is_online: true,
            connection_quality: ConnectionQuality::Good,
            recent_errors: VecDeque::with_capacity(10),
        }
    }

    /// Update network status based on a successful operation
    pub fn record_success(&mut self, latency: Duration) {
        self.is_online = true;
        self.last_check = Some(Instant::now());

        self.connection_quality = if latency < Duration::from_millis(50) {
            ConnectionQuality::Excellent
        } else if latency < Duration::from_millis(150) {
            ConnectionQuality::Good
        } else if latency < Duration::from_millis(500) {
            ConnectionQuality::Fair
        } else {
            ConnectionQuality::Poor
        };

        // Clear old errors
        let now = Instant::now();
        self.recent_errors
            .retain(|&t| now.duration_since(t) < Duration::from_secs(60));
    }

    /// Update network status based on a failed operation
    pub fn record_failure(&mut self) {
        let now = Instant::now();
        self.last_check = Some(now);

        if self.recent_errors.len() >= 10 {
            self.recent_errors.pop_front();
        }
        self.recent_errors.push_back(now);

        // Update quality based on error rate
        let error_count = self.recent_errors.len();
        if error_count >= 5 {
            self.is_online = false;
            self.connection_quality = ConnectionQuality::Offline;
        } else if error_count >= 3 {
            self.connection_quality = ConnectionQuality::Poor;
        } else if error_count >= 1 {
            self.connection_quality = ConnectionQuality::Fair;
        }
    }

    pub fn is_online(&self) -> bool {
        self.is_online
    }

    pub fn quality(&self) -> ConnectionQuality {
        self.connection_quality
    }

    pub fn should_prefer_client(&self) -> bool {
        matches!(
            self.connection_quality,
            ConnectionQuality::Poor | ConnectionQuality::Offline
        )
    }
}

/// Client hardware capabilities
#[derive(Debug, Clone)]
pub struct ClientCapabilities {
    pub cpu_cores: usize,
    pub available_memory_mb: usize,
    pub media_store_size: usize,
    pub average_client_search_time: Option<Duration>,
}

impl ClientCapabilities {
    pub fn new() -> Self {
        Self {
            cpu_cores: num_cpus::get(),
            available_memory_mb: 0, // Will be updated at runtime
            media_store_size: 0,
            average_client_search_time: None,
        }
    }

    pub fn update_memory(&mut self) {
        // This would use a system info crate in production
        // For now, we'll estimate based on available heap
        self.available_memory_mb = 1024; // Placeholder
    }

    pub fn is_low_spec(&self) -> bool {
        self.cpu_cores <= 2 || self.available_memory_mb < 512
    }

    pub fn can_handle_large_search(&self) -> bool {
        !self.is_low_spec() && self.media_store_size < 10000
    }
}

/// Strategy weight calculator for decision making
#[derive(Debug, Clone)]
pub struct StrategyWeights {
    client_weight: f32,
    server_weight: f32,
    hybrid_weight: f32,
}

impl StrategyWeights {
    pub fn new() -> Self {
        Self {
            client_weight: 1.0,
            server_weight: 1.0,
            hybrid_weight: 1.0,
        }
    }

    /// Calculate weights based on historical performance and current conditions
    pub fn calculate(
        &mut self,
        history: &PerformanceHistory,
        network: &NetworkMonitor,
        capabilities: &ClientCapabilities,
    ) {
        // Base weights from success rates
        self.client_weight = history.get_success_rate(super::types::SearchStrategy::Client);
        self.server_weight = history.get_success_rate(super::types::SearchStrategy::Server);
        self.hybrid_weight = history.get_success_rate(super::types::SearchStrategy::Hybrid);

        // Adjust for network quality
        match network.quality() {
            ConnectionQuality::Offline => {
                self.server_weight = 0.0;
                self.hybrid_weight = 0.0;
            }
            ConnectionQuality::Poor => {
                self.server_weight *= 0.3;
                self.hybrid_weight *= 0.5;
            }
            ConnectionQuality::Fair => {
                self.server_weight *= 0.7;
                self.hybrid_weight *= 0.8;
            }
            _ => {}
        }

        // Adjust for client capabilities
        if capabilities.is_low_spec() {
            self.client_weight *= 0.5;
            self.server_weight *= 1.5;
        }

        // Adjust based on execution times
        if let Some(client_time) =
            history.get_average_execution_time(super::types::SearchStrategy::Client)
        {
            if let Some(server_time) =
                history.get_average_execution_time(super::types::SearchStrategy::Server)
            {
                let time_ratio = client_time.as_millis() as f32 / server_time.as_millis() as f32;
                if time_ratio > 2.0 {
                    self.client_weight *= 0.5;
                } else if time_ratio < 0.5 {
                    self.server_weight *= 0.5;
                }
            }
        }

        // Normalize weights
        let total = self.client_weight + self.server_weight + self.hybrid_weight;
        if total > 0.0 {
            self.client_weight /= total;
            self.server_weight /= total;
            self.hybrid_weight /= total;
        } else {
            // Fallback to defaults if no data
            self.client_weight = 0.5;
            self.server_weight = 0.3;
            self.hybrid_weight = 0.2;
        }
    }

    pub fn get_best_strategy(&self) -> super::types::SearchStrategy {
        if self.server_weight >= self.client_weight && self.server_weight >= self.hybrid_weight {
            super::types::SearchStrategy::Server
        } else if self.client_weight >= self.hybrid_weight {
            super::types::SearchStrategy::Client
        } else {
            super::types::SearchStrategy::Hybrid
        }
    }
}
