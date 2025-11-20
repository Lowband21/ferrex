// MediaStore Tests
// 
// Requirements:
// - MediaStore must maintain media references with proper lifecycle
// - Must not downgrade Details to Endpoints
// - Must notify subscribers of changes
// - Must support batch operations for performance

use ferrex_player::domains::media::store::{MediaStore, MediaChangeEvent, ChangeType};
use ferrex_player::infrastructure::api_types::{
    MediaReference, MediaId, MediaDetailsOption, TmdbDetails,
    MovieReference, SeriesReference, SeasonReference, EpisodeReference,
    MovieID, SeriesID, SeasonID, EpisodeID, MediaFile,
    EnhancedMovieDetails, EnhancedSeriesDetails,
};
use ferrex_core::media::{MovieTitle, MovieURL, SeriesTitle, SeriesURL};
use std::sync::{Arc, RwLock};
use std::path::PathBuf;
use uuid::Uuid;

/// Helper to create a test movie with endpoint details
fn create_test_movie(id: &str, title: &str, library_id: Uuid) -> MovieReference {
    MovieReference {
        id: MovieID::new(id.to_string()).unwrap(),
        tmdb_id: 12345,
        title: MovieTitle::new(title.to_string()).unwrap(),
        details: MediaDetailsOption::Endpoint(format!("/api/movie/{}", id)),
        endpoint: MovieURL::from_string(format!("/api/movie/{}", id)),
        file: MediaFile {
            id: Uuid::new_v4(),
            path: PathBuf::from(format!("/test/movies/{}.mkv", id)),
            filename: format!("{}.mkv", title),
            size: 1024 * 1024 * 100, // 100MB
            created_at: chrono::Utc::now(),
            media_file_metadata: None,
            library_id,
        },
        theme_color: None,
    }
}

/// Helper to create a test movie with full details
fn create_test_movie_with_details(id: &str, title: &str, library_id: Uuid) -> MovieReference {
    let mut movie = create_test_movie(id, title, library_id);
    movie.details = MediaDetailsOption::Details(TmdbDetails::Movie(EnhancedMovieDetails {
        id: 12345,
        title: title.to_string(),
        overview: Some("Test movie overview".to_string()),
        release_date: Some("2024-01-01".to_string()),
        runtime: Some(120),
        vote_average: Some(7.5),
        vote_count: Some(1000),
        popularity: Some(100.0),
        genres: vec!["Action".to_string(), "Adventure".to_string()],
        production_companies: vec!["Test Studios".to_string()],
        poster_path: Some("/poster.jpg".to_string()),
        backdrop_path: Some("/backdrop.jpg".to_string()),
        logo_path: None,
        images: ferrex_core::media::MediaImages::default(),
        cast: vec![],
        crew: vec![],
        videos: vec![],
        keywords: vec!["test".to_string()],
        external_ids: ferrex_core::media::ExternalIds::default(),
    }));
    movie
}

/// Helper to create a test series
fn create_test_series(id: &str, title: &str, library_id: Uuid) -> SeriesReference {
    SeriesReference {
        id: SeriesID::new(id.to_string()).unwrap(),
        library_id,
        tmdb_id: 54321,
        title: SeriesTitle::new(title.to_string()).unwrap(),
        details: MediaDetailsOption::Endpoint(format!("/api/series/{}", id)),
        endpoint: SeriesURL::from_string(format!("/api/series/{}", id)),
        created_at: chrono::Utc::now(),
        theme_color: None,
    }
}

#[test]
fn media_store_starts_empty() {
    let store = MediaStore::new();
    assert!(store.is_empty(), "New MediaStore should be empty");
    assert_eq!(store.len(), 0, "New MediaStore should have length 0");
}

