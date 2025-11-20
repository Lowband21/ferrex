//! Search domain types and state management

use ferrex_core::LibraryID;
use ferrex_core::query::types::SearchField;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::infra::api_types::Media;

pub const SEARCH_RESULTS_SCROLL_ID: &str = "search-window-results";

/// Search UI mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    /// Dropdown overlay below search bar
    Dropdown,
    /// Full screen search view
    FullScreen,
}

/// Search execution strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchStrategy {
    /// Local (client-side) search — reserved for future use
    Client,
    /// Query server directly (current behavior)
    Server,
    /// Hybrid approach — reserved for future evaluation
    Hybrid,
}

/// Individual search result with relevance info
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// The media reference
    pub media_ref: Media,
    /// Display title
    pub title: String,
    /// Subtitle (e.g., "Series • S01E05" or "2024 • Action")
    pub subtitle: Option<String>,
    /// Year of release
    pub year: Option<i32>,
    /// Poster URL if available
    pub poster_url: Option<String>,
    /// Match relevance score (0.0 - 1.0)
    pub match_score: f32,
    /// Which field matched the search
    pub match_field: SearchField,
    /// Library this result belongs to
    pub library_id: Option<LibraryID>,
}

/// Search cache entry
#[derive(Debug, Clone)]
pub struct CachedSearchResults {
    /// The cached results
    pub results: Vec<SearchResult>,
    /// When this cache entry was created
    pub timestamp: Instant,
    /// Total number of results (for pagination)
    pub total_count: usize,
    /// The query that produced these results
    pub query: String,
    /// Strategy used to get these results
    pub strategy: SearchStrategy,
}

/// Main search domain state
#[derive(Debug, Clone)]
pub struct SearchState {
    /// Current search query
    pub query: String,
    /// Previous query (for incremental search)
    pub previous_query: Option<String>,
    /// Current search results
    pub results: Vec<SearchResult>,
    /// Whether a search is in progress
    pub is_searching: bool,
    /// Current UI mode
    pub mode: SearchMode,
    /// Total result count (for pagination)
    pub total_results: usize,
    /// Number of results currently displayed
    pub displayed_results: usize,
    /// Results per page for pagination
    pub page_size: usize,
    /// Selected result index (for keyboard navigation)
    pub selected_index: Option<usize>,
    /// Current scroll offset of the search window results list (in pixels)
    pub window_scroll_offset: f32,
    /// Indicates that escape was pressed once and next press should close
    pub escape_pending: bool,
    /// Search result cache (query -> results)
    pub cache: HashMap<String, CachedSearchResults>,
    /// Cache TTL
    pub cache_ttl: Duration,
    /// Last search timestamp (for debouncing)
    pub last_search_time: Option<Instant>,
    /// Minimum time between searches
    pub debounce_duration: Duration,
    /// Current search strategy
    pub current_strategy: Option<SearchStrategy>,
    /// Error message if search failed
    pub error: Option<String>,
    /// Selected library for search scope
    pub library_id: Option<LibraryID>,
    /// Search fields to include
    pub search_fields: Vec<SearchField>,
    /// Enable fuzzy matching
    pub fuzzy_matching: bool,
    /// Decision engine for strategy selection
    pub decision_engine: SearchDecisionEngine,
    /// Last search performance metric (for recording)
    pub last_metric: Option<super::metrics::SearchPerformanceMetrics>,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            query: String::new(),
            previous_query: None,
            results: Vec::new(),
            is_searching: false,
            mode: SearchMode::Dropdown,
            total_results: 0,
            displayed_results: 0,
            page_size: 10, // Show 10 results in dropdown
            selected_index: None,
            window_scroll_offset: 0.0,
            escape_pending: false,
            cache: HashMap::new(),
            cache_ttl: Duration::from_secs(300), // 5 minute cache
            last_search_time: None,
            debounce_duration: Duration::from_millis(200),
            current_strategy: None,
            error: None,
            library_id: None,
            search_fields: vec![SearchField::Title], // Start with title-only search
            fuzzy_matching: true,
            decision_engine: SearchDecisionEngine::new_simple(), // Start with simple engine
            last_metric: None,
        }
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
impl SearchState {
    /// Check if we should execute a new search (debouncing)
    pub fn should_search(&self) -> bool {
        if self.query.is_empty() {
            return false;
        }

        match self.last_search_time {
            None => true,
            Some(last_time) => last_time.elapsed() >= self.debounce_duration,
        }
    }

