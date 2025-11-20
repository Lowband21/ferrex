//! Tests for sorting strategy implementations

#[cfg(test)]
mod tests {
    use crate::query::sorting::{
        fallback::{FallbackSortBuilder, MetadataFallbackSort, SmartFallbackSort},
        fields::{TitleField, RatingField},
        performance::{LazySort, ParallelSort},
        strategy::{AdaptiveSort, ChainedSort, ConstFieldSort, FieldSort, SortCost},
        SortStrategy,
    };
    use crate::{
        EnhancedMovieDetails, MediaDetailsOption, MediaFile, MovieID, MovieReference, MovieTitle,
        MovieURL, TmdbDetails,
    };
    use std::path::PathBuf;
    use uuid::Uuid;

    fn create_test_movie(id: &str, title: &str, rating: Option<f32>) -> MovieReference {
        let details = if let Some(rating_val) = rating {
            MediaDetailsOption::Details(TmdbDetails::Movie(EnhancedMovieDetails {
                id: 12345,
                title: title.to_string(),
                overview: Some("A test movie".to_string()),
                release_date: Some("2023-01-15".to_string()),
                runtime: Some(120),
                vote_average: Some(rating_val),
                vote_count: Some(1000),
                popularity: Some(85.3),
                genres: vec![],
                production_companies: vec![],
                poster_path: None,
                backdrop_path: None,
                logo_path: None,
                images: crate::MediaImages::default(),
                cast: vec![],
                crew: vec![],
                videos: vec![],
                keywords: vec![],
                external_ids: crate::ExternalIds::default(),
            }))
        } else {
            MediaDetailsOption::Endpoint(format!("/api/movies/{}", id))
        };

        MovieReference {
            id: MovieID::new(id.to_string()).unwrap(),
            tmdb_id: 12345,
            title: MovieTitle::new(title.to_string()).unwrap(),
            details,
            endpoint: MovieURL::from_string(format!("/api/movies/{}", id)),
            file: MediaFile {
                id: Uuid::new_v4(),
                path: PathBuf::from(format!("/movies/{}.mp4", id)),
                filename: format!("{}.mp4", id),
                size: 1000000,
                created_at: chrono::Utc::now(),
                media_file_metadata: None,
                library_id: Uuid::new_v4(),
            },
            theme_color: None,
        }
    }

    #[test]
    fn test_field_sort_by_title() {
        let mut movies = vec![
            create_test_movie("3", "Charlie", None),
            create_test_movie("1", "Alice", None),
            create_test_movie("2", "Bob", None),
        ];

        let sort = FieldSort::<MovieReference, _>::new(TitleField, false);
        sort.sort(&mut movies);

        assert_eq!(movies[0].title.as_str(), "Alice");
        assert_eq!(movies[1].title.as_str(), "Bob");
        assert_eq!(movies[2].title.as_str(), "Charlie");
    }

    #[test]
    fn test_field_sort_reverse() {
        let mut movies = vec![
            create_test_movie("3", "Charlie", None),
            create_test_movie("1", "Alice", None),
            create_test_movie("2", "Bob", None),
        ];

        let sort = FieldSort::<MovieReference, _>::new(TitleField, true);
        sort.sort(&mut movies);

        assert_eq!(movies[0].title.as_str(), "Charlie");
        assert_eq!(movies[1].title.as_str(), "Bob");
        assert_eq!(movies[2].title.as_str(), "Alice");
    }

    #[test]
    fn test_field_sort_by_rating() {
        let mut movies = vec![
            create_test_movie("1", "Movie 1", Some(7.5)),
            create_test_movie("2", "Movie 2", Some(8.5)),
            create_test_movie("3", "Movie 3", Some(6.5)),
        ];

        let sort = FieldSort::<MovieReference, _>::new(RatingField, true); // Descending
        sort.sort(&mut movies);

        // Check ratings are in descending order
        if let MediaDetailsOption::Details(TmdbDetails::Movie(details)) = &movies[0].details {
            assert_eq!(details.vote_average, Some(8.5));
        }
        if let MediaDetailsOption::Details(TmdbDetails::Movie(details)) = &movies[1].details {
            assert_eq!(details.vote_average, Some(7.5));
        }
        if let MediaDetailsOption::Details(TmdbDetails::Movie(details)) = &movies[2].details {
            assert_eq!(details.vote_average, Some(6.5));
        }
    }

