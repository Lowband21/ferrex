use crate::{ImageType, MediaID, MediaType};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Synchronized playback session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncSession {
    pub id: Uuid,
    pub room_code: String, // 6-char code like "ABC123"
    pub host_id: Uuid,
    pub media_id: Uuid,
    pub media_type: MediaType,
    pub state: PlaybackState,
    pub participants: Vec<Participant>,
    pub created_at: i64,
    pub expires_at: i64, // Auto-cleanup after 24h
}

/// Current playback state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackState {
    pub position: f64, // Seconds
    pub is_playing: bool,
    pub playback_rate: f32,
    pub last_sync: i64, // Unix timestamp for drift correction
}

/// Session participant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Participant {
    pub user_id: Uuid,
    pub display_name: String,
    pub is_ready: bool,  // Buffered and ready
    pub latency_ms: u32, // For sync compensation
    pub last_ping: i64,
}

/// WebSocket message types for sync sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SyncMessage {
    // Host -> Server -> Participants
    Play { position: f64, timestamp: i64 },
    Pause { position: f64 },
    Seek { position: f64 },
    SetRate { rate: f32 },

    // Participant -> Server -> Host
    Ready { user_id: Uuid },
    NotReady { user_id: Uuid },
    RequestSync,

    // Server -> All
    UserJoined { participant: Participant },
    UserLeft { user_id: Uuid },
    SyncState { state: PlaybackState },

    // Heartbeat
    Ping { timestamp: i64 },
    Pong { timestamp: i64 },
}

/// Request to create a sync session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSyncSessionRequest {
    pub media_id: MediaID,
}

/// Response after creating a sync session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSyncSessionResponse {
    pub session_id: Uuid,
    pub room_code: String,
    pub websocket_url: String,
}

/// Request to join a sync session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinSyncSessionRequest {
    pub room_code: String,
}

/// Response after joining a sync session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinSyncSessionResponse {
    pub session_id: Uuid,
    pub media_id: Uuid,
    pub websocket_url: String,
    pub current_state: PlaybackState,
    pub participants: Vec<Participant>,
}

/// Sync session errors
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
pub enum SyncSessionError {
    #[error("Invalid room code")]
    InvalidRoomCode,

    #[error("Session expired")]
    SessionExpired,

    #[error("Session full")]
    SessionFull,

    #[error("Not authorized")]
    NotAuthorized,

    #[error("Media not found")]
    MediaNotFound,
}

impl SyncSession {
    /// Generate a new room code
    pub fn generate_room_code() -> String {
        use rand::Rng;

        // Use alphanumeric without confusing chars (0, O, I, 1)
        const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

        let mut rng = rand::thread_rng();
        (0..6)
            .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
            .collect()
    }

    /// Check if the session has expired
    pub fn is_expired(&self) -> bool {
        chrono::Utc::now().timestamp() > self.expires_at
    }

    /// Add a participant to the session
    pub fn add_participant(&mut self, participant: Participant) -> Result<(), SyncSessionError> {
        // Limit to 10 participants
        if self.participants.len() >= 10 {
            return Err(SyncSessionError::SessionFull);
        }

        // Remove if already exists
        self.participants
            .retain(|p| p.user_id != participant.user_id);

        // Add new participant
        self.participants.push(participant);

        Ok(())
    }

    /// Remove a participant from the session
    pub fn remove_participant(&mut self, user_id: Uuid) {
        self.participants.retain(|p| p.user_id != user_id);
    }

    /// Check if all participants are ready
    pub fn all_ready(&self) -> bool {
        self.participants.iter().all(|p| p.is_ready)
    }
}

impl PlaybackState {
    /// Calculate current position accounting for elapsed time
    pub fn calculate_current_position(&self, now: i64) -> f64 {
        if !self.is_playing {
            self.position
        } else {
            let elapsed = (now - self.last_sync) as f64 / 1000.0;
            self.position + (elapsed * self.playback_rate as f64)
        }
    }

    /// Apply latency compensation for a participant
    pub fn apply_latency_compensation(&self, latency_ms: u32) -> f64 {
        // Add latency to position for smooth sync
        self.position + (latency_ms as f64 / 1000.0)
    }
}
