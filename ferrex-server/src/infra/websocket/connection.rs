use anyhow::Result;
use ferrex_core::sync_session::SyncMessage;
use ferrex_core::user::User;
use std::{fmt, sync::Arc};
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

#[derive(Clone)]
pub struct Connection {
    /// Unique connection ID
    pub id: Uuid,
    /// User associated with this connection
    pub user: Arc<User>,
    /// Current room code (if in a sync session)
    pub room_code: Arc<RwLock<Option<String>>>,
    /// Channel to send messages to this connection
    sender: mpsc::Sender<SyncMessage>,
    /// Last ping timestamp for connection health
    pub last_ping: Arc<RwLock<i64>>,
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let room_code = self
            .room_code
            .try_read()
            .ok()
            .and_then(|guard| guard.clone());
        let last_ping = self.last_ping.try_read().ok().map(|guard| *guard);

        f.debug_struct("Connection")
            .field("id", &self.id)
            .field("user_id", &self.user.id)
            .field("username", &self.user.username)
            .field("room_code", &room_code)
            .field("channel_closed", &self.sender.is_closed())
            .field("last_ping", &last_ping)
            .finish()
    }
}

impl Connection {
    pub fn new(user: User, sender: mpsc::Sender<SyncMessage>) -> Self {
        Self {
            id: Uuid::now_v7(),
            user: Arc::new(user),
            room_code: Arc::new(RwLock::new(None)),
            sender,
            last_ping: Arc::new(RwLock::new(chrono::Utc::now().timestamp())),
        }
    }

    /// Send a message to this connection
    pub async fn send_message(&self, message: SyncMessage) -> Result<()> {
        self.sender.send(message).await.map_err(|_| {
            anyhow::anyhow!("Failed to send message: channel closed")
        })
    }

    /// Update the current room code
    pub async fn set_room_code(&self, room_code: Option<String>) {
        *self.room_code.write().await = room_code;
    }

    /// Get the current room code
    pub async fn get_room_code(&self) -> Option<String> {
        self.room_code.read().await.clone()
    }

    /// Update last ping timestamp
    pub async fn update_ping(&self) {
        *self.last_ping.write().await = chrono::Utc::now().timestamp();
    }

    /// Check if connection is healthy (pinged within last 60 seconds)
    pub async fn is_healthy(&self) -> bool {
        let last_ping = *self.last_ping.read().await;
        let now = chrono::Utc::now().timestamp();
        now - last_ping < 60
    }
}
