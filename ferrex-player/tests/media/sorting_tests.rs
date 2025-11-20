// Media Store Sorting Tests
//
// Requirements from Phase 3 (RUS-128):
// - Library-centric sorting API
// - Support for any SortStrategy from ferrex_core
// - Handle missing metadata gracefully with fallback
// - Performance < 100ms for 10,000 items
// - Maintain backward compatibility

use ferrex_player::domains::media::store::{MediaStore, MediaType};
use ferrex_core::{
    MediaFile, MovieReference, MovieTitle, MovieURL, MovieID,
    MediaDetailsOption, TmdbDetails, EnhancedMovieDetails,
    MediaReference,
    query::sorting::{SortStrategy, strategy::FieldSort, fields::*},
};
use std::path::PathBuf;
use uuid::Uuid;

// Helper function to create a test movie
fn create_test_movie(id: &str, title: &str, rating: Option<f32>, library_id: Uuid) -> MovieReference {
    let details = if let Some(rating_val) = rating {
        MediaDetailsOption::Details(TmdbDetails::Movie(EnhancedMovieDetails {
            id: 12345,
            title: title.to_string(),
            overview: Some("A test movie".to_string()),
            release_date: Some("2023-01-15".to_string()),
            runtime: Some(120),
            vote_average: Some(rating_val),
            vote_count: Some(100),
            popularity: Some(50.0),
            genres: vec!["Action".to_string()],
            production_companies: vec!["Test Studio".to_string()],
            
            // Media assets
            poster_path: None,
            backdrop_path: None,
            logo_path: None,
            images: Default::default(),
            
            // Credits
            cast: vec![],
            crew: vec![],
            
            // Additional
            videos: vec![],
            keywords: vec![],
            external_ids: Default::default(),
        }))
    } else {
        MediaDetailsOption::Endpoint(format!("/api/movies/{}", id))
    };

    MovieReference {
        id: MovieID::new(id.to_string()).unwrap(),
        tmdb_id: 12345,
        title: MovieTitle::new(title.to_string()).unwrap(),
        details,
        endpoint: MovieURL::from_string(format!("/api/stream/{}", id)),
        file: MediaFile {
            id: Uuid::new_v4(),
            path: PathBuf::from(format!("/movies/{}.mp4", title)),
            filename: format!("{}.mp4", title),
            size: 1000000,
            created_at: chrono::Utc::now(),
            media_file_metadata: None,
            library_id,
        },
        theme_color: None,
    }
}

#[test]
fn test_sort_library_by_title_ascending() {
    // RED: Write the test that should fail initially
    let mut store = MediaStore::new();
    let library_id = Uuid::new_v4();
    
    // Add movies in reverse alphabetical order
    let movie_c = create_test_movie("3", "Charlie", None, library_id);
    let movie_b = create_test_movie("2", "Bravo", None, library_id);
    let movie_a = create_test_movie("1", "Alpha", None, library_id);
    
    store.upsert(MediaReference::Movie(movie_c.clone()));
    store.upsert(MediaReference::Movie(movie_b.clone()));
    store.upsert(MediaReference::Movie(movie_a.clone()));
    
    // Sort by title ascending
    let strategy = FieldSort::new(TitleField, false);
    let sorted = store.get_sorted_library_movies(library_id, strategy);
    
    // Verify the movies are sorted alphabetically
    assert_eq!(sorted.len(), 3, "Should have 3 movies");
    assert_eq!(sorted[0].title.as_str(), "Alpha");
    assert_eq!(sorted[1].title.as_str(), "Bravo");
    assert_eq!(sorted[2].title.as_str(), "Charlie");
}

#[test]
fn test_sort_library_by_rating_with_fallback() {
    let mut store = MediaStore::new();
    let library_id = Uuid::new_v4();
    
    // Add movies with mixed metadata presence
    let movie_high = create_test_movie("1", "High Rated", Some(8.5), library_id);
    let movie_low = create_test_movie("2", "Low Rated", Some(6.5), library_id);
    let movie_none = create_test_movie("3", "No Rating", None, library_id); // No metadata
    
    store.upsert(MediaReference::Movie(movie_none.clone()));
    store.upsert(MediaReference::Movie(movie_high.clone()));
    store.upsert(MediaReference::Movie(movie_low.clone()));
    
    // Sort by rating descending (highest first) with metadata fallback
    use ferrex_core::query::sorting::fallback::MetadataFallbackSort;
    let strategy = MetadataFallbackSort::new(
        FieldSort::new(RatingField, true),
        FieldSort::new(TitleField, false),
        |movie: &MovieReference| {
            matches!(&movie.details, MediaDetailsOption::Details(_))
        }
    );
    
    let sorted = store.get_sorted_library_movies(library_id, strategy);
    
    // Verify order: High rated first, low rated second, no rating last
    assert_eq!(sorted.len(), 3);
    assert_eq!(sorted[0].title.as_str(), "High Rated");
    assert_eq!(sorted[1].title.as_str(), "Low Rated");
    assert_eq!(sorted[2].title.as_str(), "No Rating");
}

