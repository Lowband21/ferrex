//! Core types for the decision engine

use crate::query::types::MediaQuery;

/// The selected query execution strategy
#[derive(Debug, Clone)]
pub struct QueryStrategy {
    /// The execution mode to use
    pub execution_mode: ExecutionMode,

    /// Confidence in the selected strategy (0.0 to 1.0)
    pub confidence: f32,

    /// Estimated latency in milliseconds
    pub estimated_latency_ms: u64,

    /// Human-readable reasoning for the selection
    pub reasoning: String,
}

/// Execution modes for query processing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExecutionMode {
    /// All operations performed client-side
    ClientOnly,

    /// All operations performed server-side
    ServerOnly,

    /// Client performs filtering, server performs sorting
    HybridClientFilter,

    /// Server performs filtering, client performs sorting
    HybridServerFilter,

    /// Try both approaches in parallel, use the fastest
    ParallelRace,
}

/// Configuration for the decision engine
#[derive(Debug, Clone)]
pub struct StrategyConfig {
    /// Minimum metadata completeness ratio to prefer client-side (0.0 to 1.0)
    pub min_metadata_completeness: f32,

    /// Threshold in milliseconds for triggering parallel race mode
    pub parallel_race_threshold_ms: u64,

    /// Maximum dataset size for client-side processing
    pub max_client_dataset_size: usize,

    /// Minimum network quality score for server-side preference (0.0 to 1.0)
    pub network_quality_threshold: f32,

    /// Weight multiplier for cache hit bonus
    pub cache_hit_weight: f32,

    /// Enable adaptive learning from query results
    pub enable_learning: bool,

    /// Prefer consistency over performance (always use same strategy for similar queries)
    pub prefer_consistency: bool,
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            min_metadata_completeness: 0.8,
            parallel_race_threshold_ms: 500,
            max_client_dataset_size: 10_000,
            network_quality_threshold: 0.7,
            cache_hit_weight: 2.0,
            enable_learning: true,
            prefer_consistency: false,
        }
    }
}

/// Context for query execution decision
#[derive(Debug, Clone)]
pub struct QueryContext<T> {
    /// The query to execute
    pub query: MediaQuery,

    /// Available data on the client
    pub available_data: Vec<T>,

    /// Whether the client has cached results
    pub has_cache: bool,

    /// Age of cached results in seconds (if applicable)
    pub cache_age_seconds: Option<u64>,

    /// Expected total dataset size (if known)
    pub expected_total_size: Option<usize>,

    /// User preferences or hints
    pub hints: QueryHints,
}

/// User-provided hints for query execution
#[derive(Debug, Clone, Default)]
pub struct QueryHints {
    /// User prefers faster response over accuracy
    pub prefer_speed: bool,

    /// User needs absolutely fresh data
    pub require_fresh: bool,

    /// User is on a metered connection
    pub metered_connection: bool,

    /// User explicitly requested offline mode
    pub offline_mode: bool,

    /// Maximum acceptable latency in milliseconds
    pub max_latency_ms: Option<u64>,
}

/// Result of executing a query with performance metrics
#[derive(Debug, Clone)]
pub struct QueryExecutionResult<T> {
    /// The actual execution mode used
    pub execution_mode: ExecutionMode,

    /// The result data
    pub data: Vec<T>,

    /// Actual execution time in milliseconds
    pub execution_time_ms: u64,

    /// Whether the result came from cache
    pub from_cache: bool,

    /// Any errors or warnings
    pub diagnostics: Vec<String>,
}

impl QueryStrategy {
    /// Create a simple client-only strategy
    pub fn client_only() -> Self {
        Self {
            execution_mode: ExecutionMode::ClientOnly,
            confidence: 1.0,
            estimated_latency_ms: 10,
            reasoning: "Client-only execution requested".to_string(),
        }
    }

    /// Create a simple server-only strategy
    pub fn server_only() -> Self {
        Self {
            execution_mode: ExecutionMode::ServerOnly,
            confidence: 1.0,
            estimated_latency_ms: 100,
            reasoning: "Server-only execution requested".to_string(),
        }
    }

    /// Check if the strategy should use client-side processing
    pub fn uses_client(&self) -> bool {
        matches!(
            self.execution_mode,
            ExecutionMode::ClientOnly
                | ExecutionMode::HybridClientFilter
                | ExecutionMode::ParallelRace
        )
    }

    /// Check if the strategy should use server-side processing
    pub fn uses_server(&self) -> bool {
        matches!(
            self.execution_mode,
            ExecutionMode::ServerOnly
                | ExecutionMode::HybridServerFilter
                | ExecutionMode::ParallelRace
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strategy_uses_client_server() {
        let client_strategy = QueryStrategy::client_only();
        assert!(client_strategy.uses_client());
        assert!(!client_strategy.uses_server());

        let server_strategy = QueryStrategy::server_only();
        assert!(!server_strategy.uses_client());
        assert!(server_strategy.uses_server());

        let parallel_strategy = QueryStrategy {
            execution_mode: ExecutionMode::ParallelRace,
            confidence: 0.5,
            estimated_latency_ms: 50,
            reasoning: "Testing".to_string(),
        };
        assert!(parallel_strategy.uses_client());
        assert!(parallel_strategy.uses_server());
    }

    #[test]
    fn test_default_config() {
        let config = StrategyConfig::default();
        assert_eq!(config.min_metadata_completeness, 0.8);
        assert_eq!(config.parallel_race_threshold_ms, 500);
        assert_eq!(config.max_client_dataset_size, 10_000);
    }
}