#[test]
fn can_insert_and_retrieve_movie() {
    let mut store = MediaStore::new();
    let library_id = Uuid::new_v4();
    let movie = create_test_movie("123", "Test Movie", library_id);
    
    // Insert movie
    let inserted_id = store.upsert(MediaReference::Movie(movie.clone()));
    assert!(matches!(inserted_id, MediaId::Movie(_)), "Requirement: MediaStore must accept new media references");
    
    // Retrieve movie
    let media_id = MediaId::Movie(MovieID::new("123".to_string()).unwrap());
    let retrieved = store.get(&media_id);
    assert!(retrieved.is_some(), "Requirement: MediaStore must retrieve stored media");
    
    match retrieved.unwrap() {
        MediaReference::Movie(m) => {
            assert_eq!(m.id, movie.id);
            assert_eq!(m.title, movie.title);
        }
        _ => panic!("MediaStore returned wrong media type"),
    }
}

#[test]
fn updates_existing_media_reference() {
    let mut store = MediaStore::new();
    let library_id = Uuid::new_v4();
    let movie1 = create_test_movie("123", "Original Title", library_id);
    let mut movie2 = movie1.clone();
    movie2.title = MovieTitle::new("Updated Title".to_string()).unwrap();
    
    // Insert first version
    store.upsert(MediaReference::Movie(movie1));
    
    // Update with second version
    let updated_id = store.upsert(MediaReference::Movie(movie2.clone()));
    assert!(matches!(updated_id, MediaId::Movie(_)), "Requirement: MediaStore must update existing references");
    
    // Verify update
    let media_id = MediaId::Movie(MovieID::new("123".to_string()).unwrap());
    match store.get(&media_id) {
        Some(MediaReference::Movie(m)) => {
            assert_eq!(m.title.as_str(), "Updated Title", 
                "MediaStore must persist updates to existing media");
        }
        _ => panic!("Movie not found after update"),
    }
}

#[test]
fn upgrades_endpoint_to_details() {
    let mut store = MediaStore::new();
    let library_id = Uuid::new_v4();
    
    // Start with endpoint
    let movie_endpoint = create_test_movie("123", "Test Movie", library_id);
    store.upsert(MediaReference::Movie(movie_endpoint.clone()));
    
    // Update with full details
    let movie_details = create_test_movie_with_details("123", "Test Movie", library_id);
    let updated_id = store.upsert(MediaReference::Movie(movie_details.clone()));
    assert!(matches!(updated_id, MediaId::Movie(_)), "Requirement: MediaStore must upgrade Endpoint to Details");
    
    // Verify details are stored
    let media_id = MediaId::Movie(MovieID::new("123".to_string()).unwrap());
    match store.get(&media_id) {
        Some(MediaReference::Movie(m)) => {
            match &m.details {
                MediaDetailsOption::Details(TmdbDetails::Movie(movie_details)) => {
                    assert_eq!(movie_details.overview, Some("Test movie overview".to_string()));
                    assert_eq!(movie_details.vote_average, Some(7.5));
                }
                MediaDetailsOption::Details(_) => {
                    panic!("Expected Movie details but got different type");
                }
                MediaDetailsOption::Endpoint(_) => {
                    panic!("MediaStore failed to upgrade to Details");
                }
            }
        }
        _ => panic!("Movie not found after details upgrade"),
    }
}

#[test]
fn prevents_details_downgrade_to_endpoint() {
    let mut store = MediaStore::new();
    let library_id = Uuid::new_v4();
    
    // Start with full details
    let movie_details = create_test_movie_with_details("123", "Test Movie", library_id);
    store.upsert(MediaReference::Movie(movie_details.clone()));
    
    // Try to downgrade to endpoint
    let movie_endpoint = create_test_movie("123", "Test Movie", library_id);
    let _id = store.upsert(MediaReference::Movie(movie_endpoint));
    // The downgrade protection happens silently - verify by checking the stored value
    
    // Verify details are preserved
    let media_id = MediaId::Movie(MovieID::new("123".to_string()).unwrap());
    match store.get(&media_id) {
        Some(MediaReference::Movie(m)) => {
            assert!(matches!(m.details, MediaDetailsOption::Details(_)),
                "MediaStore must preserve Details when downgrade is attempted");
        }
        _ => panic!("Movie not found"),
    }
}

