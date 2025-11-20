//! Cost estimation for different query execution strategies

use crate::query::decision_engine::{
    DataCompleteness, NetworkQuality, QueryComplexity, QueryContext,
    QueryMetrics,
};

/// Estimates the cost (in milliseconds) of different execution strategies
#[derive(Debug, Clone)]
pub struct CostEstimator {
    /// Base costs for different operations
    base_costs: BaseCosts,
}

/// Base cost values for various operations
#[derive(Debug, Clone)]
struct BaseCosts {
    /// Cost per item for client-side sorting
    client_sort_per_item_us: u64, // microseconds

    /// Cost per item for client-side filtering
    client_filter_per_item_us: u64,

    /// Base network latency
    network_base_latency_ms: u64,

    /// Cost per KB of data transfer
    transfer_per_kb_ms: f32,

    /// Server query base cost
    server_query_base_ms: u64,

    /// Cache lookup cost
    cache_lookup_ms: u64,
}

impl Default for BaseCosts {
    fn default() -> Self {
        Self {
            client_sort_per_item_us: 10, // 10 microseconds per item
            client_filter_per_item_us: 5, // 5 microseconds per item
            network_base_latency_ms: 50, // 50ms base latency
            transfer_per_kb_ms: 0.1,     // 0.1ms per KB
            server_query_base_ms: 20,    // 20ms base server processing
            cache_lookup_ms: 1,          // 1ms cache lookup
        }
    }
}

impl CostEstimator {
    /// Create a new cost estimator with default values
    pub fn new() -> Self {
        Self {
            base_costs: BaseCosts::default(),
        }
    }

    /// Estimate the cost of client-side execution
    pub fn estimate_client_cost<T>(
        &self,
        context: &QueryContext<T>,
        data_completeness: DataCompleteness,
        query_complexity: QueryComplexity,
    ) -> u64 {
        let dataset_size = context.available_data.len();

        // Base sorting cost
        let sort_cost_us =
            dataset_size as u64 * self.base_costs.client_sort_per_item_us;

        // Filtering cost (if applicable)
        let has_filters = !context.query.filters.library_ids.is_empty()
            || context.query.filters.media_type.is_some()
            || !context.query.filters.genres.is_empty()
            || context.query.filters.year_range.is_some()
            || context.query.filters.rating_range.is_some()
            || context.query.filters.watch_status.is_some();

        let filter_cost_us = if has_filters {
            dataset_size as u64 * self.base_costs.client_filter_per_item_us
        } else {
            0
        };

        // Penalty for missing data
        let completeness_penalty = match data_completeness {
            DataCompleteness::High => 1.0,
            DataCompleteness::Medium => 1.5,
            DataCompleteness::Low => 3.0, // Significant penalty for incomplete data
        };

        // Complexity multiplier
        let complexity_multiplier = match query_complexity {
            QueryComplexity::Simple => 1.0,
            QueryComplexity::Moderate => 1.5,
            QueryComplexity::Complex => 2.5,
        };

        // Check cache
        let cache_benefit = if context.has_cache {
            match context.cache_age_seconds {
                Some(age) if age < 60 => 0.1,  // Very fresh cache
                Some(age) if age < 300 => 0.3, // Recent cache
                Some(_) => 0.5,                // Stale cache
                None => 1.0,                   // No cache
            }
        } else {
            1.0
        };

        // Calculate total cost in microseconds
        let total_us = (sort_cost_us + filter_cost_us) as f32
            * completeness_penalty
            * complexity_multiplier
            * cache_benefit;

        // Convert to milliseconds
        (total_us / 1000.0) as u64 + self.base_costs.cache_lookup_ms
    }

