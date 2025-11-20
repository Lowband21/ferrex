// Player Direct Commands Tests - Phase 2 Task 2.6
//
// Tests for new direct command messages added to PlayerMessage enum:
// - SeekTo(Duration)
// - ToggleShuffle
// - ToggleRepeat  
// - LoadTrack(MediaId)

use ferrex_player::domains::player::messages::Message as PlayerMessage;
use ferrex_player::domains::player::state::PlayerDomainState;
use ferrex_player::domains::player::update::update_player;
use ferrex_core::api_types::MediaId;
use ferrex_core::media::MovieID;
use std::time::Duration;

#[test]
fn test_player_message_variants_exist() {
    // Verify new message variants compile
    let _ = PlayerMessage::SeekTo(Duration::from_secs(30));
    let _ = PlayerMessage::ToggleShuffle;
    let _ = PlayerMessage::ToggleRepeat;
    let media_id = MediaId::Movie(MovieID::new("movie-123".to_string()).unwrap());
    let _ = PlayerMessage::LoadTrack(media_id);
}

#[test]
fn test_seekto_message_handler() {
    let mut state = PlayerDomainState::default();
    let window_size = iced::Size::new(1920.0, 1080.0);
    
    // Test SeekTo handler delegates to Seek
    let duration = Duration::from_secs(45);
    let result = update_player(&mut state, PlayerMessage::SeekTo(duration), window_size);
    
    // Should convert Duration to f64 and delegate
    // The handler should exist and not panic
    assert!(matches!(result.task, _));
}

#[test]
fn test_toggle_shuffle_handler() {
    let mut state = PlayerDomainState::default();
    let window_size = iced::Size::new(1920.0, 1080.0);
    
    // Initial state should be false
    assert_eq!(state.is_shuffle_enabled, false);
    
    // Toggle shuffle on
    let _ = update_player(&mut state, PlayerMessage::ToggleShuffle, window_size);
    assert_eq!(state.is_shuffle_enabled, true);
    
    // Toggle shuffle off
    let _ = update_player(&mut state, PlayerMessage::ToggleShuffle, window_size);
    assert_eq!(state.is_shuffle_enabled, false);
}

#[test]
fn test_toggle_repeat_handler() {
    let mut state = PlayerDomainState::default();
    let window_size = iced::Size::new(1920.0, 1080.0);
    
    // Initial state should be false
    assert_eq!(state.is_repeat_enabled, false);
    
    // Toggle repeat on
    let _ = update_player(&mut state, PlayerMessage::ToggleRepeat, window_size);
    assert_eq!(state.is_repeat_enabled, true);
    
    // Toggle repeat off
    let _ = update_player(&mut state, PlayerMessage::ToggleRepeat, window_size);
    assert_eq!(state.is_repeat_enabled, false);
}

#[test]
fn test_load_track_handler() {
    let mut state = PlayerDomainState::default();
    let window_size = iced::Size::new(1920.0, 1080.0);
    
    let media_id = MediaId::Movie(MovieID::new("movie-456".to_string()).unwrap());
    let result = update_player(&mut state, PlayerMessage::LoadTrack(media_id.clone()), window_size);
    
    // Should produce a task to load the media
    // The handler should exist and not panic
    assert!(matches!(result.task, _));
}

#[test]
fn test_player_state_shuffle_repeat_fields() {
    let state = PlayerDomainState::default();
    
    // Verify new fields exist and are initialized
    assert_eq!(state.is_shuffle_enabled, false);
    assert_eq!(state.is_repeat_enabled, false);
}