    #[test]
    fn test_chained_sort() {
        let mut movies = vec![
            create_test_movie("1", "Alice", Some(7.5)),
            create_test_movie("2", "Bob", Some(8.5)),
            create_test_movie("3", "Alice", Some(8.5)),
            create_test_movie("4", "Charlie", Some(7.5)),
        ];

        // Primary sort by rating (descending), secondary by title (ascending)
        let sort = ChainedSort::new()
            .then_by(FieldSort::<MovieReference, _>::new(RatingField, true))
            .then_by(FieldSort::<MovieReference, _>::new(TitleField, false));

        sort.sort(&mut movies);

        // Movies should be sorted by rating first, then by title for same ratings
        assert_eq!(movies[0].title.as_str(), "Alice"); // 8.5 rating
        assert_eq!(movies[1].title.as_str(), "Bob"); // 8.5 rating
        assert_eq!(movies[2].title.as_str(), "Alice"); // 7.5 rating
        assert_eq!(movies[3].title.as_str(), "Charlie"); // 7.5 rating
    }

    #[test]
    fn test_adaptive_sort() {
        let mut movies = vec![
            create_test_movie("1", "Alice", Some(8.5)),
            create_test_movie("2", "Bob", None), // No rating
            create_test_movie("3", "Charlie", Some(7.5)),
        ];

        // Use rating when available, fall back to title
        let sort = AdaptiveSort::new(
            FieldSort::<MovieReference, _>::new(RatingField, true),  // Prefer rating
            FieldSort::<MovieReference, _>::new(TitleField, false),  // Fallback to title
        );

        // Check if it can apply
        assert!(sort.can_apply(&movies[0])); // Has rating or title
        assert!(sort.can_apply(&movies[1])); // Has title at least

        // For movies without ratings, it should use title sort
        sort.sort(&mut movies);
    }

    #[test]
    fn test_const_field_sort() {
        let mut movies = vec![
            create_test_movie("3", "Charlie", None),
            create_test_movie("1", "Alice", None),
            create_test_movie("2", "Bob", None),
        ];

        // Compile-time optimized sort
        let sort = ConstFieldSort::<_, _, false>::new(TitleField);
        sort.sort(&mut movies);

        assert_eq!(movies[0].title.as_str(), "Alice");
        assert_eq!(movies[1].title.as_str(), "Bob");
        assert_eq!(movies[2].title.as_str(), "Charlie");

        // Test reverse with const generic
        let mut movies2 = movies.clone();
        let sort_reverse = ConstFieldSort::<_, _, true>::new(TitleField);
        sort_reverse.sort(&mut movies2);

        assert_eq!(movies2[0].title.as_str(), "Charlie");
        assert_eq!(movies2[1].title.as_str(), "Bob");
        assert_eq!(movies2[2].title.as_str(), "Alice");
    }

    #[test]
    fn test_sort_cost_estimates() {
        let sort_title = FieldSort::<MovieReference, _>::new(TitleField, false);
        assert_eq!(sort_title.cost_estimate(), SortCost::Moderate);

        let sort_rating = FieldSort::<MovieReference, _>::new(RatingField, false);
        assert_eq!(sort_rating.cost_estimate(), SortCost::Expensive); // Requires fetch

        let chained = ChainedSort::new()
            .then_by(FieldSort::<MovieReference, _>::new(RatingField, true))
            .then_by(FieldSort::<MovieReference, _>::new(TitleField, false));
        assert_eq!(chained.cost_estimate(), SortCost::Expensive); // Max of all strategies
    }