#[test]
fn test_performance_with_large_dataset() {
    use std::time::Instant;
    
    let mut store = MediaStore::new();
    let library_id = Uuid::new_v4();
    
    // Add 10,000 movies
    for i in 0..10_000 {
        let movie = create_test_movie(
            &format!("movie-{:04}", i),
            &format!("Movie {:04}", i),
            Some((i % 10) as f32 + 5.0), // Ratings from 5.0 to 14.0
            library_id
        );
        store.upsert(MediaReference::Movie(movie));
    }
    
    // Time the sorting operation
    let start = Instant::now();
    let strategy = FieldSort::new(RatingField, true);
    let sorted = store.get_sorted_library_movies(library_id, strategy);
    let elapsed = start.elapsed();
    
    // Verify results
    assert_eq!(sorted.len(), 10_000);
    
    // Performance requirement: < 100ms for 10,000 items
    assert!(
        elapsed.as_millis() < 100,
        "Sorting 10,000 items took {}ms, should be < 100ms",
        elapsed.as_millis()
    );
    
    // Verify sorting is correct (highest rated should be first)
    // Items with rating 14.0 should come before items with rating 5.0
    if let MediaDetailsOption::Details(TmdbDetails::Movie(details)) = &sorted[0].details {
        assert_eq!(details.vote_average, Some(14.0));
    }
    if let MediaDetailsOption::Details(TmdbDetails::Movie(details)) = &sorted[9999].details {
        assert_eq!(details.vote_average, Some(5.0));
    }
}

#[test]
fn test_multiple_libraries_independence() {
    // Verify that sorting respects library boundaries
    let mut store = MediaStore::new();
    let library_a = Uuid::new_v4();
    let library_b = Uuid::new_v4();
    
    // Add movies to Library A
    let movie_a1 = create_test_movie("a1", "Alpha Library A", Some(8.0), library_a);
    let movie_a2 = create_test_movie("a2", "Bravo Library A", Some(7.0), library_a);
    
    // Add movies to Library B  
    let movie_b1 = create_test_movie("b1", "Charlie Library B", Some(9.0), library_b);
    let movie_b2 = create_test_movie("b2", "Delta Library B", Some(6.0), library_b);
    
    store.upsert(MediaReference::Movie(movie_a1.clone()));
    store.upsert(MediaReference::Movie(movie_a2.clone()));
    store.upsert(MediaReference::Movie(movie_b1.clone()));
    store.upsert(MediaReference::Movie(movie_b2.clone()));
    
    // Sort Library A by title
    let strategy_a = FieldSort::new(TitleField, false);
    let sorted_a = store.get_sorted_library_movies(library_a, strategy_a);
    
    // Sort Library B by title
    let strategy_b = FieldSort::new(TitleField, false);
    let sorted_b = store.get_sorted_library_movies(library_b, strategy_b);
    
    // Verify Library A only contains its own movies
    assert_eq!(sorted_a.len(), 2, "Library A should have exactly 2 movies");
    assert_eq!(sorted_a[0].title.as_str(), "Alpha Library A");
    assert_eq!(sorted_a[1].title.as_str(), "Bravo Library A");
    
    // Verify Library B only contains its own movies
    assert_eq!(sorted_b.len(), 2, "Library B should have exactly 2 movies");
    assert_eq!(sorted_b[0].title.as_str(), "Charlie Library B");
    assert_eq!(sorted_b[1].title.as_str(), "Delta Library B");
    
    // Verify sorting by rating also respects library boundaries
    let rating_strategy_a = FieldSort::new(RatingField, true); // Descending
    let sorted_a_by_rating = store.get_sorted_library_movies(library_a, rating_strategy_a);
    let rating_strategy_b = FieldSort::new(RatingField, true); // Descending
    let sorted_b_by_rating = store.get_sorted_library_movies(library_b, rating_strategy_b);
    
    // Library A: 8.0 should come before 7.0
    assert_eq!(sorted_a_by_rating[0].title.as_str(), "Alpha Library A"); // 8.0
    assert_eq!(sorted_a_by_rating[1].title.as_str(), "Bravo Library A"); // 7.0
    
    // Library B: 9.0 should come before 6.0
    assert_eq!(sorted_b_by_rating[0].title.as_str(), "Charlie Library B"); // 9.0
    assert_eq!(sorted_b_by_rating[1].title.as_str(), "Delta Library B");   // 6.0
}

// ======== Phase 3: MediaStore Integration Tests ========
// These tests verify the integration between UI controls and MediaStore sorting

#[cfg(test)]
mod mediastore_integration_tests {
    use super::*;
    use ferrex_player::domains::media::MediaStoreSorting;
    use ferrex_player::domains::ui::SortBy;
    
