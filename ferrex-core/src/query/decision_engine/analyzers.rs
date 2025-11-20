//! Analyzers for data completeness and query complexity

use crate::query::decision_engine::types::QueryContext;
use crate::query::types::{MediaQuery, SortBy};
use crate::{Media, MediaDetailsOption, MovieReference, SeriesReference};

/// Levels of data completeness
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub enum DataCompleteness {
    /// Less than 30% of items have required metadata
    Low,

    /// 30-80% of items have required metadata
    Medium,

    /// More than 80% of items have required metadata
    High,
}

/// Analyzer for determining data completeness
#[derive(Debug, Default, Clone, Copy)]
pub struct DataCompletenessAnalyzer;

impl DataCompletenessAnalyzer {
    /// Analyze the completeness of available data for a query
    pub fn analyze<T>(context: &QueryContext<T>) -> DataCompleteness {
        if context.available_data.is_empty() {
            return DataCompleteness::Low;
        }

        // For now, we'll use a generic approach
        // In a real implementation, we'd check specific fields based on T
        let _sample_size = context.available_data.len().min(100);

        // Default to medium completeness
        // This would be overridden by specific implementations
        DataCompleteness::Medium
    }

    /// Analyze completeness for MovieReference data
    pub fn analyze_movies(movies: &[MovieReference], query: &MediaQuery) -> DataCompleteness {
        if movies.is_empty() {
            return DataCompleteness::Low;
        }

        let sample_size = movies.len().min(100);
        let sample = &movies[..sample_size];

        // Check what fields the query needs
        let needs_metadata = Self::query_needs_metadata(query);

        if !needs_metadata {
            // All movies have basic fields like title and file info
            return DataCompleteness::High;
        }

        // Count how many have TMDB details
        let with_details = sample
            .iter()
            .filter(|movie| matches!(&movie.details, MediaDetailsOption::Details(_)))
            .count();

        let completeness_ratio = with_details as f32 / sample_size as f32;

        match completeness_ratio {
            r if r < 0.3 => DataCompleteness::Low,
            r if r < 0.8 => DataCompleteness::Medium,
            _ => DataCompleteness::High,
        }
    }

    /// Analyze completeness for SeriesReference data
    pub fn analyze_series(series: &[SeriesReference], query: &MediaQuery) -> DataCompleteness {
        if series.is_empty() {
            return DataCompleteness::Low;
        }

        let sample_size = series.len().min(100);
        let sample = &series[..sample_size];

        let needs_metadata = Self::query_needs_metadata(query);

        if !needs_metadata {
            return DataCompleteness::High;
        }

        let with_details = sample
            .iter()
            .filter(|s| matches!(&s.details, MediaDetailsOption::Details(_)))
            .count();

        let completeness_ratio = with_details as f32 / sample_size as f32;

        match completeness_ratio {
            r if r < 0.3 => DataCompleteness::Low,
            r if r < 0.8 => DataCompleteness::Medium,
            _ => DataCompleteness::High,
        }
    }

    /// Analyze completeness for mixed Media data
    pub fn analyze_media_refs(refs: &[Media], query: &MediaQuery) -> DataCompleteness {
        if refs.is_empty() {
            return DataCompleteness::Low;
        }

        let sample_size = refs.len().min(100);
        let sample = &refs[..sample_size];

        let needs_metadata = Self::query_needs_metadata(query);

        if !needs_metadata {
            return DataCompleteness::High;
        }

        let with_details = sample
            .iter()
            .filter(|media_ref| match media_ref {
                Media::Movie(m) => matches!(&m.details, MediaDetailsOption::Details(_)),
                Media::Series(s) => matches!(&s.details, MediaDetailsOption::Details(_)),
                _ => false,
            })
            .count();

        let completeness_ratio = with_details as f32 / sample_size as f32;

        match completeness_ratio {
            r if r < 0.3 => DataCompleteness::Low,
            r if r < 0.8 => DataCompleteness::Medium,
            _ => DataCompleteness::High,
        }
    }

    /// Check if a query requires TMDB metadata
    fn query_needs_metadata(query: &MediaQuery) -> bool {
        // Check sort fields
        let sort_needs_metadata = matches!(
            query.sort.primary,
            SortBy::ReleaseDate | SortBy::Rating | SortBy::Runtime
        );

        // Check filters
        let filter_needs_metadata = !query.filters.genres.is_empty()
            || query.filters.year_range.is_some()
            || query.filters.rating_range.is_some();

        sort_needs_metadata || filter_needs_metadata
    }
}

/// Analyzer for query complexity
#[derive(Debug, Default, Clone, Copy)]
pub struct QueryComplexityAnalyzer;

