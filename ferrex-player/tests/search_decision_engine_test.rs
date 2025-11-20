#[cfg(test)]
mod search_decision_engine_tests {
    use ferrex_core::query::types::SearchField;
    use ferrex_player::domains::search::metrics::SearchPerformanceMetrics;
    use ferrex_player::domains::search::types::{SearchDecisionEngine, SearchStrategy};
    use std::time::{Duration, Instant};

    #[test]
    fn test_simple_decision_engine() {
        let engine = SearchDecisionEngine::new_simple();

        // Test with no network
        let strategy = SearchDecisionEngine::determine_strategy(
            "test query",
            0.5,   // 50% data completeness
            false, // not complex
            false, // no network
        );
        assert_eq!(strategy, SearchStrategy::Client);

        // Test with complex query
        let strategy = SearchDecisionEngine::determine_strategy(
            "test query",
            0.5,
            true, // complex query
            true, // network available
        );
        assert_eq!(strategy, SearchStrategy::Server);

        // Test with good cache coverage
        let strategy = SearchDecisionEngine::determine_strategy(
            "test query",
            0.9, // 90% data completeness
            false,
            true,
        );
        assert_eq!(strategy, SearchStrategy::Client);
    }

    #[test]
    fn test_enhanced_decision_engine() {
        let mut engine = SearchDecisionEngine::new_with_metrics();

        // Record some successful client searches
        for i in 0..5 {
            let metric = SearchPerformanceMetrics {
                strategy: SearchStrategy::Client,
                query_length: 10,
                field_count: 1,
                execution_time: Duration::from_millis(50 + i * 10),
                result_count: 10,
                success: true,
                network_latency: None,
                timestamp: Instant::now(),
            };
            engine.record_execution(metric);
        }

        // Record some slower server searches
        for i in 0..5 {
            let metric = SearchPerformanceMetrics {
                strategy: SearchStrategy::Server,
                query_length: 10,
                field_count: 1,
                execution_time: Duration::from_millis(200 + i * 20),
                result_count: 10,
                success: true,
                network_latency: Some(Duration::from_millis(150)),
                timestamp: Instant::now(),
            };
            engine.record_execution(metric);
        }

        // Now the engine should prefer client based on historical performance
        let strategy =
            engine.determine_strategy_enhanced("test query", 0.5, &[SearchField::Title], true);

        // Should prefer client due to better performance history
        assert_eq!(strategy, SearchStrategy::Client);
    }

    #[test]
    fn test_network_failure_handling() {
        let mut engine = SearchDecisionEngine::new_with_metrics();

        // Record multiple server failures
        for _ in 0..3 {
            engine.record_network_failure();
            let metric = SearchPerformanceMetrics {
                strategy: SearchStrategy::Server,
                query_length: 10,
                field_count: 1,
                execution_time: Duration::from_millis(5000), // timeout
                result_count: 0,
                success: false,
                network_latency: None,
                timestamp: Instant::now(),
            };
            engine.record_execution(metric);
        }

        // Now the engine should avoid server
        let strategy =
            engine.determine_strategy_enhanced("test query", 0.5, &[SearchField::Title], true);

        // Should prefer client due to server failures
        assert_eq!(strategy, SearchStrategy::Client);
    }

    #[test]
    fn test_complex_query_detection() {
        // Test simple query
        assert!(!SearchDecisionEngine::is_complex_query(
            "simple",
            &[SearchField::Title]
        ));

        // Test complex query with multiple fields
        assert!(SearchDecisionEngine::is_complex_query(
            "test",
            &[SearchField::Title, SearchField::Overview]
        ));

        // Test complex query with operators
        assert!(SearchDecisionEngine::is_complex_query(
            "test AND other",
            &[SearchField::Title]
        ));
        assert!(SearchDecisionEngine::is_complex_query(
            "test OR other",
            &[SearchField::Title]
        ));
        assert!(SearchDecisionEngine::is_complex_query(
            "\"exact match\"",
            &[SearchField::Title]
        ));

        // Test complex query with cast/crew fields
        assert!(SearchDecisionEngine::is_complex_query(
            "test",
            &[SearchField::Cast]
        ));
        assert!(SearchDecisionEngine::is_complex_query(
            "test",
            &[SearchField::Crew]
        ));
    }
}
