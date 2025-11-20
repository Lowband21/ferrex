use crate::{
    MediaError, Result,
    api_types::ScalarRange,
    query::{
        MediaQuery,
        decision_engine::{QueryComplexity, QueryComplexityAnalyzer},
    },
};
use tracing::{debug, warn};

/// Configuration for query complexity limits
#[derive(Debug, Clone)]
pub struct ComplexityConfig {
    /// Maximum allowed complexity score
    pub max_complexity_score: u32,

    /// Whether to allow complex queries during peak hours
    pub restrict_peak_hours: bool,

    /// Peak hours start (24-hour format)
    pub peak_hours_start: u8,

    /// Peak hours end (24-hour format)
    pub peak_hours_end: u8,

    /// Maximum complexity during peak hours
    pub peak_hours_max_complexity: u32,

    /// Maximum result set size
    pub max_result_size: usize,
}

impl Default for ComplexityConfig {
    fn default() -> Self {
        Self {
            max_complexity_score: 15,
            restrict_peak_hours: false,
            peak_hours_start: 18,
            peak_hours_end: 22,
            peak_hours_max_complexity: 10,
            max_result_size: 10000,
        }
    }
}

/// Guard that enforces query complexity limits
#[derive(Debug, Clone)]
pub struct QueryComplexityGuard {
    config: ComplexityConfig,
}

impl QueryComplexityGuard {
    /// Create a new complexity guard with default configuration
    pub fn new() -> Self {
        Self::with_config(ComplexityConfig::default())
    }

    /// Create a new complexity guard with custom configuration
    pub fn with_config(config: ComplexityConfig) -> Self {
        Self { config }
    }

