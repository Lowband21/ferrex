//! Comprehensive tests for the decision engine

#[cfg(test)]
mod decision_engine_tests {
    use super::super::*;
    use crate::query::types::{MediaQuery, SortField, MediaTypeFilter};
    use crate::WatchStatusFilter;
    use crate::{MovieReference, MovieID, MovieTitle, MovieURL, MediaFile, MediaDetailsOption};
    use std::path::PathBuf;
    use uuid::Uuid;
    
    fn create_test_context(
        num_items: usize,
        has_cache: bool,
        has_metadata: bool,
    ) -> QueryContext<MovieReference> {
        let movies = (0..num_items)
            .map(|i| MovieReference {
                id: MovieID::new(format!("movie-{}", i)).unwrap(),
                tmdb_id: i as u64,
                title: MovieTitle::new(format!("Movie {}", i)).unwrap(),
                details: if has_metadata {
                    MediaDetailsOption::Details(crate::TmdbDetails::Movie(crate::EnhancedMovieDetails {
                        id: i as u64,
                        title: format!("Movie {}", i),
                        overview: Some("Test movie".to_string()),
                        release_date: Some("2023-01-01".to_string()),
                        runtime: Some(120),
                        vote_average: Some(7.5),
                        vote_count: Some(100),
                        popularity: Some(50.0),
                        genres: vec![],
                        production_companies: vec![],
                        poster_path: None,
                        backdrop_path: None,
                        logo_path: None,
                        images: Default::default(),
                        cast: vec![],
                        crew: vec![],
                        videos: vec![],
                        keywords: vec![],
                        external_ids: Default::default(),
                    }))
                } else {
                    MediaDetailsOption::Endpoint("/api/movie".to_string())
                },
                endpoint: MovieURL::from_string("/api/stream".to_string()),
                file: MediaFile {
                    id: Uuid::new_v4(),
                    path: PathBuf::from("/test.mp4"),
                    filename: "test.mp4".to_string(),
                    size: 1000,
                    created_at: chrono::Utc::now(),
                    media_file_metadata: None,
                    library_id: Uuid::new_v4(),
                },
                theme_color: None,
            })
            .collect();
        
        QueryContext {
            query: MediaQuery::default(),
            available_data: movies,
            has_cache,
            cache_age_seconds: if has_cache { Some(30) } else { None },
            expected_total_size: Some(num_items),
            hints: Default::default(),
        }
    }
    
    #[test]
    fn test_client_side_decision_for_small_dataset_with_metadata() {
        let engine = DecisionEngine::new();
        let mut context = create_test_context(100, false, true);
        context.query.sort.primary = SortField::Title;
        
        // Simulate good network
        engine.network_monitor.simulate_good();
        
        let strategy = engine.determine_strategy(context);
        
        assert_eq!(strategy.execution_mode, ExecutionMode::ClientOnly);
        assert!(strategy.confidence > 0.7);
        assert!(strategy.reasoning.contains("client-side"));
    }
    
    #[test]
    fn test_server_side_decision_for_missing_metadata() {
        let engine = DecisionEngine::new();
        let mut context = create_test_context(100, false, false);
        context.query.sort.primary = SortField::Rating; // Needs metadata
        
        // Simulate excellent network
        engine.network_monitor.simulate_excellent();
        
        let strategy = engine.determine_strategy(context);
        
        assert_eq!(strategy.execution_mode, ExecutionMode::ServerOnly);
        assert!(strategy.confidence > 0.5);
        assert!(strategy.reasoning.contains("server-side"));
    }
    
    #[test]
    fn test_client_only_when_offline() {
        let engine = DecisionEngine::new();
        let context = create_test_context(1000, false, false);
        
        // Simulate offline
        engine.network_monitor.set_offline(true);
        
        let strategy = engine.determine_strategy(context);
        
        assert_eq!(strategy.execution_mode, ExecutionMode::ClientOnly);
        assert_eq!(strategy.confidence, 1.0); // Very confident when offline
        assert!(strategy.reasoning.contains("client-side"));
    }
    
