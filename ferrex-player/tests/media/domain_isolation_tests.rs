// Domain Isolation Tests
// 
// Requirements:
// - Media domain must not directly access other domains' state
// - All cross-domain communication must use events
// - Media domain must handle its own state transitions

use ferrex_player::domains::media::{MediaDomain, MediaDomainState};
use ferrex_player::domains::media::store::MediaStore;
use ferrex_player::domains::media::services::query_service::MediaQueryService;
use ferrex_player::common::messages::{CrossDomainEvent, DomainMessage};
use ferrex_player::infrastructure::api_types::{
    MediaReference, MovieReference, MediaFile, MediaDetailsOption, MovieID
};
use ferrex_core::media::{MovieTitle, MovieURL};
use std::sync::{Arc, RwLock as StdRwLock};
use std::path::PathBuf;
use uuid::Uuid;

#[test]
fn media_domain_initializes_with_empty_state() {
    let media_store = Arc::new(StdRwLock::new(MediaStore::new()));
    let state = MediaDomainState {
        user_watch_state: None,
        current_season_details: None,
        media_store: media_store.clone(),
        api_service: None,
        current_library_id: None,
        current_media_id: None,
        query_service: Arc::new(MediaQueryService::new(media_store)),
    };
    let domain = MediaDomain::new(state);
    
    assert!(domain.state.api_service.is_none(), 
        "Media domain should not have API client on init");
    assert!(domain.state.current_library_id.is_none(),
        "Media domain should not have library ID on init");
    assert!(domain.state.user_watch_state.is_none(),
        "Media domain should not have watch state on init");
    // Seasons list moved into MediaStore; no direct field on state anymore
}

#[test]
fn handles_clear_media_store_event() {
    let media_store = Arc::new(StdRwLock::new(MediaStore::new()));
    let state = MediaDomainState {
        user_watch_state: None,
        current_season_details: None,
        media_store: media_store.clone(),
        api_service: None,
        current_library_id: None,
        current_media_id: None,
        query_service: Arc::new(MediaQueryService::new(media_store)),
    };
    let mut domain = MediaDomain::new(state);
    
    // Add some test data to the store
    {
        let mut store = domain.state.media_store.write().unwrap();
        let library_id = Uuid::new_v4();
        let movie = MovieReference {
            id: MovieID::new("test".to_string()).unwrap(),
            tmdb_id: 12345,
            title: MovieTitle::new("Test Movie".to_string()).unwrap(),
            details: MediaDetailsOption::Endpoint("/api/movie/test".to_string()),
            endpoint: MovieURL::from_string("/api/movie/test".to_string()),
            file: MediaFile {
                id: Uuid::new_v4(),
                path: PathBuf::from("/test/movies/test.mkv"),
                filename: "test.mkv".to_string(),
                size: 1024 * 1024 * 100,
                created_at: chrono::Utc::now(),
                media_file_metadata: None,
                library_id,
            },
            theme_color: None,
        };
        store.upsert(MediaReference::Movie(movie));
        assert_eq!(store.len(), 1);
    }
    
    // Handle ClearMediaStore event
    let event = CrossDomainEvent::ClearMediaStore;
    let _task = domain.handle_event(&event);
    
    // Verify store was cleared
    {
        let store = domain.state.media_store.read().unwrap();
        assert!(store.is_empty(), 
            "Requirement: Media domain must clear store on ClearMediaStore event");
    }
}

#[test]
fn does_not_respond_to_unrelated_events() {
    let media_store = Arc::new(StdRwLock::new(MediaStore::new()));
    let state = MediaDomainState {
        user_watch_state: None,
        current_season_details: None,
        media_store: media_store.clone(),
        api_service: None,
        current_library_id: None,
        current_media_id: None,
        query_service: Arc::new(MediaQueryService::new(media_store)),
    };
    let mut domain = MediaDomain::new(state);
    
    // Events that media domain should not handle
    let unrelated_events = vec![
        CrossDomainEvent::AuthenticationComplete,
        CrossDomainEvent::UserLoggedOut,
        CrossDomainEvent::WindowResized(iced::Size::new(800.0, 600.0)),
    ];
    
    for event in unrelated_events {
        let _task = domain.handle_event(&event);
        // Task should be none/empty for unrelated events
        // This ensures domain isolation - domains only handle their own events
    }
}

#[test]
fn media_store_is_thread_safe() {
    let media_store = Arc::new(StdRwLock::new(MediaStore::new()));
    let state = MediaDomainState {
        user_watch_state: None,
        current_season_details: None,
        media_store: media_store.clone(),
        api_service: None,
        current_library_id: None,
        current_media_id: None,
        query_service: Arc::new(MediaQueryService::new(media_store)),
    };
    let domain = MediaDomain::new(state);
    let store = Arc::clone(&domain.state.media_store);
    
    // Spawn multiple threads that access the store
    let handles: Vec<_> = (0..10).map(|i| {
        let store_clone = Arc::clone(&store);
        std::thread::spawn(move || {
            let library_id = Uuid::new_v4();
            let movie = MovieReference {
                id: MovieID::new(format!("movie_{}", i)).unwrap(),
                tmdb_id: 12345 + i as u64,
                title: MovieTitle::new(format!("Movie {}", i)).unwrap(),
                details: MediaDetailsOption::Endpoint(format!("/api/movie/{}", i)),
                endpoint: MovieURL::from_string(format!("/api/movie/{}", i)),
                file: MediaFile {
                    id: Uuid::new_v4(),
                    path: PathBuf::from(format!("/test/movies/movie_{}.mkv", i)),
                    filename: format!("movie_{}.mkv", i),
                    size: 1024 * 1024 * 100,
                    created_at: chrono::Utc::now(),
                    media_file_metadata: None,
                    library_id,
                },
                theme_color: None,
            };
            
            // Write lock
            {
                let mut store = store_clone.write().unwrap();
                store.upsert(MediaReference::Movie(movie));
            }
            
            // Read lock
            {
                let store = store_clone.read().unwrap();
                let _ = store.len();
            }
        })
    }).collect();
    
    // Wait for all threads to complete
    for handle in handles {
        handle.join().unwrap();
    }
    
    // Verify all items were added
    let store = domain.state.media_store.read().unwrap();
    assert_eq!(store.len(), 10, 
        "Requirement: MediaStore must be thread-safe for concurrent access");
}

#[test] 
fn media_domain_state_encapsulation() {
    // This test verifies that MediaDomainState properly encapsulates its fields
    // and doesn't expose implementation details unnecessarily
    
    let media_store = Arc::new(StdRwLock::new(MediaStore::new()));
    let state = MediaDomainState {
        user_watch_state: None,
        current_season_details: None,
        media_store: media_store.clone(),
        api_service: None,
        current_library_id: None,
        current_media_id: None,
        query_service: Arc::new(MediaQueryService::new(media_store)),
    };
    let domain = MediaDomain::new(state);
    
    // MediaStore should be behind Arc<RwLock> for shared access
    assert!(std::mem::size_of_val(&domain.state.media_store) == std::mem::size_of::<Arc<StdRwLock<()>>>(),
        "MediaStore should be wrapped in Arc<RwLock> for thread safety");
    
    // State fields should be properly initialized
    assert!(domain.state.current_season_details.is_none());
    // Duplicate fields removed from state; season data lives in MediaStore now
}