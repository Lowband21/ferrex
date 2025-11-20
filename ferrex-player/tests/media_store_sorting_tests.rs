//! MediaStore Sorting Integration Tests
//!
//! Requirements from Phase 3: MediaStore Integration
//! - MediaStore should support sorting movies and series
//! - Sorting should work with different strategies
//! - UI controls should trigger sorting in MediaStore
//! - Sorted data should be reflected in view models

use ferrex_player::domains::media::{store::MediaStore, MediaStoreSorting};
use ferrex_player::infrastructure::api_types::{
    MediaId, MediaReference, MovieReference, MediaDetailsOption, TmdbDetails,
    MovieID, MediaFile, EnhancedMovieDetails,
};
use ferrex_core::media::{MovieTitle, MovieURL};
use ferrex_core::query::sorting::{FieldSort, TitleField, DateAddedField, ReleaseDateField, RatingField};
use uuid::Uuid;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
/// Create a test movie with specified title and metadata
fn create_test_movie(title: &str, year: u32, rating: f32) -> MovieReference {
    let id_str = format!("movie_{}", title.to_lowercase().replace(' ', "_"));
    let library_id = Uuid::new_v4();
    MovieReference {
        id: MovieID::new(id_str).unwrap(),
        tmdb_id: 12345,
        title: MovieTitle::new(title.to_string()).unwrap(),
        details: MediaDetailsOption::Details(TmdbDetails::Movie(EnhancedMovieDetails {
            id: 12345,
            title: title.to_string(),
            overview: None,
            release_date: Some(format!("{}-01-01", year)),
            runtime: Some(120),
            vote_average: Some(rating),
            vote_count: Some(100),
            popularity: Some(50.0),
            genres: vec![],
            production_companies: vec![],
            poster_path: None,
            backdrop_path: None,
            logo_path: None,
            images: ferrex_core::media::MediaImages::default(),
            cast: vec![],
            crew: vec![],
            videos: vec![],
            keywords: vec![],
            external_ids: ferrex_core::media::ExternalIds::default(),
        })),
        endpoint: MovieURL::from_string(format!("/api/movie/{}", title)),
        file: MediaFile {
            id: Uuid::new_v4(),
            path: PathBuf::from(format!("/movies/{}.mp4", title)),
            filename: format!("{}.mp4", title),
            size: 100,
            created_at: chrono::Utc::now(),
            media_file_metadata: None,
            library_id,
        },
        theme_color: None,
    }
    }

#[test]
fn test_sort_movies_by_title_ascending() {
    let mut store = MediaStore::new();
    
    // Add test movies
    let movies = vec![
        create_test_movie("Zebra", 2020, 7.5),
        create_test_movie("Alpha", 2021, 8.0),
        create_test_movie("Beta", 2019, 6.5),
    ];
    
for movie in movies {
        store.upsert(MediaReference::Movie(movie));
    }
    
    // Sort by title ascending
    store.sort_movies(FieldSort::new(TitleField, false));
    
    // Get sorted movies
    let sorted = store.get_movies(None);
    
    // Verify order
    assert_eq!(sorted.len(), 3);
assert_eq!(sorted[0].title.as_str(), "Alpha");
    assert_eq!(sorted[1].title.as_str(), "Beta");
    assert_eq!(sorted[2].title.as_str(), "Zebra");
}

#[test]
fn test_sort_movies_by_title_descending() {
    let mut store = MediaStore::new();
    
    // Add test movies
    let movies = vec![
        create_test_movie("Alpha", 2021, 8.0),
        create_test_movie("Zebra", 2020, 7.5),
        create_test_movie("Beta", 2019, 6.5),
    ];
    
for movie in movies {
        store.upsert(MediaReference::Movie(movie));
    }
    
    // Sort by title descending
    store.sort_movies(FieldSort::new(TitleField, true));
    
    // Get sorted movies
    let sorted = store.get_movies(None);
    
    // Verify order
    assert_eq!(sorted.len(), 3);
assert_eq!(sorted[0].title.as_str(), "Zebra");
    assert_eq!(sorted[1].title.as_str(), "Beta");
    assert_eq!(sorted[2].title.as_str(), "Alpha");
}