    /// Get cached results if available and not expired
    pub fn get_cached_results(
        &self,
        query: &str,
    ) -> Option<&CachedSearchResults> {
        self.cache.get(query).and_then(|cached| {
            if cached.timestamp.elapsed() < self.cache_ttl {
                Some(cached)
            } else {
                None
            }
        })
    }

    /// Cache search results
    pub fn cache_results(
        &mut self,
        query: String,
        results: Vec<SearchResult>,
        strategy: SearchStrategy,
    ) {
        let total_count = results.len();
        self.cache.insert(
            query.clone(),
            CachedSearchResults {
                results,
                timestamp: Instant::now(),
                total_count,
                query,
                strategy,
            },
        );
    }

    /// Clear search state
    pub fn clear(&mut self) {
        self.query.clear();
        self.previous_query = None;
        self.results.clear();
        self.is_searching = false;
        self.selected_index = None;
        self.window_scroll_offset = 0.0;
        self.error = None;
        self.total_results = 0;
        self.displayed_results = 0;
        self.escape_pending = false;
    }

    /// Navigate selection up
    pub fn select_previous(&mut self) {
        if self.results.is_empty() {
            return;
        }

        self.selected_index = match self.selected_index {
            None => Some(self.results.len() - 1),
            Some(0) => Some(self.results.len() - 1),
            Some(i) => Some(i - 1),
        };
    }

    /// Navigate selection down
    pub fn select_next(&mut self) {
        if self.results.is_empty() {
            return;
        }

        self.selected_index = match self.selected_index {
            None => Some(0),
            Some(i) if i >= self.results.len() - 1 => Some(0),
            Some(i) => Some(i + 1),
        };
    }

    /// Get currently selected result
    pub fn get_selected(&self) -> Option<&SearchResult> {
        self.selected_index.and_then(|i| self.results.get(i))
    }

    /// Set search mode
    pub fn set_mode(&mut self, mode: SearchMode) {
        self.mode = mode;

        // Adjust page size based on mode
        self.page_size = match mode {
            SearchMode::Dropdown => 10,
            SearchMode::FullScreen => 50,
        };
        self.escape_pending = false;
    }
}

