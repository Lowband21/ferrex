use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;
use ferrex_core::sync_session::SyncMessage;
use ferrex_core::user::User;
use anyhow::Result;

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

impl Connection {
    pub fn new(user: User, sender: mpsc::Sender<SyncMessage>) -> Self {
        Self {
            id: Uuid::new_v4(),
            user: Arc::new(user),
            room_code: Arc::new(RwLock::new(None)),
            sender,
            last_ping: Arc::new(RwLock::new(chrono::Utc::now().timestamp())),
        }
    }
    
    /// Send a message to this connection
    pub async fn send_message(&self, message: SyncMessage) -> Result<()> {
        self.sender
            .send(message)
            .await
            .map_err(|_| anyhow::anyhow!("Failed to send message: channel closed"))
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