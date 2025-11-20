//! Network quality monitoring for strategy selection

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Network quality levels
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub enum NetworkQuality {
    /// No network connection
    Offline,

    /// High latency, packet loss, or low bandwidth
    Poor,

    /// Moderate conditions
    Good,

    /// Low latency, high bandwidth
    Excellent,
}

/// Monitor for tracking network conditions
pub struct NetworkMonitor {
    measurements: Arc<RwLock<NetworkMeasurements>>,
}

impl fmt::Debug for NetworkMonitor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let summary = self
            .measurements
            .read()
            .map(|m| {
                (
                    m.latencies.len(),
                    m.bandwidths.len(),
                    m.packet_loss,
                    m.is_offline,
                    m.last_update.elapsed().as_millis(),
                )
            })
            .unwrap_or((0, 0, 0.0, false, 0));

        f.debug_struct("NetworkMonitor")
            .field("latency_samples", &summary.0)
            .field("bandwidth_samples", &summary.1)
            .field("packet_loss", &summary.2)
            .field("offline", &summary.3)
            .field("last_update_age_ms", &summary.4)
            .finish()
    }
}

/// Network performance measurements
#[derive(Debug, Clone)]
struct NetworkMeasurements {
    /// Recent latency measurements in milliseconds
    latencies: VecDeque<u64>,

    /// Recent bandwidth measurements in bytes per second
    bandwidths: VecDeque<u64>,

    /// Packet loss rate (0.0 to 1.0)
    packet_loss: f32,

    /// Last measurement time
    last_update: Instant,

    /// Whether we're currently offline
    is_offline: bool,
}

impl NetworkMonitor {
    /// Create a new network monitor
    pub fn new() -> Self {
        Self {
            measurements: Arc::new(RwLock::new(NetworkMeasurements {
                latencies: VecDeque::with_capacity(100),
                bandwidths: VecDeque::with_capacity(100),
                packet_loss: 0.0,
                last_update: Instant::now(),
                is_offline: false,
            })),
        }
    }

    /// Get the current network quality assessment
    pub fn current_quality(&self) -> NetworkQuality {
        let measurements = self.measurements.read().unwrap();

        if measurements.is_offline {
            return NetworkQuality::Offline;
        }

        // Calculate average latency
        let avg_latency = if !measurements.latencies.is_empty() {
            measurements.latencies.iter().sum::<u64>() / measurements.latencies.len() as u64
        } else {
            100 // Default to 100ms if no measurements
        };

        // Calculate average bandwidth
        let avg_bandwidth = if !measurements.bandwidths.is_empty() {
            measurements.bandwidths.iter().sum::<u64>() / measurements.bandwidths.len() as u64
        } else {
            1_000_000 // Default to 1MB/s if no measurements
        };

        // Determine quality based on metrics
        match (avg_latency, avg_bandwidth, measurements.packet_loss) {
            (lat, bw, loss) if lat < 50 && bw > 10_000_000 && loss < 0.01 => {
                NetworkQuality::Excellent
            }
            (lat, bw, loss) if lat < 200 && bw > 1_000_000 && loss < 0.05 => NetworkQuality::Good,
            _ => NetworkQuality::Poor,
        }
    }

    /// Record a latency measurement
    pub fn record_latency(&self, latency_ms: u64) {
        let mut measurements = self.measurements.write().unwrap();

        measurements.latencies.push_back(latency_ms);
        if measurements.latencies.len() > 100 {
            measurements.latencies.pop_front();
        }

        measurements.last_update = Instant::now();
        measurements.is_offline = false;
    }

    /// Record a bandwidth measurement
    pub fn record_bandwidth(&self, bytes_per_second: u64) {
        let mut measurements = self.measurements.write().unwrap();

        measurements.bandwidths.push_back(bytes_per_second);
        if measurements.bandwidths.len() > 100 {
            measurements.bandwidths.pop_front();
        }

        measurements.last_update = Instant::now();
        measurements.is_offline = false;
    }

    /// Record packet loss rate
    pub fn record_packet_loss(&self, loss_rate: f32) {
        let mut measurements = self.measurements.write().unwrap();
        measurements.packet_loss = loss_rate.clamp(0.0, 1.0);
        measurements.last_update = Instant::now();
    }

    /// Mark the network as offline
    pub fn set_offline(&self, offline: bool) {
        let mut measurements = self.measurements.write().unwrap();
        measurements.is_offline = offline;
        measurements.last_update = Instant::now();
    }