#[test]
fn batch_operations_work() {
    let mut store = MediaStore::new();
    let library_id = Uuid::new_v4();
    
    // Begin batch
    store.begin_batch();
    
    // Insert multiple items in batch
    for i in 0..5 {
        let movie = create_test_movie(&format!("{}", i), &format!("Movie {}", i), library_id);
        store.upsert(MediaReference::Movie(movie));
    }
    
    // End batch
    store.end_batch();
    
    // All items should be stored
    assert_eq!(store.len(), 5, "All batched items should be stored");
}

#[test]
fn filters_media_by_library() {
    let mut store = MediaStore::new();
    let library1 = Uuid::new_v4();
    let library2 = Uuid::new_v4();
    
    // Add movies to different libraries
    store.upsert(MediaReference::Movie(create_test_movie("1", "Movie 1", library1)));
    store.upsert(MediaReference::Movie(create_test_movie("2", "Movie 2", library1)));
    store.upsert(MediaReference::Movie(create_test_movie("3", "Movie 3", library2)));
    
    // Get movies from library1
    let lib1_movies = store.get_movies(Some(library1));
    assert_eq!(lib1_movies.len(), 2, "Should return only library1 movies");
    assert!(lib1_movies.iter().all(|m| m.file.library_id == library1));
    
    // Get movies from library2
    let lib2_movies = store.get_movies(Some(library2));
    assert_eq!(lib2_movies.len(), 1, "Should return only library2 movies");
    assert!(lib2_movies.iter().all(|m| m.file.library_id == library2));
    
    // Get all movies
    let all_movies = store.get_movies(None);
    assert_eq!(all_movies.len(), 3, "Should return all movies when no filter");
}

#[test]
fn clears_library_media() {
    let mut store = MediaStore::new();
    let library1 = Uuid::new_v4();
    let library2 = Uuid::new_v4();
    
    // Add items to both libraries
    store.upsert(MediaReference::Movie(create_test_movie("1", "Movie 1", library1)));
    store.upsert(MediaReference::Movie(create_test_movie("2", "Movie 2", library2)));
    store.upsert(MediaReference::Series(create_test_series("3", "Series 1", library1)));
    
    assert_eq!(store.len(), 3);
    
    // Clear library1
    store.clear_library(library1);
    
    // Only library2 items should remain
    assert_eq!(store.len(), 1, "Requirement: clear_library must remove all media from specified library");
    let remaining = store.get_movies(None);
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].file.library_id, library2);
}

#[test]
fn removes_media_reference() {
    let mut store = MediaStore::new();
    let library_id = Uuid::new_v4();
    let movie = create_test_movie("123", "Test Movie", library_id);
    
    // Add and verify
    store.upsert(MediaReference::Movie(movie.clone()));
    assert_eq!(store.len(), 1);
    
    // Remove
    let media_id = MediaId::Movie(MovieID::new("123".to_string()).unwrap());
    let removed = store.remove(&media_id);
    assert!(removed.is_some(), "Should return Some when item is removed");
    
    // Verify removal
    assert_eq!(store.len(), 0);
    assert!(store.get(&media_id).is_none(), "Removed media should not be retrievable");
    
    // Try to remove again
    let removed_again = store.remove(&media_id);
    assert!(removed_again.is_none(), "Should return None when item doesn't exist");
}

