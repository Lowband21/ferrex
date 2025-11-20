//! Decision Engine for Smart Client/Server Strategy Selection
//! 
//! This module provides an intelligent decision engine that determines whether
//! to use client-side or server-side sorting/filtering based on:
//! - Data availability and completeness
//! - Query complexity
//! - Network conditions
//! - Historical performance metrics

pub mod types;
pub mod analyzers;
pub mod estimator;
pub mod monitor;

pub use types::{QueryStrategy, ExecutionMode, StrategyConfig, QueryContext};
pub use analyzers::{DataCompleteness, DataCompletenessAnalyzer, QueryComplexityAnalyzer};
pub use estimator::CostEstimator;
pub use monitor::{NetworkMonitor, NetworkQuality};

use std::sync::{Arc, RwLock};
use crate::query::sorting::SortableEntity;

/// The main decision engine that orchestrates strategy selection
pub struct DecisionEngine {
    config: StrategyConfig,
    metrics: Arc<RwLock<QueryMetrics>>,
    network_monitor: NetworkMonitor,
    cost_estimator: CostEstimator,
}

/// Metrics tracking for query performance
#[derive(Debug, Clone, Default)]
pub struct QueryMetrics {
    /// Average client-side sort time for different dataset sizes
    pub client_sort_times: Vec<(usize, u64)>, // (dataset_size, time_ms)
    
    /// Average server query times for different complexities
    pub server_query_times: Vec<(QueryComplexity, u64)>,
    
    /// Network latency measurements
    pub network_latencies: Vec<u64>,
    
    /// Cache hit rates
    pub cache_hit_rate: f32,
    
    /// Success rates for different strategies
    pub strategy_success_rates: std::collections::HashMap<ExecutionMode, f32>,
}

/// Query complexity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueryComplexity {
    Simple,   // Single field sort, no filters
    Moderate, // Multi-field sort or simple filters
    Complex,  // Complex filters with multiple sorts
}

impl DecisionEngine {
    /// Create a new decision engine with default configuration
    pub fn new() -> Self {
        Self::with_config(StrategyConfig::default())
    }
    
    /// Create a new decision engine with custom configuration
    pub fn with_config(config: StrategyConfig) -> Self {
        Self {
            config,
            metrics: Arc::new(RwLock::new(QueryMetrics::default())),
            network_monitor: NetworkMonitor::new(),
            cost_estimator: CostEstimator::new(),
        }
    }
    
    /// Analyze the query and available data to determine the best execution strategy
    pub fn determine_strategy<T>(&self, context: QueryContext<T>) -> QueryStrategy 
    where
        T: SortableEntity
    {
        // Analyze data completeness
        let data_completeness = DataCompletenessAnalyzer::analyze(&context);
        
        // Analyze query complexity
        let query_complexity = QueryComplexityAnalyzer::analyze(&context.query);
        
        // Check network quality
        let network_quality = self.network_monitor.current_quality();
        
        // Estimate costs for different strategies
        let client_cost = self.cost_estimator.estimate_client_cost(
            &context,
            data_completeness,
            query_complexity,
        );
        
        let server_cost = self.cost_estimator.estimate_server_cost(
            query_complexity,
            network_quality,
            &self.metrics.read().unwrap(),
        );
        
        // Select the best strategy based on costs and constraints
        self.select_strategy(
            client_cost,
            server_cost,
            data_completeness,
            network_quality,
            query_complexity,
        )
    }
    
    /// Select the optimal execution strategy based on analyzed factors
    fn select_strategy(
        &self,
        client_cost: u64,
        server_cost: u64,
        data_completeness: DataCompleteness,
        network_quality: NetworkQuality,
        query_complexity: QueryComplexity,
    ) -> QueryStrategy {
        let execution_mode = match (data_completeness, network_quality, query_complexity) {
            // Obvious client-side cases
            (DataCompleteness::High, _, QueryComplexity::Simple) => ExecutionMode::ClientOnly,
            (_, NetworkQuality::Offline, _) => ExecutionMode::ClientOnly,
            
            // Obvious server-side cases
            (DataCompleteness::Low, NetworkQuality::Excellent, _) => ExecutionMode::ServerOnly,
            (_, NetworkQuality::Excellent, QueryComplexity::Complex) => ExecutionMode::ServerOnly,
            
            // Hybrid strategies
            (DataCompleteness::Medium, NetworkQuality::Good, QueryComplexity::Moderate) => {
                if client_cost < server_cost {
                    ExecutionMode::HybridClientFilter
                } else {
                    ExecutionMode::HybridServerFilter
                }
            }
            
            // Race condition for uncertain cases
            _ if client_cost.abs_diff(server_cost) < self.config.parallel_race_threshold_ms => {
                ExecutionMode::ParallelRace
            }
            
            // Default to lowest cost
            _ => {
                if client_cost < server_cost {
                    ExecutionMode::ClientOnly
                } else {
                    ExecutionMode::ServerOnly
                }
            }
        };
        
        let confidence = self.calculate_confidence(
            client_cost,
            server_cost,
            data_completeness,
            network_quality,
        );
        
        QueryStrategy {
            execution_mode,
            confidence,
            estimated_latency_ms: client_cost.min(server_cost),
            reasoning: self.generate_reasoning(
                execution_mode,
                data_completeness,
                network_quality,
                query_complexity,
            ),
        }
    }
    