    /// Check if a query exceeds complexity limits
    pub fn check_query(&self, query: &MediaQuery) -> Result<()> {
        // Calculate the query complexity score
        let complexity_score = self.calculate_complexity_score(query);
        let complexity_level = QueryComplexityAnalyzer::analyze(query);

        debug!(
            "Query complexity check: score={}, level={:?}",
            complexity_score, complexity_level
        );

        // Check against maximum complexity
        let max_allowed = if self.config.restrict_peak_hours && self.is_peak_hours() {
            self.config.peak_hours_max_complexity
        } else {
            self.config.max_complexity_score
        };

        if complexity_score > max_allowed {
            warn!(
                "Query exceeds complexity limit: {} > {} (peak_hours={})",
                complexity_score,
                max_allowed,
                self.is_peak_hours()
            );
            return Err(MediaError::InvalidMedia(format!(
                "Query too complex: complexity score {} exceeds limit of {}",
                complexity_score, max_allowed
            )));
        }

        // Check result set size limits
        if query.pagination.limit > self.config.max_result_size {
            return Err(MediaError::InvalidMedia(format!(
                "Query requests too many results: {} exceeds limit of {}",
                query.pagination.limit, self.config.max_result_size
            )));
        }

        // Additional checks for specific expensive operations
        if let Some(search) = &query.search {
            if search.fuzzy && search.text.len() < 3 {
                return Err(MediaError::InvalidMedia(
                    "Fuzzy search requires at least 3 characters".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Calculate a numeric complexity score for the query
    fn calculate_complexity_score(&self, query: &MediaQuery) -> u32 {
        let mut score = 0;

        // Base cost
        score += 1;

        // Filter complexity
        if !query.filters.library_ids.is_empty() {
            score += query.filters.library_ids.len() as u32;
        }

        if query.filters.media_type.is_some() {
            score += 1;
        }

        if !query.filters.genres.is_empty() {
            score += query.filters.genres.len() as u32 * 2;
        }

        if query.filters.year_range.is_some() {
            score += 2;
        }

        if query.filters.rating_range.is_some() {
            score += 2;
        }

        if query.filters.watch_status.is_some() {
            score += 5; // Watch status requires joins
        }

        // Search complexity
        if let Some(search) = &query.search {
            if search.fuzzy {
                score += 10; // Fuzzy search is expensive
            } else {
                score += 5; // Regular search
            }

            score += search.fields.len() as u32 * 2;
        }

        // Sort complexity
        score += match query.sort.primary {
            crate::query::SortBy::Title | crate::query::SortBy::DateAdded => 1,
            crate::query::SortBy::ReleaseDate | crate::query::SortBy::Rating => 2,
            crate::query::SortBy::Runtime => 2,
            crate::query::SortBy::LastWatched | crate::query::SortBy::WatchProgress => 5,
            crate::query::SortBy::Popularity
            | crate::query::SortBy::Bitrate
            | crate::query::SortBy::FileSize
            | crate::query::SortBy::ContentRating
            | crate::query::SortBy::Resolution => 3,
        };

        if query.sort.secondary.is_some() {
            score += 3;
        }

        // Pagination impact (large offsets are expensive)
        if query.pagination.offset > 1000 {
            score += (query.pagination.offset / 1000) as u32;
        }

        score
    }

    /// Check if current time is within peak hours
    fn is_peak_hours(&self) -> bool {
        use chrono::{Local, Timelike};

        let current_hour = Local::now().hour() as u8;

        if self.config.peak_hours_start <= self.config.peak_hours_end {
            // Normal case: peak hours don't cross midnight
            current_hour >= self.config.peak_hours_start
                && current_hour < self.config.peak_hours_end
        } else {
            // Peak hours cross midnight
            current_hour >= self.config.peak_hours_start
                || current_hour < self.config.peak_hours_end
        }
    }

    /// Get a recommendation for simplifying a complex query
    pub fn suggest_simplification(&self, query: &MediaQuery) -> Vec<String> {
        let mut suggestions = Vec::new();

        if let Some(search) = &query.search {
            if search.fuzzy && search.fields.len() > 1 {
                suggestions.push(
                    "Consider searching only in specific fields instead of all fields".to_string(),
                );
            }
            if search.fuzzy {
                suggestions.push("Consider using exact search instead of fuzzy search".to_string());
            }
        }

        if query.filters.genres.len() > 3 {
            suggestions.push("Consider filtering by fewer genres".to_string());
        }

        if query.sort.secondary.is_some() {
            suggestions.push("Consider using only primary sort".to_string());
        }

        if query.pagination.offset > 1000 {
            suggestions
                .push("Consider using cursor-based pagination for large offsets".to_string());
        }

        if query.filters.watch_status.is_some() && !query.filters.library_ids.is_empty() {
            suggestions.push(
                "Consider removing library filter when using watch status filter".to_string(),
            );
        }

        suggestions
    }
}

impl Default for QueryComplexityGuard {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::*;
    use uuid::Uuid;

    #[test]
    fn test_simple_query_allowed() {
        let guard = QueryComplexityGuard::new();
        let query = MediaQuery {
            filters: MediaFilters::default(),
            sort: SortCriteria {
                primary: SortBy::Title,
                order: SortOrder::Ascending,
                secondary: None,
            },
            pagination: Pagination {
                offset: 0,
                limit: 50,
            },
            search: None,
            user_context: None,
        };

        assert!(guard.check_query(&query).is_ok());
    }

    #[test]
    fn test_complex_query_rejected() {
        let guard = QueryComplexityGuard::with_config(ComplexityConfig {
            max_complexity_score: 10,
            ..Default::default()
        });

        let query = MediaQuery {
            filters: MediaFilters {
                genres: vec![
                    "Action".to_string(),
                    "Drama".to_string(),
                    "Comedy".to_string(),
                ],
                year_range: Some(ScalarRange::new(2000, 2023)),
                rating_range: Some(ScalarRange::new(7.0, 10.0)),
                watch_status: Some(crate::watch_status::WatchStatusFilter::InProgress),
                ..Default::default()
            },
            sort: SortCriteria {
                primary: SortBy::LastWatched,
                order: SortOrder::Descending,
                secondary: Some(SortBy::Rating),
            },
            search: Some(SearchQuery {
                text: "test".to_string(),
                fields: vec![SearchField::All],
                fuzzy: true,
            }),
            pagination: Pagination {
                offset: 5000,
                limit: 100,
            },
            user_context: Some(Uuid::new_v4()),
        };

        assert!(guard.check_query(&query).is_err());
    }

    #[test]
    fn test_fuzzy_search_minimum_length() {
        let guard = QueryComplexityGuard::new();
        let query = MediaQuery {
            search: Some(SearchQuery {
                text: "ab".to_string(),
                fields: vec![SearchField::Title],
                fuzzy: true,
            }),
            ..Default::default()
        };

        let result = guard.check_query(&query);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("3 characters"));
    }

    #[test]
    fn test_result_size_limit() {
        let guard = QueryComplexityGuard::new();
        let query = MediaQuery {
            pagination: Pagination {
                offset: 0,
                limit: 20000, // Exceeds default max of 10000
            },
            ..Default::default()
        };

        let result = guard.check_query(&query);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too many results"));
    }

    #[test]
    fn test_simplification_suggestions() {
        let guard = QueryComplexityGuard::new();
        let query = MediaQuery {
            filters: MediaFilters {
                genres: vec![
                    "A".to_string(),
                    "B".to_string(),
                    "C".to_string(),
                    "D".to_string(),
                ],
                ..Default::default()
            },
            sort: SortCriteria {
                primary: SortBy::Title,
                order: SortOrder::Ascending,
                secondary: Some(SortBy::Rating),
            },
            search: Some(SearchQuery {
                text: "test".to_string(),
                fields: vec![SearchField::All],
                fuzzy: true,
            }),
            pagination: Pagination {
                offset: 2000,
                limit: 50,
            },
            ..Default::default()
        };

        let suggestions = guard.suggest_simplification(&query);
        assert!(!suggestions.is_empty());
        assert!(suggestions.iter().any(|s| s.contains("fewer genres")));
        assert!(suggestions.iter().any(|s| s.contains("primary sort")));
        assert!(
            suggestions
                .iter()
                .any(|s| s.contains("cursor-based pagination"))
        );
    }
}