impl QueryComplexityAnalyzer {
    /// Analyze the complexity of a query
    pub fn analyze(query: &MediaQuery) -> super::QueryComplexity {
        let mut complexity_score = 0;

        // Check sort complexity
        complexity_score += match query.sort.primary {
            SortBy::Title | SortBy::DateAdded => 1,
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
    use crate::{LibraryID, MediaFile, MovieID, MovieTitle, MovieURL, UrlLike};
    use std::path::PathBuf;
    use uuid::Uuid;

    fn create_test_movie(has_details: bool) -> MovieReference {
        MovieReference {
            id: MovieID::new(),
            library_id: LibraryID::new_uuid(),
            tmdb_id: 123,
            title: MovieTitle::new("Test Movie".to_string()).unwrap(),
            details: if has_details {
                MediaDetailsOption::Details(crate::TmdbDetails::Movie(
                    crate::EnhancedMovieDetails {
                        id: 123,
                        title: "Test Movie".to_string(),
                        overview: Some("A test movie".to_string()),
                        release_date: Some("2023-01-15".to_string()),
                        runtime: Some(120),
                        vote_average: Some(7.5),
                        vote_count: Some(100),
                        popularity: Some(50.0),
                        content_rating: None,
                        content_ratings: Vec::new(),
                        release_dates: Vec::new(),
                        genres: vec![],
                        spoken_languages: vec![],
                        production_companies: vec![],
                        production_countries: vec![],
                        homepage: None,
                        status: None,
                        tagline: None,
                        budget: None,
                        revenue: None,
                        poster_path: None,
                        backdrop_path: None,
                        logo_path: None,
                        images: Default::default(),
                        cast: vec![],
                        crew: vec![],
                        videos: vec![],
                        keywords: vec![],
                        external_ids: Default::default(),
                        alternative_titles: Vec::new(),
                        translations: Vec::new(),
                        collection: None,
                        recommendations: Vec::new(),
                        similar: Vec::new(),
                        original_title: None,
                    },
                ))
            } else {
                MediaDetailsOption::Endpoint("/movie/123".to_string())
            },
            endpoint: MovieURL::from_string("/stream/123".to_string()),
            file: MediaFile {
                id: Uuid::new_v4(),
                path: PathBuf::from("/test.mp4"),
                filename: "test.mp4".to_string(),
                size: 1000,
                created_at: chrono::Utc::now(),
                media_file_metadata: None,
                library_id: LibraryID::new_uuid(),
            },
            theme_color: None,
        }
    }

    #[test]
    fn test_data_completeness_analysis() {
        // All movies with details - High completeness
        let movies_with_details: Vec<MovieReference> =
            (0..10).map(|_| create_test_movie(true)).collect();

        let query = MediaQuery::default();
        let completeness = DataCompletenessAnalyzer::analyze_movies(&movies_with_details, &query);
        assert_eq!(completeness, DataCompleteness::High);

        // No movies with details - Low completeness
        let movies_without_details: Vec<MovieReference> =
            (0..10).map(|_| create_test_movie(false)).collect();

        let mut query_needs_metadata = MediaQuery::default();
        query_needs_metadata.sort.primary = SortBy::Rating;

        let completeness = DataCompletenessAnalyzer::analyze_movies(
            &movies_without_details,
            &query_needs_metadata,
        );
        assert_eq!(completeness, DataCompleteness::Low);

        // Mixed - Medium completeness
        let mut mixed_movies = vec![];
        for i in 0..10 {
            mixed_movies.push(create_test_movie(i < 5));
        }

        let completeness =
            DataCompletenessAnalyzer::analyze_movies(&mixed_movies, &query_needs_metadata);
        assert_eq!(completeness, DataCompleteness::Medium);
    }

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
        moderate_query.filters.library_ids = vec![Uuid::new_v4()];
        moderate_query.filters.media_type = Some(crate::query::types::MediaTypeFilter::Movie);

        assert_eq!(
            QueryComplexityAnalyzer::analyze(&moderate_query),
            super::super::QueryComplexity::Moderate
        );

        // Complex query
        let mut complex_query = moderate_query.clone();
        complex_query.filters.genres = vec!["Action".to_string()];
        complex_query.filters.year_range = Some(ScalarRange::new(2020, 2024));
        complex_query.filters.watch_status = Some(crate::WatchStatusFilter::InProgress);
        complex_query.search = Some(crate::query::types::SearchQuery {
            text: "test".to_string(),
            fields: vec![],
            fuzzy: false,
        });

        assert_eq!(
            QueryComplexityAnalyzer::analyze(&complex_query),
            super::super::QueryComplexity::Complex
        );
    }
}
