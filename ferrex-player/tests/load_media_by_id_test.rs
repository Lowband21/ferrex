// LoadMediaById Tests - Efficient O(1) media lookup
//
// Tests for the LoadMediaById functionality that provides
// efficient O(1) lookup of media files by MediaId

use ferrex_player::domains::media::messages::Message as MediaMessage;
use ferrex_core::api_types::MediaId;
use ferrex_core::media::{MovieID, MediaFile};
use std::path::PathBuf;
use uuid::Uuid;

// Note: MediaStore tests removed as the module is internal
// The O(1) lookup is tested through integration tests

#[test]
fn test_load_media_by_id_message_exists() {
    // Verify the message variant exists and compiles
    let media_id = MediaId::Movie(MovieID::new("test".to_string()).unwrap());
    let _ = MediaMessage::LoadMediaById(media_id);
}

#[test]
fn test_media_file_conversion_from_core() {
    // Test conversion from core MediaFile to player MediaFile
    let core_file = ferrex_core::media::MediaFile {
        id: Uuid::new_v4(),
        path: PathBuf::from("/test/movie.mp4"),
        filename: "movie.mp4".to_string(),
        size: 1000000,
        created_at: chrono::Utc::now(),
        media_file_metadata: None,
        library_id: Uuid::new_v4(),
    };
    
    // Convert to player MediaFile type
    let player_file = ferrex_player::domains::media::library::MediaFile::from(core_file.clone());
    
    assert_eq!(player_file.filename, "movie.mp4");
    assert_eq!(player_file.path, "/test/movie.mp4");
    assert_eq!(player_file.size, 1000000);
    assert_eq!(player_file.id, core_file.id.to_string());
    assert_eq!(player_file.library_id, Some(core_file.library_id.to_string()));
}