    #[test]
    fn test_ui_sort_by_creates_valid_strategies() {
        // Test that UI SortBy values correctly map to sorting strategies
        let test_cases = vec![
            (SortBy::Title, "Title mapping"),
            (SortBy::DateAdded, "DateAdded mapping"),
            (SortBy::Year, "Year/ReleaseDate mapping"),
            (SortBy::Rating, "Rating mapping"),
        ];
        
        for (sort_by, description) in test_cases {
            // Create strategies for both ascending and descending
            let ascending_strategy = MediaStore::create_sort_strategy_for_movies(sort_by, true);
            let descending_strategy = MediaStore::create_sort_strategy_for_movies(sort_by, false);
            
            // Verify strategies can apply to a sample movie reference
            use ferrex_player::infrastructure::api_types::{MovieReference, MovieID, MediaDetailsOption};
            use ferrex_core::media::{MovieTitle, MovieURL};
            let sample = MovieReference {
                id: MovieID::new("sample".to_string()).unwrap(),
                tmdb_id: 1,
                title: MovieTitle::new("Sample".to_string()).unwrap(),
                details: MediaDetailsOption::Endpoint("/api/movie/sample".to_string()),
                endpoint: MovieURL::from_string("/api/movie/sample".to_string()),
                file: ferrex_player::infrastructure::api_types::MediaFile {
                    id: uuid::Uuid::new_v4(),
                    path: std::path::PathBuf::from("/tmp/sample.mkv"),
                    filename: "sample.mkv".to_string(),
                    size: 1,
                    created_at: chrono::Utc::now(),
                    media_file_metadata: None,
                    library_id: uuid::Uuid::new_v4(),
                },
                theme_color: None,
            };
            assert!(ascending_strategy.can_apply(&sample), "Failed for {}", description);
            assert!(descending_strategy.can_apply(&sample), "Failed for {}", description);
        }
    }
    
    #[test]
    fn test_sort_movies_mutates_store() {
        let mut store = MediaStore::new();
        let library_id = Uuid::new_v4();
        
        // Add movies in random order
        let movies = vec![
            create_test_movie("3", "Zebra", Some(7.5), library_id),
            create_test_movie("1", "Alpha", Some(8.0), library_id),
            create_test_movie("2", "Beta", Some(6.5), library_id),
        ];
        
        for movie in movies {
            store.upsert(MediaReference::Movie(movie));
        }
        
        // Sort movies in-place by title
        store.sort_movies(FieldSort::new(TitleField, false));
        
        // Get movies and verify they are sorted
        let sorted = store.get_movies(Some(library_id));
        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].title.as_str(), "Alpha");
        assert_eq!(sorted[1].title.as_str(), "Beta");
        assert_eq!(sorted[2].title.as_str(), "Zebra");
    }
    
    #[test]
    fn test_get_sorted_movies_without_mutation() {
        let store = MediaStore::new();
        let library_id = Uuid::new_v4();
        let mut store = store; // Make mutable for adding
        
        // Add movies
        let movies = vec![
            create_test_movie("3", "Zebra", Some(7.5), library_id),
            create_test_movie("1", "Alpha", Some(8.0), library_id),
            create_test_movie("2", "Beta", Some(6.5), library_id),
        ];
        
        for movie in movies {
            store.upsert(MediaReference::Movie(movie));
        }
        
        // Make immutable for testing
        let store = store;
        
        // Get sorted movies without mutating
        let sorted = store.get_sorted_movies(Some(library_id), FieldSort::new(TitleField, false));
        
        // Verify sorted order
        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].title.as_str(), "Alpha");
        assert_eq!(sorted[1].title.as_str(), "Beta");
        assert_eq!(sorted[2].title.as_str(), "Zebra");
    }
    
    #[test]
    fn test_sorting_with_mixed_sort_orders() {
        let mut store = MediaStore::new();
        let library_id = Uuid::new_v4();
        
        // Add movies with ratings
        let movies = vec![
            create_test_movie("1", "High Rated", Some(9.0), library_id),
            create_test_movie("2", "Low Rated", Some(5.5), library_id),
            create_test_movie("3", "Mid Rated", Some(7.0), library_id),
        ];
        
        for movie in movies {
            store.upsert(MediaReference::Movie(movie));
        }
        
        // Test ascending rating (lowest first)
        let ascending = store.get_sorted_movies(Some(library_id), FieldSort::new(RatingField, false));
        assert_eq!(ascending[0].title.as_str(), "Low Rated");
        assert_eq!(ascending[1].title.as_str(), "Mid Rated");
        assert_eq!(ascending[2].title.as_str(), "High Rated");
        
        // Test descending rating (highest first)
        let descending = store.get_sorted_movies(Some(library_id), FieldSort::new(RatingField, true));
        assert_eq!(descending[0].title.as_str(), "High Rated");
        assert_eq!(descending[1].title.as_str(), "Mid Rated");
        assert_eq!(descending[2].title.as_str(), "Low Rated");
    }
}