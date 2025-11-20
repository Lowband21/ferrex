use super::service::SearchService;
use super::types::SearchStrategy;
use ferrex_core::query::types::SearchField;
use std::time::{Duration, Instant};

/// Lightweight calibration results
#[derive(Debug, Clone)]
pub struct CalibrationResults {
    pub client_baseline_ms: Option<u64>,
    pub server_baseline_ms: Option<u64>,
    pub network_latency_ms: Option<u64>,
    pub optimal_strategy: SearchStrategy,
    pub calibrated_at: Instant,
}

impl Default for CalibrationResults {
    fn default() -> Self {
        Self {
            client_baseline_ms: None,
            server_baseline_ms: None,
            network_latency_ms: None,
            optimal_strategy: SearchStrategy::Client,
            calibrated_at: Instant::now(),
        }
    }
}

/// Search calibrator for startup performance testing
pub struct SearchCalibrator;

impl SearchCalibrator {
    /// Run a lightweight calibration to determine optimal search strategy
    /// This should be called once at startup and periodically in background
    pub async fn calibrate(service: &SearchService) -> CalibrationResults {
        let mut results = CalibrationResults::default();

        // Test queries - simple and likely to return results
        let test_queries = ["the", "a", "movie"];

        // Test client performance (always available)
        let client_start = Instant::now();
        for query in &test_queries {
            let _ = service
                .search(
                    query,
                    &[SearchField::Title],
                    SearchStrategy::Client,
                    None,
                    false,
                )
                .await;
        }
        let client_duration = client_start.elapsed();
        results.client_baseline_ms = Some(
            (client_duration.as_millis() / test_queries.len() as u128) as u64,
        );

        // Test server performance (if available)
        if service.has_network() {
            let server_start = Instant::now();
            let mut server_success = false;

            for query in &test_queries {
                if (service
                    .search(
                        query,
                        &[SearchField::Title],
                        SearchStrategy::Server,
                        None,
                        false,
                    )
                    .await)
                    .is_ok()
                {
                    server_success = true;
                }
            }

            if server_success {
                let server_duration = server_start.elapsed();
                results.server_baseline_ms = Some(
                    (server_duration.as_millis() / test_queries.len() as u128)
                        as u64,
                );

                // Estimate network latency (rough approximation)
                if let Some(client_ms) = results.client_baseline_ms
                    && let Some(server_ms) = results.server_baseline_ms
                    && server_ms > client_ms
                {
                    results.network_latency_ms = Some(server_ms - client_ms);
                }
            }
        }

        // Determine optimal strategy based on calibration
        results.optimal_strategy = Self::determine_optimal_strategy(&results);
        results.calibrated_at = Instant::now();

        results
    }

    /// Quick network check without full search
    pub async fn check_network_latency(
        service: &SearchService,
    ) -> Option<Duration> {
        if !service.has_network() {
            return None;
        }

        let start = Instant::now();
        // Try a minimal server query
        match service
            .search(
                "test",
                &[SearchField::Title],
                SearchStrategy::Server,
                None,
                false,
            )
            .await
        {
            Ok(_) => Some(start.elapsed()),
            Err(_) => None,
        }
    }

    fn determine_optimal_strategy(
        results: &CalibrationResults,
    ) -> SearchStrategy {
        match (results.client_baseline_ms, results.server_baseline_ms) {
            (Some(client), Some(server)) => {
                // If server is more than 2x slower, prefer client
                if server > client * 2 {
                    SearchStrategy::Client
                }
                // If server is significantly faster, prefer it
                else if client > server * 2 {
                    SearchStrategy::Server
                }
                // Otherwise, use hybrid for best of both
                else {
                    SearchStrategy::Hybrid
                }
            }
            (Some(_), None) => SearchStrategy::Client,
            (None, Some(_)) => SearchStrategy::Server,
            (None, None) => SearchStrategy::Client,
        }
    }
}