    /// Calculate confidence in the selected strategy
    fn calculate_confidence(
        &self,
        client_cost: u64,
        server_cost: u64,
        data_completeness: DataCompleteness,
        network_quality: NetworkQuality,
    ) -> f32 {
        let cost_difference = (client_cost as f32 - server_cost as f32).abs();
        let max_cost = client_cost.max(server_cost) as f32;
        let cost_confidence = if max_cost > 0.0 {
            cost_difference / max_cost
        } else {
            0.5
        };
        
        let data_confidence = match data_completeness {
            DataCompleteness::High => 0.9,
            DataCompleteness::Medium => 0.6,
            DataCompleteness::Low => 0.3,
        };
        
        let network_confidence = match network_quality {
            NetworkQuality::Excellent => 0.9,
            NetworkQuality::Good => 0.7,
            NetworkQuality::Poor => 0.4,
            NetworkQuality::Offline => 1.0, // Very confident we need client-side
        };
        
        // Weighted average
        (cost_confidence * 0.4 + data_confidence * 0.3 + network_confidence * 0.3)
            .min(1.0)
            .max(0.0)
    }
    
    /// Generate human-readable reasoning for the strategy selection
    fn generate_reasoning(
        &self,
        mode: ExecutionMode,
        data_completeness: DataCompleteness,
        network_quality: NetworkQuality,
        query_complexity: QueryComplexity,
    ) -> String {
        match mode {
            ExecutionMode::ClientOnly => {
                format!(
                    "Using client-side execution: {:?} data completeness, {:?} network, {:?} query",
                    data_completeness, network_quality, query_complexity
                )
            }
            ExecutionMode::ServerOnly => {
                format!(
                    "Using server-side execution: {:?} data completeness, {:?} network, {:?} query",
                    data_completeness, network_quality, query_complexity
                )
            }
            ExecutionMode::HybridClientFilter => {
                "Using hybrid approach: client-side filtering with server-side sorting".to_string()
            }
            ExecutionMode::HybridServerFilter => {
                "Using hybrid approach: server-side filtering with client-side sorting".to_string()
            }
            ExecutionMode::ParallelRace => {
                "Running parallel race: trying both client and server, using fastest result".to_string()
            }
        }
    }
    
    /// Update metrics after query execution
    pub fn record_execution(
        &self,
        mode: ExecutionMode,
        execution_time_ms: u64,
        success: bool,
        dataset_size: usize,
    ) {
        let mut metrics = self.metrics.write().unwrap();
        
        // Update success rates
        let entry = metrics.strategy_success_rates.entry(mode).or_insert(0.0);
        *entry = (*entry * 0.9) + (if success { 0.1 } else { 0.0 }); // Exponential moving average
        
        // Record timing data
        if mode == ExecutionMode::ClientOnly || mode == ExecutionMode::HybridClientFilter {
            metrics.client_sort_times.push((dataset_size, execution_time_ms));
            
            // Keep only last 100 measurements
            if metrics.client_sort_times.len() > 100 {
                metrics.client_sort_times.remove(0);
            }
        }
    }
}

impl Default for DecisionEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod basic_tests {
    use super::*;
    
    #[test]
    fn test_decision_engine_creation() {
        let engine = DecisionEngine::new();
        let custom_config = StrategyConfig {
            min_metadata_completeness: 0.7,
            ..Default::default()
        };
        let custom_engine = DecisionEngine::with_config(custom_config);
        
        // Basic sanity check
        assert_eq!(custom_engine.config.min_metadata_completeness, 0.7);
    }
}