    #[test]
    fn test_can_apply_logic() {
        let movie_with_details = create_test_movie("1", "Test", Some(8.0));
        let movie_without_details = create_test_movie("2", "Test", None);

        let sort_rating = FieldSort::<MovieReference, _>::new(RatingField, false);
        let sort_title = FieldSort::<MovieReference, _>::new(TitleField, false);

        // Rating requires fetch, should return false for movie without details
        assert!(sort_rating.can_apply(&movie_with_details));
        assert!(!sort_rating.can_apply(&movie_without_details));

        // Title is always available
        assert!(sort_title.can_apply(&movie_with_details));
        assert!(sort_title.can_apply(&movie_without_details));
    }

    #[test]
    fn test_parallel_sort_threshold() {
        let mut movies = vec![
            create_test_movie("3", "Charlie", None),
            create_test_movie("1", "Alice", None),
            create_test_movie("2", "Bob", None),
        ];

        // Create parallel sort with low threshold for testing
        let sort = ParallelSort::with_threshold(
            FieldSort::<MovieReference, _>::new(TitleField, false),
            2, // Low threshold for testing
        );

        sort.sort(&mut movies);

        assert_eq!(movies[0].title.as_str(), "Alice");
        assert_eq!(movies[1].title.as_str(), "Bob");
        assert_eq!(movies[2].title.as_str(), "Charlie");
    }

    #[test]
    fn test_lazy_sort() {
        let mut movies = vec![
            create_test_movie("3", "Charlie", None),
            create_test_movie("1", "Alice", None),
            create_test_movie("2", "Bob", None),
        ];

        let sort = LazySort::new(FieldSort::<MovieReference, _>::new(TitleField, false));
        sort.sort(&mut movies);

        assert_eq!(movies[0].title.as_str(), "Alice");
        assert_eq!(movies[1].title.as_str(), "Bob");
        assert_eq!(movies[2].title.as_str(), "Charlie");
    }

    #[test]
    fn test_smart_fallback_sort() {
        let mut movies = vec![
            create_test_movie("1", "Alice", Some(8.5)),
            create_test_movie("2", "Bob", None),
            create_test_movie("3", "Charlie", Some(7.5)),
        ];

        let sort = SmartFallbackSort::new()
            .with_rule(
                |movie: &MovieReference| {
                    matches!(&movie.details, MediaDetailsOption::Details(_))
                },
                FieldSort::<MovieReference, _>::new(RatingField, true),
                100,
            )
            .with_default(FieldSort::<MovieReference, _>::new(TitleField, false));

        sort.sort(&mut movies);
    }

    #[test]
    fn test_fallback_sort_builder() {
        let mut movies = vec![
            create_test_movie("1", "Alice", Some(8.5)),
            create_test_movie("2", "Bob", None),
            create_test_movie("3", "Charlie", Some(7.5)),
        ];

        let sort = FallbackSortBuilder::<MovieReference>::new()
            .when_available::<MovieReference>(
                |movie: &MovieReference| {
                    matches!(&movie.details, MediaDetailsOption::Details(_))
                },
                FieldSort::<MovieReference, _>::new(RatingField, true),
            )
            .otherwise(FieldSort::<MovieReference, _>::new(TitleField, false));

        sort.sort(&mut movies);
    }

    #[test]
    fn test_metadata_fallback_sort() {
        let mut movies = vec![
            create_test_movie("1", "Alice", Some(8.5)),
            create_test_movie("2", "Bob", None),
            create_test_movie("3", "Charlie", Some(7.5)),
        ];

        let sort = MetadataFallbackSort::new(
            FieldSort::<MovieReference, _>::new(RatingField, true),
            FieldSort::<MovieReference, _>::new(TitleField, false),
            |movie: &MovieReference| {
                matches!(&movie.details, MediaDetailsOption::Details(_))
            },
        );

        sort.sort(&mut movies);
    }


}