    /// Estimate the cost of server-side execution
    pub fn estimate_server_cost(
        &self,
        query_complexity: QueryComplexity,
        network_quality: NetworkQuality,
        metrics: &QueryMetrics,
    ) -> u64 {
        // Base server processing cost
        let server_cost = self.base_costs.server_query_base_ms
            * match query_complexity {
                QueryComplexity::Simple => 1,
                QueryComplexity::Moderate => 2,
                QueryComplexity::Complex => 4,
            };

        // Network latency
        let network_latency = match network_quality {
            NetworkQuality::Excellent => {
                self.base_costs.network_base_latency_ms
            }
            NetworkQuality::Good => self.base_costs.network_base_latency_ms * 2,
            NetworkQuality::Poor => self.base_costs.network_base_latency_ms * 5,
            NetworkQuality::Offline => u64::MAX / 2, // Effectively infinite
        };

        // Estimate data transfer cost (assuming ~1KB per item)
        let estimated_response_size_kb = 100; // Rough estimate
        let transfer_cost = (estimated_response_size_kb as f32
            * self.base_costs.transfer_per_kb_ms)
            as u64;

        // Apply historical adjustment if available
        let historical_adjustment = if !metrics.server_query_times.is_empty() {
            // Find similar complexity queries
            let similar_times: Vec<u64> = metrics
                .server_query_times
                .iter()
                .filter(|(complexity, _)| *complexity == query_complexity)
                .map(|(_, time)| *time)
                .collect();

            if !similar_times.is_empty() {
                let avg_time = similar_times.iter().sum::<u64>()
                    / similar_times.len() as u64;
                avg_time as f32 / server_cost as f32
            } else {
                1.0
            }
        } else {
            1.0
        };

        // Calculate total
        let base_total = server_cost + network_latency + transfer_cost;
        (base_total as f32 * historical_adjustment) as u64
    }

    /// Estimate the cost of a hybrid strategy
    pub fn estimate_hybrid_cost(
        &self,
        client_filter_cost: u64,
        server_sort_cost: u64,
        coordination_overhead_ms: u64,
    ) -> u64 {
        client_filter_cost + server_sort_cost + coordination_overhead_ms
    }

    /// Estimate the benefit of using cached results
    pub fn cache_benefit_factor(cache_age_seconds: Option<u64>) -> f32 {
        match cache_age_seconds {
            Some(age) if age < 10 => 0.01,  // Almost instant
            Some(age) if age < 60 => 0.1,   // Very fast
            Some(age) if age < 300 => 0.3,  // Fast
            Some(age) if age < 3600 => 0.5, // Moderate
            _ => 1.0,                       // No benefit
        }
    }
}

impl Default for CostEstimator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::types::MediaQuery;

    #[test]
    fn test_client_cost_estimation() {
        let estimator = CostEstimator::new();

        // Create a context with 1000 items
        let context = QueryContext {
            query: MediaQuery::default(),
            available_data: vec![(); 1000], // Dummy data
            has_cache: false,
            cache_age_seconds: None,
            expected_total_size: Some(1000),
            hints: Default::default(),
        };

        // Test with high completeness and simple query
        let cost = estimator.estimate_client_cost(
            &context,
            DataCompleteness::High,
            QueryComplexity::Simple,
        );

        // Should be relatively low (around 10-20ms for 1000 items)
        assert!(cost < 50);

        // Test with low completeness and complex query
        let cost_complex = estimator.estimate_client_cost(
            &context,
            DataCompleteness::Low,
            QueryComplexity::Complex,
        );

        // Should be significantly higher
        assert!(cost_complex > cost * 3);
    }

    #[test]
    fn test_server_cost_estimation() {
        let estimator = CostEstimator::new();
        let metrics = QueryMetrics::default();

        // Test with excellent network
        let cost_excellent = estimator.estimate_server_cost(
            QueryComplexity::Simple,
            NetworkQuality::Excellent,
            &metrics,
        );

        // Should be reasonable (< 200ms)
        assert!(cost_excellent < 200);

        // Test with poor network
        let cost_poor = estimator.estimate_server_cost(
            QueryComplexity::Simple,
            NetworkQuality::Poor,
            &metrics,
        );

        // Should be much higher
        assert!(cost_poor > cost_excellent * 3);

        // Test offline
        let cost_offline = estimator.estimate_server_cost(
            QueryComplexity::Simple,
            NetworkQuality::Offline,
            &metrics,
        );

        // Should be effectively infinite
        assert!(cost_offline > 1_000_000);
    }

    #[test]
    fn test_cache_benefit() {
        assert!(CostEstimator::cache_benefit_factor(Some(5)) < 0.1);
        assert!(CostEstimator::cache_benefit_factor(Some(120)) < 0.5);
        assert_eq!(CostEstimator::cache_benefit_factor(None), 1.0);
    }
}
