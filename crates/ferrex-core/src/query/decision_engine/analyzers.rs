//! Analyzers for data completeness and query complexity

use crate::query::types::{MediaQuery, SortBy};

/// Analyzer for query complexity
#[derive(Debug, Default, Clone, Copy)]
pub struct QueryComplexityAnalyzer;

impl QueryComplexityAnalyzer {
    /// Analyze the complexity of a query
    pub fn analyze(query: &MediaQuery) -> super::QueryComplexity {
        let mut complexity_score = 0;

        // Check sort complexity
        complexity_score += match query.sort.primary {
            SortBy::Title | SortBy::DateAdded | SortBy::CreatedAt => 1,
            SortBy::ReleaseDate | SortBy::Rating => 2,
            SortBy::LastWatched | SortBy::WatchProgress => 3,
            _ => 2,
        };

        // Secondary sort adds complexity
        if query.sort.secondary.is_some() {
            complexity_score += 2;
        }

        // Check filter complexity
        if !query.filters.library_ids.is_empty() {
            complexity_score += 1;
        }
        if query.filters.media_type.is_some() {
            complexity_score += 1;
        }
        if !query.filters.genres.is_empty() {
            complexity_score += 2;
        }
        if query.filters.year_range.is_some() {
            complexity_score += 2;
        }
        if query.filters.rating_range.is_some() {
            complexity_score += 2;
        }
        if query.filters.watch_status.is_some() {
            complexity_score += 3; // Requires user context
        }

        // Check search complexity
        if query.search.is_some() {
            complexity_score += 3; // Text search is complex
        }

        // Map score to complexity level
        match complexity_score {
            0..=3 => super::QueryComplexity::Simple,
            4..=7 => super::QueryComplexity::Moderate,
            _ => super::QueryComplexity::Complex,
        }
    }

    /// Check if a query can be efficiently executed client-side
    pub fn can_execute_client_side(query: &MediaQuery) -> bool {
        // Queries requiring user context generally need server
        if query.filters.watch_status.is_some() {
            return false;
        }

        // Queries with LastWatched or WatchProgress sort need server
        if matches!(
            query.sort.primary,
            SortBy::LastWatched | SortBy::WatchProgress
        ) {
            return false;
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        api::types::ScalarRange,
        domain::watch::WatchStatusFilter,
        query::types::{MediaTypeFilter, SearchField, SearchQuery},
    };
    use uuid::Uuid;

    #[test]
    fn test_query_complexity_analysis() {
        // Simple query
        let simple_query = MediaQuery::default();
        assert_eq!(
            QueryComplexityAnalyzer::analyze(&simple_query),
            super::super::QueryComplexity::Simple
        );

        // Moderate query
        let mut moderate_query = MediaQuery::default();
        moderate_query.sort.primary = SortBy::Title;
        moderate_query.sort.secondary = Some(SortBy::Rating);
        moderate_query.filters.library_ids = vec![Uuid::now_v7()];
        moderate_query.filters.media_type = Some(MediaTypeFilter::Movie);

        assert_eq!(
            QueryComplexityAnalyzer::analyze(&moderate_query),
            super::super::QueryComplexity::Moderate
        );

        // Complex query
        let mut complex_query = moderate_query.clone();
        complex_query.filters.genres = vec!["Action".to_string()];
        complex_query.filters.year_range = Some(ScalarRange::new(2020, 2024));
        complex_query.filters.watch_status =
            Some(WatchStatusFilter::InProgress);
        complex_query.search = Some(SearchQuery {
            text: "test".to_string(),
            fields: vec![SearchField::Title],
            fuzzy: false,
        });

        assert_eq!(
            QueryComplexityAnalyzer::analyze(&complex_query),
            super::super::QueryComplexity::Complex
        );
    }
}