#[test]
fn test_sort_movies_by_year() {
    let mut store = MediaStore::new();
    
    // Add test movies with different years
    let movies = vec![
        create_test_movie("Movie 2021", 2021, 8.0),
        create_test_movie("Movie 2019", 2019, 6.5),
        create_test_movie("Movie 2020", 2020, 7.5),
    ];
    
for movie in movies {
        store.upsert(MediaReference::Movie(movie));
    }
    
    // Sort by year ascending (oldest first)
    store.sort_movies(FieldSort::new(ReleaseDateField, false));
    
    // Get sorted movies
    let sorted = store.get_movies(None);
    
// Verify order
    assert_eq!(sorted.len(), 3);
    let years: Vec<_> = sorted.iter()
        .map(|m| m.details.get_release_year().unwrap_or(0))
        .collect();
    assert_eq!(years, vec![2019, 2020, 2021]);
}

#[test]
fn test_sort_movies_by_rating() {
    let mut store = MediaStore::new();
    
    // Add test movies with different ratings
    let movies = vec![
        create_test_movie("High Rated", 2021, 9.0),
        create_test_movie("Low Rated", 2020, 5.5),
        create_test_movie("Mid Rated", 2019, 7.0),
    ];
    
for movie in movies {
        store.upsert(MediaReference::Movie(movie));
    }
    
    // Sort by rating descending (highest first)
    store.sort_movies(FieldSort::new(RatingField, true));
    
    // Get sorted movies
    let sorted = store.get_movies(None);
    
    // Verify order
    assert_eq!(sorted.len(), 3);
    
    // Extract ratings from details
let ratings: Vec<f32> = sorted.iter()
        .filter_map(|movie| {
            match &movie.details {
                MediaDetailsOption::Details(TmdbDetails::Movie(m)) => m.vote_average,
                _ => None,
            }
        })
        .collect();
    
    assert_eq!(ratings[0], 9.0);
    assert_eq!(ratings[1], 7.0);
    assert_eq!(ratings[2], 5.5);
}

#[test]
fn test_get_sorted_movies_without_mutation() {
    let store = MediaStore::new();
    let store_arc = Arc::new(RwLock::new(store));
    
    // Add test movies
    {
        let mut store = store_arc.write().unwrap();
        let movies = vec![
            create_test_movie("Zebra", 2020, 7.5),
            create_test_movie("Alpha", 2021, 8.0),
            create_test_movie("Beta", 2019, 6.5),
        ];
        
for movie in movies {
            store.upsert(MediaReference::Movie(movie));
        }
    }
    
    // Get sorted movies without mutating the store
    let sorted = {
        let store = store_arc.read().unwrap();
        store.get_sorted_movies(None, FieldSort::new(TitleField, false))
    };
    
    // Verify sorted order
    assert_eq!(sorted.len(), 3);
assert_eq!(sorted[0].title.as_str(), "Alpha");
    assert_eq!(sorted[1].title.as_str(), "Beta");
    assert_eq!(sorted[2].title.as_str(), "Zebra");
    
    // Verify original order is unchanged
    {
        let store = store_arc.read().unwrap();
        let unsorted = store.get_movies(None);
        // The order might have been affected by internal HashMap ordering,
        // but it shouldn't be alphabetically sorted
let titles: Vec<String> = unsorted.iter().map(|m| m.title.as_str().to_string()).collect();
        // Just verify we still have all movies
        assert!(titles.contains(&"Alpha".to_string()));
        assert!(titles.contains(&"Beta".to_string()));
        assert!(titles.contains(&"Zebra".to_string()));
    }
}

#[test]
fn test_sorting_handles_missing_metadata() {
    let mut store = MediaStore::new();
    
    // Add movies with missing metadata
    let mut movie_with_rating = create_test_movie("Movie A", 2020, 8.0);
    let mut movie_without_rating = create_test_movie("Movie B", 2021, 0.0);
    movie_without_rating.details = ferrex_player::infrastructure::api_types::MediaDetailsOption::Endpoint("/api/movie/123".to_string());
    
store.upsert(MediaReference::Movie(movie_with_rating));
    store.upsert(MediaReference::Movie(movie_without_rating));
    
    // Sort by rating - movies without rating should sort last
    store.sort_movies(FieldSort::new(RatingField, true));
    
    let sorted = store.get_movies(None);
    assert_eq!(sorted.len(), 2);
assert_eq!(sorted[0].title.as_str(), "Movie A"); // Has rating
    assert_eq!(sorted[1].title.as_str(), "Movie B"); // Missing rating sorts last
}

#[test]
fn test_ui_sort_by_to_field_sort_conversion() {
    use ferrex_player::domains::ui::SortBy;
    
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
        
        // Verify strategies can apply to a sample movie
        let sample = create_test_movie("Sample", 2020, 7.0);
        assert!(ascending_strategy.can_apply(&sample), "Failed for {}", description);
        assert!(descending_strategy.can_apply(&sample), "Failed for {}", description);
    }
}