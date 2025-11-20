#[cfg(test)]
mod sync_session_tests {
    use ferrex_core::api_types::MediaID;
    use ferrex_core::sync_session::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    use uuid::Uuid;

    fn create_test_sync_session() -> SyncSession {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        SyncSession {
            id: Uuid::new_v4(),
            room_code: SyncSession::generate_room_code(),
            host_id: Uuid::new_v4(),
            media_id: MediaID::Movie(
                ferrex_core::media::MovieID::new("test_movie".to_string()).unwrap(),
            ),
            state: PlaybackState {
                position: 0.0,
                is_playing: false,
                playback_rate: 1.0,
                last_sync: now,
            },
            participants: Vec::new(),
            created_at: now,
            expires_at: now + 86400, // 24 hours
        }
    }

    fn create_test_participant(user_id: Uuid, name: &str) -> Participant {
        Participant {
            user_id,
            display_name: name.to_string(),
            is_ready: false,
            latency_ms: 50,
            last_ping: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
        }
    }

    #[test]
    fn test_room_code_generation() {
        let code1 = SyncSession::generate_room_code();
        let code2 = SyncSession::generate_room_code();

        // Should be 6 characters
        assert_eq!(code1.len(), 6);
        assert_eq!(code2.len(), 6);

        // Should be different
        assert_ne!(code1, code2);

        // Should only contain allowed characters (no confusing ones)
        let allowed_chars = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
        for c in code1.chars() {
            assert!(allowed_chars.contains(c));
        }
    }

    #[test]
    fn test_session_expiry() {
        let mut session = create_test_sync_session();

        // Not expired
        assert!(!session.is_expired());

        // Set to expired
        session.expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            - 100;

        assert!(session.is_expired());
    }

    #[test]
    fn test_add_participant() {
        let mut session = create_test_sync_session();
        let user1 = Uuid::new_v4();
        let participant1 = create_test_participant(user1, "User 1");

        // Add participant
        assert!(session.add_participant(participant1.clone()).is_ok());
        assert_eq!(session.participants.len(), 1);
        assert_eq!(session.participants[0].user_id, user1);
    }

    #[test]
    fn test_participant_limit() {
        let mut session = create_test_sync_session();

        // Add 10 participants (max)
        for i in 0..10 {
            let participant = create_test_participant(Uuid::new_v4(), &format!("User {}", i));
            assert!(session.add_participant(participant).is_ok());
        }

        assert_eq!(session.participants.len(), 10);

        // Try to add 11th participant
        let extra_participant = create_test_participant(Uuid::new_v4(), "Extra User");
        let result = session.add_participant(extra_participant);

        assert!(matches!(result, Err(SyncSessionError::SessionFull)));
        assert_eq!(session.participants.len(), 10);
    }

    #[test]
    fn test_participant_replacement() {
        let mut session = create_test_sync_session();
        let user_id = Uuid::new_v4();

        // Add participant
        let participant1 = create_test_participant(user_id, "User 1");
        session.add_participant(participant1).unwrap();
        assert_eq!(session.participants[0].display_name, "User 1");

        // Add same user with different name
        let participant2 = create_test_participant(user_id, "User 1 Updated");
        session.add_participant(participant2).unwrap();

        // Should replace, not add
        assert_eq!(session.participants.len(), 1);
        assert_eq!(session.participants[0].display_name, "User 1 Updated");
    }

    #[test]
    fn test_remove_participant() {
        let mut session = create_test_sync_session();
        let user1 = Uuid::new_v4();
        let user2 = Uuid::new_v4();

        session
            .add_participant(create_test_participant(user1, "User 1"))
            .unwrap();
        session
            .add_participant(create_test_participant(user2, "User 2"))
            .unwrap();
        assert_eq!(session.participants.len(), 2);

        // Remove user1
        session.remove_participant(user1);
        assert_eq!(session.participants.len(), 1);
        assert_eq!(session.participants[0].user_id, user2);

        // Remove non-existent user (should be no-op)
        session.remove_participant(Uuid::new_v4());
        assert_eq!(session.participants.len(), 1);
    }

    #[test]
    fn test_all_ready_check() {
        let mut session = create_test_sync_session();

        // Empty session is ready
        assert!(session.all_ready());

        // Add not-ready participant
        let mut participant1 = create_test_participant(Uuid::new_v4(), "User 1");
        participant1.is_ready = false;
        session.add_participant(participant1).unwrap();
        assert!(!session.all_ready());

        // Make them ready
        session.participants[0].is_ready = true;
        assert!(session.all_ready());

        // Add another not-ready participant
        let participant2 = create_test_participant(Uuid::new_v4(), "User 2");
        session.add_participant(participant2).unwrap();
        assert!(!session.all_ready());
    }