/// Search decision engine that determines execution strategy
#[derive(Debug, Clone)]
pub struct SearchDecisionEngine {
    /// Optional performance metrics for informed decisions
    metrics: Option<Box<super::metrics::PerformanceHistory>>,
    /// Optional calibration results
    calibration: Option<super::calibrator::CalibrationResults>,
    /// Network monitor
    network_monitor: Option<Box<super::metrics::NetworkMonitor>>,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl SearchDecisionEngine {
    /// Create a new simple decision engine (no metrics)
    pub fn new_simple() -> Self {
        Self {
            metrics: None,
            calibration: None,
            network_monitor: None,
        }
    }

    /// Create an enhanced decision engine with metrics
    pub fn new_with_metrics() -> Self {
        Self {
            metrics: Some(Box::new(super::metrics::PerformanceHistory::new())),
            calibration: None,
            network_monitor: Some(Box::new(
                super::metrics::NetworkMonitor::new(),
            )),
        }
    }

    /// Update calibration results
    pub fn set_calibration(
        &mut self,
        calibration: super::calibrator::CalibrationResults,
    ) {
        self.calibration = Some(calibration);
    }

    /// Record a search execution for learning
    pub fn record_execution(
        &mut self,
        metric: super::metrics::SearchPerformanceMetrics,
    ) {
        if let Some(ref mut metrics) = self.metrics {
            metrics.add_metric(metric);
        }
    }

    /// Record network success
    pub fn record_network_success(&mut self, latency: std::time::Duration) {
        if let Some(ref mut monitor) = self.network_monitor {
            monitor.record_success(latency);
        }
    }

    /// Record network failure
    pub fn record_network_failure(&mut self) {
        if let Some(ref mut monitor) = self.network_monitor {
            monitor.record_failure();
        }
    }

    /// Enhanced strategy determination using metrics if available
    pub fn determine_strategy_enhanced(
        &self,
        query: &str,
        data_completeness: f32,
        fields: &[SearchField],
        network_available: bool,
    ) -> SearchStrategy {
        let is_complex = Self::is_complex_query(query, fields);

        // If we have metrics and calibration, use enhanced decision
        if let Some(ref metrics) = self.metrics {
            // Check recent failures
            if metrics.has_recent_failures(SearchStrategy::Server, 0.5) {
                // Server is failing, prefer client
                return SearchStrategy::Client;
            }

            // Use calibration if available
            if let Some(ref calibration) = self.calibration {
                // If calibration strongly prefers one strategy, use it
                if let Some(client_ms) = calibration.client_baseline_ms
                    && let Some(server_ms) = calibration.server_baseline_ms
                {
                    // Strong preference based on 3x performance difference
                    if client_ms * 3 < server_ms {
                        return SearchStrategy::Client;
                    } else if server_ms * 3 < client_ms {
                        return SearchStrategy::Server;
                    }
                }
            }

            // Check network quality
            if let Some(ref monitor) = self.network_monitor
                && monitor.should_prefer_client()
            {
                return SearchStrategy::Client;
            }

            // Use historical performance data
            let client_avg =
                metrics.get_average_execution_time(SearchStrategy::Client);
            let server_avg =
                metrics.get_average_execution_time(SearchStrategy::Server);

            match (client_avg, server_avg) {
                (Some(client), Some(server)) => {
                    // Prefer faster strategy with 20% margin
                    if client.as_millis() * 120 < server.as_millis() * 100 {
                        SearchStrategy::Client
                    } else if server.as_millis() * 120
                        < client.as_millis() * 100
                    {
                        SearchStrategy::Server
                    } else {
                        SearchStrategy::Hybrid
                    }
                }
                _ => {
                    // Fall back to simple heuristics
                    Self::determine_strategy(
                        query,
                        data_completeness,
                        is_complex,
                        network_available,
                    )
                }
            }
        } else {
            // Use simple heuristics
            Self::determine_strategy(
                query,
                data_completeness,
                is_complex,
                network_available,
            )
        }
    }

    /// Simple strategy determination (backward compatible)
    pub fn determine_strategy(
        query: &str,
        data_completeness: f32,
        is_complex_query: bool,
        network_available: bool,
    ) -> SearchStrategy {
        // Simple heuristics for now
        if query.len() < 3 {
            // Short queries are fast on client
            SearchStrategy::Client
        } else if !network_available {
            // No network, must use client
            SearchStrategy::Client
        } else if is_complex_query {
            // Complex queries need server
            SearchStrategy::Server
        } else if data_completeness > 0.8 {
            // Good cache coverage, use client
            SearchStrategy::Client
        } else {
            // Mixed scenario, use hybrid
            SearchStrategy::Hybrid
        }
    }

    /// Check if a query is complex (needs server processing)
    pub fn is_complex_query(query: &str, fields: &[SearchField]) -> bool {
        // Complex if searching multiple fields or using special operators
        fields.len() > 1
            || fields.contains(&SearchField::Cast)
            || fields.contains(&SearchField::Crew)
            || query.contains("AND")
            || query.contains("OR")
            || query.contains("\"")
    }
}