#[test]
fn tracks_media_changes() {
    let mut store = MediaStore::new();
    let library_id = Uuid::new_v4();
    
    // Insert a movie
    let movie = create_test_movie("123", "Test Movie", library_id);
    let id1 = store.upsert(MediaReference::Movie(movie.clone()));
    assert_eq!(store.len(), 1);
    
    // Update the movie
    let mut updated_movie = movie.clone();
    updated_movie.title = MovieTitle::new("Updated Title".to_string()).unwrap();
    let id2 = store.upsert(MediaReference::Movie(updated_movie));
    assert_eq!(id1, id2, "Same ID for updates");
    assert_eq!(store.len(), 1, "Count unchanged after update");
    
    // Remove the movie
    let media_id = MediaId::Movie(MovieID::new("123".to_string()).unwrap());
    let removed = store.remove(&media_id);
    assert!(removed.is_some());
    assert_eq!(store.len(), 0, "Count decreases after removal");
}

#[test]
fn stores_all_movies() {
    let mut store = MediaStore::new();
    let library_id = Uuid::new_v4();
    
    // Add movies
    store.upsert(MediaReference::Movie(create_test_movie("1", "Zebra", library_id)));
    store.upsert(MediaReference::Movie(create_test_movie("2", "Apple", library_id)));
    store.upsert(MediaReference::Movie(create_test_movie("3", "Mango", library_id)));
    
    // Get all movies
    let movies = store.get_all_movies();
    assert_eq!(movies.len(), 3);
    
    // Movies should all be present (order not guaranteed)
    let titles: Vec<&str> = movies.iter().map(|m| m.title.as_str()).collect();
    assert!(titles.contains(&"Apple"));
    assert!(titles.contains(&"Mango"));
    assert!(titles.contains(&"Zebra"));
}

#[test]
fn finds_media_by_file_id() {
    let mut store = MediaStore::new();
    let library_id = Uuid::new_v4();
    let file_uuid = Uuid::new_v4();
    
    // Add movie with specific file UUID
    let mut movie = create_test_movie("123", "Test Movie", library_id);
    movie.file.id = file_uuid;
    store.upsert(MediaReference::Movie(movie.clone()));
    
    // Find by file UUID
    let found = store.find_by_file_id(&file_uuid.to_string());
    assert!(!found.is_empty(), "Requirement: MediaStore must support file_id lookups");
    assert_eq!(found.len(), 1, "Should find exactly one movie");
    
    // Verify the found media ID
    match &found[0] {
        MediaId::Movie(movie_id) => {
            assert_eq!(movie_id.as_str(), "123");
        }
        _ => panic!("Wrong media type returned"),
    }
    
    // Try non-existent file_id
    let not_found = store.find_by_file_id(&Uuid::new_v4().to_string());
    assert!(not_found.is_empty());
}

#[test]
fn manages_series_with_seasons() {
    let mut store = MediaStore::new();
    let library_id = Uuid::new_v4();
    
    // Add series
    let series = create_test_series("s1", "Test Series", library_id);
    store.upsert(MediaReference::Series(series));
    
    // Add seasons
    for i in 1..=3 {
        let season = SeasonReference {
            id: SeasonID::new(format!("season_{}", i)).unwrap(),
            season_number: ferrex_core::media::SeasonNumber::new(i as u8),
            series_id: SeriesID::new("s1".to_string()).unwrap(),
            library_id: library_id,
            tmdb_series_id: 54321,
            details: MediaDetailsOption::Endpoint(format!("/api/series/s1/season/{}", i)),
            endpoint: ferrex_core::media::SeasonURL::from_string(format!("/api/series/s1/season/{}", i)),
            created_at: chrono::Utc::now(),
            theme_color: None,
        };
        store.upsert(MediaReference::Season(season));
    }
    
    // Get seasons for series
    let seasons = store.get_seasons("s1");
    assert_eq!(seasons.len(), 3, "Should return all seasons for series");
    assert!(seasons.iter().all(|s| s.series_id.as_str() == "s1"));
    
    // Verify season ordering
    assert_eq!(seasons[0].season_number.value(), 1);
    assert_eq!(seasons[1].season_number.value(), 2);
    assert_eq!(seasons[2].season_number.value(), 3);
}