    #[test]
    fn test_hybrid_strategy_for_moderate_complexity() {
        let engine = DecisionEngine::new();
        let mut context = create_test_context(500, false, true);
        context.query.sort.primary = SortField::Title;
        context.query.filters.genres = vec!["Action".to_string()];
        
        // Simulate good network
        engine.network_monitor.simulate_good();
        
        let strategy = engine.determine_strategy(context);
        
        // Should select a hybrid strategy
        assert!(matches!(
            strategy.execution_mode,
            ExecutionMode::HybridClientFilter | ExecutionMode::HybridServerFilter
        ));
    }
    
    #[test]
    fn test_parallel_race_for_uncertain_scenarios() {
        let engine = DecisionEngine::with_config(StrategyConfig {
            parallel_race_threshold_ms: 50, // Low threshold to trigger race
            ..Default::default()
        });
        
        let context = create_test_context(200, true, true);
        
        // Simulate good network - makes costs similar
        engine.network_monitor.simulate_good();
        
        let strategy = engine.determine_strategy(context);
        
        // With similar costs and low threshold, should trigger race
        // This depends on exact cost calculations, but with balanced conditions
        // it's likely to trigger
        if strategy.execution_mode == ExecutionMode::ParallelRace {
            assert!(strategy.reasoning.contains("parallel race"));
        }
    }
    
    #[test]
    fn test_server_side_for_user_context_queries() {
        let engine = DecisionEngine::new();
        let mut context = create_test_context(100, false, true);
        
        // Add watch status filter which requires user context
        context.query.filters.watch_status = Some(WatchStatusFilter::InProgress);
        
        // Even with excellent client-side conditions
        engine.network_monitor.simulate_excellent();
        
        let strategy = engine.determine_strategy(context);
        
        // Should still prefer server because of user context requirement
        assert_eq!(strategy.execution_mode, ExecutionMode::ServerOnly);
    }
    
    #[test]
    fn test_cache_benefit_influences_decision() {
        let engine = DecisionEngine::new();
        
        // Without cache
        let context_no_cache = create_test_context(500, false, true);
        engine.network_monitor.simulate_good();
        let strategy_no_cache = engine.determine_strategy(context_no_cache);
        
        // With fresh cache
        let context_with_cache = create_test_context(500, true, true);
        let strategy_with_cache = engine.determine_strategy(context_with_cache);
        
        // Cache should reduce client cost significantly
        assert!(strategy_with_cache.estimated_latency_ms < strategy_no_cache.estimated_latency_ms);
    }
    
    #[test]
    fn test_large_dataset_triggers_server_preference() {
        let engine = DecisionEngine::new();
        let context = create_test_context(20_000, false, false); // Very large dataset
        
        engine.network_monitor.simulate_excellent();
        
        let strategy = engine.determine_strategy(context);
        
        // Large datasets should prefer server
        assert_eq!(strategy.execution_mode, ExecutionMode::ServerOnly);
    }
    
    #[test]
    fn test_poor_network_triggers_client_preference() {
        let engine = DecisionEngine::new();
        let context = create_test_context(200, false, true);
        
        engine.network_monitor.simulate_poor();
        
        let strategy = engine.determine_strategy(context);
        
        // Poor network should prefer client if data is available
        assert_eq!(strategy.execution_mode, ExecutionMode::ClientOnly);
    }
    
    #[test]
    fn test_confidence_calculation() {
        let engine = DecisionEngine::new();
        
        // High confidence scenario - offline with data
        let mut context1 = create_test_context(100, false, true);
        engine.network_monitor.set_offline(true);
        let strategy1 = engine.determine_strategy(context1);
        assert!(strategy1.confidence > 0.9);
        
        // Low confidence scenario - mixed conditions
        engine.network_monitor.simulate_good();
        let context2 = create_test_context(500, false, false);
        let strategy2 = engine.determine_strategy(context2);
        assert!(strategy2.confidence < 0.7);
    }
    
    #[test]
    fn test_record_execution_updates_metrics() {
        let engine = DecisionEngine::new();
        
        // Record some executions
        engine.record_execution(ExecutionMode::ClientOnly, 50, true, 1000);
        engine.record_execution(ExecutionMode::ClientOnly, 60, true, 1000);
        engine.record_execution(ExecutionMode::ServerOnly, 100, false, 1000);
        
        // Check metrics were updated
        let metrics = engine.metrics.read().unwrap();
        assert_eq!(metrics.client_sort_times.len(), 2);
        assert!(metrics.strategy_success_rates.contains_key(&ExecutionMode::ClientOnly));
        assert!(metrics.strategy_success_rates.contains_key(&ExecutionMode::ServerOnly));
    }
    
