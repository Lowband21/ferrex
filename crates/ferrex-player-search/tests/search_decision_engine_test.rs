use ferrex_core::query::types::SearchField;
use ferrex_player_search::metrics::SearchPerformanceMetrics;
use ferrex_player_search::types::{SearchDecisionEngine, SearchStrategy};
use std::time::{Duration, Instant};

#[test]
fn simple_decision_engine_selects_expected_strategy() {
    assert_eq!(
        SearchDecisionEngine::determine_strategy(
            "test query",
            0.5,
            false,
            false
        ),
        SearchStrategy::Client
    );
    assert_eq!(
        SearchDecisionEngine::determine_strategy("test query", 0.5, true, true),
        SearchStrategy::Server
    );
    assert_eq!(
        SearchDecisionEngine::determine_strategy(
            "test query",
            0.9,
            false,
            true
        ),
        SearchStrategy::Client
    );
}

#[test]
fn enhanced_decision_engine_prefers_faster_history() {
    let mut engine = SearchDecisionEngine::new_with_metrics();

    for i in 0..5 {
        engine.record_execution(SearchPerformanceMetrics {
            strategy: SearchStrategy::Client,
            query_length: 10,
            field_count: 1,
            execution_time: Duration::from_millis(50 + i * 10),
            result_count: 10,
            success: true,
            network_latency: None,
            timestamp: Instant::now(),
        });
    }

    for i in 0..5 {
        engine.record_execution(SearchPerformanceMetrics {
            strategy: SearchStrategy::Server,
            query_length: 10,
            field_count: 1,
            execution_time: Duration::from_millis(200 + i * 20),
            result_count: 10,
            success: true,
            network_latency: Some(Duration::from_millis(150)),
            timestamp: Instant::now(),
        });
    }

    let strategy = engine.determine_strategy_enhanced(
        "test query",
        0.5,
        &[SearchField::Title],
        true,
    );

    assert_eq!(strategy, SearchStrategy::Client);
}

#[test]
fn network_failures_push_search_to_client() {
    let mut engine = SearchDecisionEngine::new_with_metrics();

    for _ in 0..3 {
        engine.record_network_failure();
        engine.record_execution(SearchPerformanceMetrics {
            strategy: SearchStrategy::Server,
            query_length: 10,
            field_count: 1,
            execution_time: Duration::from_millis(5000),
            result_count: 0,
            success: false,
            network_latency: None,
            timestamp: Instant::now(),
        });
    }

    let strategy = engine.determine_strategy_enhanced(
        "test query",
        0.5,
        &[SearchField::Title],
        true,
    );

    assert_eq!(strategy, SearchStrategy::Client);
}

#[test]
fn complex_query_detection_covers_fields_and_operators() {
    assert!(!SearchDecisionEngine::is_complex_query(
        "simple",
        &[SearchField::Title]
    ));
    assert!(SearchDecisionEngine::is_complex_query(
        "test",
        &[SearchField::Title, SearchField::Overview]
    ));
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
    assert!(SearchDecisionEngine::is_complex_query(
        "test",
        &[SearchField::Cast]
    ));
    assert!(SearchDecisionEngine::is_complex_query(
        "test",
        &[SearchField::Crew]
    ));
}