    /// Estimate round-trip time for a request of given size
    pub fn estimate_rtt(&self, request_size_bytes: usize) -> Duration {
        let measurements = self.measurements.read().unwrap();

        if measurements.is_offline {
            return Duration::from_secs(u64::MAX);
        }

        // Base latency
        let base_latency_ms = if !measurements.latencies.is_empty() {
            measurements.latencies.iter().sum::<u64>() / measurements.latencies.len() as u64
        } else {
            100
        };

        // Add time for data transfer
        let bandwidth = if !measurements.bandwidths.is_empty() {
            measurements.bandwidths.iter().sum::<u64>() / measurements.bandwidths.len() as u64
        } else {
            1_000_000 // 1MB/s default
        };

        let transfer_time_ms = if bandwidth > 0 {
            (request_size_bytes as u64 * 1000) / bandwidth
        } else {
            1000
        };

        // Account for packet loss (retransmissions)
        let packet_loss_factor = 1.0 + measurements.packet_loss;
        let total_ms = ((base_latency_ms + transfer_time_ms) as f32 * packet_loss_factor) as u64;

        Duration::from_millis(total_ms)
    }

    /// Check if measurements are stale
    pub fn measurements_are_stale(&self) -> bool {
        let measurements = self.measurements.read().unwrap();
        measurements.last_update.elapsed() > Duration::from_secs(60)
    }

    /// Get a quality score from 0.0 to 1.0
    pub fn quality_score(&self) -> f32 {
        match self.current_quality() {
            NetworkQuality::Excellent => 1.0,
            NetworkQuality::Good => 0.7,
            NetworkQuality::Poor => 0.3,
            NetworkQuality::Offline => 0.0,
        }
    }
}

impl Default for NetworkMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper for simulating network conditions in tests
#[cfg(test)]
impl NetworkMonitor {
    /// Simulate excellent network conditions
    pub fn simulate_excellent(&self) {
        self.record_latency(20);
        self.record_bandwidth(100_000_000); // 100MB/s
        self.record_packet_loss(0.0);
    }

    /// Simulate good network conditions
    pub fn simulate_good(&self) {
        self.record_latency(100);
        self.record_bandwidth(10_000_000); // 10MB/s
        self.record_packet_loss(0.01);
    }

    /// Simulate poor network conditions
    pub fn simulate_poor(&self) {
        self.record_latency(500);
        self.record_bandwidth(100_000); // 100KB/s
        self.record_packet_loss(0.1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_quality_detection() {
        let monitor = NetworkMonitor::new();

        // Test offline detection
        monitor.set_offline(true);
        assert_eq!(monitor.current_quality(), NetworkQuality::Offline);
        assert_eq!(monitor.quality_score(), 0.0);

        // Test excellent conditions
        monitor.simulate_excellent();
        assert_eq!(monitor.current_quality(), NetworkQuality::Excellent);
        assert_eq!(monitor.quality_score(), 1.0);

        // Test good conditions
        monitor.simulate_good();
        assert_eq!(monitor.current_quality(), NetworkQuality::Good);
        assert_eq!(monitor.quality_score(), 0.7);

        // Test poor conditions
        monitor.simulate_poor();
        assert_eq!(monitor.current_quality(), NetworkQuality::Poor);
        assert_eq!(monitor.quality_score(), 0.3);
    }

    #[test]
    fn test_rtt_estimation() {
        let monitor = NetworkMonitor::new();

        // Set known conditions
        monitor.record_latency(50);
        monitor.record_bandwidth(1_000_000); // 1MB/s
        monitor.record_packet_loss(0.0);

        // Small request should be mostly latency
        let small_rtt = monitor.estimate_rtt(1024); // 1KB
        assert!(small_rtt.as_millis() >= 50);
        assert!(small_rtt.as_millis() < 100);

        // Large request should include transfer time
        let large_rtt = monitor.estimate_rtt(10_000_000); // 10MB
        assert!(large_rtt.as_secs() >= 10); // At least 10 seconds at 1MB/s
    }

    #[test]
    fn test_measurement_staleness() {
        let monitor = NetworkMonitor::new();

        // Fresh measurements
        monitor.record_latency(50);
        assert!(!monitor.measurements_are_stale());

        // Can't easily test staleness without mocking time
        // In production, this would return true after 60 seconds
    }
}