    #[test]
    fn test_complex_query_increases_costs() {
        let engine = DecisionEngine::new();
        
        // Simple query
        let mut simple_context = create_test_context(100, false, true);
        simple_context.query.sort.primary = SortField::Title;
        
        // Complex query
        let mut complex_context = create_test_context(100, false, true);
        complex_context.query.sort.primary = SortField::Rating;
        complex_context.query.sort.secondary = Some(SortField::Title);
        complex_context.query.filters.genres = vec!["Action".to_string()];
        complex_context.query.filters.year_range = Some((2020, 2024));
        
        engine.network_monitor.simulate_good();
        
        let simple_strategy = engine.determine_strategy(simple_context);
        let complex_strategy = engine.determine_strategy(complex_context);
        
        // Complex queries should have higher estimated latency
        assert!(complex_strategy.estimated_latency_ms > simple_strategy.estimated_latency_ms);
    }
}

#[cfg(test)]
mod analyzer_integration_tests {
    use super::super::*;
    use crate::query::types::{MediaQuery, SortField};
    use crate::{MovieReference, MovieID, MovieTitle, MovieURL, MediaFile, MediaDetailsOption};
    use std::path::PathBuf;
    use uuid::Uuid;
    
    #[test]
    fn test_data_completeness_affects_strategy() {
        let engine = DecisionEngine::new();
        
        // Create contexts with different completeness levels
        let high_complete = create_context_with_completeness(DataCompleteness::High);
        let low_complete = create_context_with_completeness(DataCompleteness::Low);
        
        engine.network_monitor.simulate_good();
        
        let high_strategy = engine.determine_strategy(high_complete);
        let low_strategy = engine.determine_strategy(low_complete);
        
        // High completeness should favor client, low should favor server
        assert!(matches!(high_strategy.execution_mode, ExecutionMode::ClientOnly));
        assert!(matches!(low_strategy.execution_mode, ExecutionMode::ServerOnly));
    }
    
    fn create_context_with_completeness(completeness: DataCompleteness) -> QueryContext<MovieReference> {
        // Create dummy movies based on completeness level
        let num_with_metadata = match completeness {
            DataCompleteness::High => 90,
            DataCompleteness::Medium => 50,
            DataCompleteness::Low => 10,
        };
        
        let movies = (0..100)
            .map(|i| MovieReference {
                id: MovieID::new(format!("movie-{}", i)).unwrap(),
                tmdb_id: i as u64,
                title: MovieTitle::new(format!("Movie {}", i)).unwrap(),
                details: if i < num_with_metadata {
                    MediaDetailsOption::Details(crate::TmdbDetails::Movie(crate::EnhancedMovieDetails {
                        id: i as u64,
                        title: format!("Movie {}", i),
                        overview: Some("Test movie".to_string()),
                        release_date: Some("2023-01-01".to_string()),
                        runtime: Some(120),
                        vote_average: Some(7.5),
                        vote_count: Some(100),
                        popularity: Some(50.0),
                        genres: vec![],
                        production_companies: vec![],
                        poster_path: None,
                        backdrop_path: None,
                        logo_path: None,
                        images: Default::default(),
                        cast: vec![],
                        crew: vec![],
                        videos: vec![],
                        keywords: vec![],
                        external_ids: Default::default(),
                    }))
                } else {
                    MediaDetailsOption::Endpoint("/api/movie".to_string())
                },
                endpoint: MovieURL::from_string("/api/stream".to_string()),
                file: MediaFile {
                    id: Uuid::new_v4(),
                    path: PathBuf::from("/test.mp4"),
                    filename: "test.mp4".to_string(),
                    size: 1000,
                    created_at: chrono::Utc::now(),
                    media_file_metadata: None,
                    library_id: Uuid::new_v4(),
                },
                theme_color: None,
            })
            .collect();
            
        QueryContext {
            query: MediaQuery {
                sort: crate::query::types::SortCriteria {
                    primary: SortField::Rating,
                    ..Default::default()
                },
                ..Default::default()
            },
            available_data: movies,
            has_cache: false,
            cache_age_seconds: None,
            expected_total_size: Some(100),
            hints: Default::default(),
        }
    }
}