    #[test]
    fn test_playback_state_position_calculation() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let state = PlaybackState {
            position: 100.0,
            is_playing: false,
            playback_rate: 1.0,
            last_sync: now - 10,
        };

        // When paused, position should not change
        assert_eq!(state.calculate_current_position(now), 100.0);

        // When playing, position should advance
        let playing_state = PlaybackState {
            position: 100.0,
            is_playing: true,
            playback_rate: 1.0,
            last_sync: now - 10,
        };
        assert_eq!(playing_state.calculate_current_position(now), 110.0); // 10 seconds elapsed

        // With different playback rate
        let fast_state = PlaybackState {
            position: 100.0,
            is_playing: true,
            playback_rate: 2.0,
            last_sync: now - 10,
        };
        assert_eq!(fast_state.calculate_current_position(now), 120.0); // 10 seconds * 2x speed
    }

    #[test]
    fn test_latency_compensation() {
        let state = PlaybackState {
            position: 100.0,
            is_playing: true,
            playback_rate: 1.0,
            last_sync: 0,
        };

        // Apply 500ms latency compensation
        let compensated = state.apply_latency_compensation(500);
        assert_eq!(compensated, 100.5); // 100 + 0.5 seconds
    }

    #[test]
    fn test_sync_message_serialization() {
        use serde_json;

        let messages = vec![
            SyncMessage::Play {
                position: 123.45,
                timestamp: 1234567890,
            },
            SyncMessage::Pause { position: 234.56 },
            SyncMessage::Seek { position: 345.67 },
            SyncMessage::SetRate { rate: 1.5 },
            SyncMessage::Ready {
                user_id: Uuid::new_v4(),
            },
            SyncMessage::RequestSync,
            SyncMessage::Ping {
                timestamp: 1234567890,
            },
        ];

        for msg in messages {
            // Should serialize and deserialize correctly
            let json = serde_json::to_string(&msg).unwrap();
            let deserialized: SyncMessage = serde_json::from_str(&json).unwrap();

            // Verify type field is present and snake_case
            assert!(json.contains("\"type\":"));
            match msg {
                SyncMessage::Play { .. } => assert!(json.contains("\"type\":\"play\"")),
                SyncMessage::Pause { .. } => assert!(json.contains("\"type\":\"pause\"")),
                SyncMessage::Seek { .. } => assert!(json.contains("\"type\":\"seek\"")),
                SyncMessage::SetRate { .. } => assert!(json.contains("\"type\":\"set_rate\"")),
                SyncMessage::Ready { .. } => assert!(json.contains("\"type\":\"ready\"")),
                SyncMessage::RequestSync => assert!(json.contains("\"type\":\"request_sync\"")),
                SyncMessage::Ping { .. } => assert!(json.contains("\"type\":\"ping\"")),
                _ => {}
            }
        }
    }

    #[test]
    fn test_create_session_request_response() {
        let request = CreateSyncSessionRequest {
            media_id: MediaID::Movie(
                ferrex_core::media::MovieID::new("test_movie".to_string()).unwrap(),
            ),
        };

        let response = CreateSyncSessionResponse {
            session_id: Uuid::new_v4(),
            room_code: "ABC123".to_string(),
            websocket_url: "ws://localhost:3000/sync/ABC123".to_string(),
        };

        assert_eq!(response.room_code.len(), 6);
        assert!(response.websocket_url.contains(&response.room_code));
    }

    #[test]
    fn test_join_session_request_response() {
        let request = JoinSyncSessionRequest {
            room_code: "XYZ789".to_string(),
        };

        let participants = vec![
            create_test_participant(Uuid::new_v4(), "Host"),
            create_test_participant(Uuid::new_v4(), "Guest"),
        ];

        let response = JoinSyncSessionResponse {
            session_id: Uuid::new_v4(),
            media_id: MediaID::Episode(
                ferrex_core::media::EpisodeID::new("show123".to_string()).unwrap(),
            ),
            websocket_url: "ws://localhost:3000/sync/XYZ789".to_string(),
            current_state: PlaybackState {
                position: 1234.5,
                is_playing: true,
                playback_rate: 1.0,
                last_sync: 1234567890,
            },
            participants,
        };

        assert_eq!(response.participants.len(), 2);
        assert_eq!(response.current_state.position, 1234.5);
    }

    #[test]
    fn test_sync_session_errors() {
        let errors = vec![
            (SyncSessionError::InvalidRoomCode, "Invalid room code"),
            (SyncSessionError::SessionExpired, "Session expired"),
            (SyncSessionError::SessionFull, "Session full"),
            (SyncSessionError::NotAuthorized, "Not authorized"),
            (SyncSessionError::MediaNotFound, "Media not found"),
        ];

        for (error, expected_msg) in errors {
            assert_eq!(error.to_string(), expected_msg);
        }
    }
}
