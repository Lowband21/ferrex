use crate::infra::websocket::connection::Connection;
use dashmap::DashMap;
use ferrex_core::sync_session::SyncMessage;
use std::{fmt, sync::Arc};
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Clone)]
pub struct ConnectionManager {
    /// Active WebSocket connections mapped by connection ID
    connections: Arc<DashMap<Uuid, Arc<Connection>>>,
    /// Session rooms - maps room code to list of connection IDs
    rooms: Arc<DashMap<String, Vec<Uuid>>>,
    /// Broadcast channel for sync messages
    broadcast: Arc<broadcast::Sender<(String, SyncMessage)>>,
}

impl fmt::Debug for ConnectionManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionManager")
            .field("connection_count", &self.connections.len())
            .field("room_count", &self.rooms.len())
            .field("broadcast_receivers", &self.broadcast.receiver_count())
            .finish()
    }
}

impl ConnectionManager {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1024);

        Self {
            connections: Arc::new(DashMap::new()),
            rooms: Arc::new(DashMap::new()),
            broadcast: Arc::new(tx),
        }
    }

    /// Register a new connection
    pub fn add_connection(&self, conn_id: Uuid, connection: Arc<Connection>) {
        self.connections.insert(conn_id, connection);
    }

    /// Remove a connection and clean up room membership
    pub fn remove_connection(&self, conn_id: Uuid) {
        // Remove from connections
        self.connections.remove(&conn_id);

        // Remove from all rooms
        for mut room in self.rooms.iter_mut() {
            room.value_mut().retain(|id| id != &conn_id);
        }

        // Clean up empty rooms
        self.rooms.retain(|_, connections| !connections.is_empty());
    }

    /// Add a connection to a room
    pub fn join_room(&self, room_code: String, conn_id: Uuid) {
        self.rooms.entry(room_code).or_default().push(conn_id);
    }

    /// Remove a connection from a room
    pub fn leave_room(&self, room_code: &str, conn_id: Uuid) {
        if let Some(mut room) = self.rooms.get_mut(room_code) {
            room.value_mut().retain(|id| id != &conn_id);
        }

        // Clean up empty room
        if let Some(room) = self.rooms.get(room_code)
            && room.is_empty()
        {
            self.rooms.remove(room_code);
        }
    }

    /// Get all connections in a room
    pub fn get_room_connections(&self, room_code: &str) -> Vec<Arc<Connection>> {
        self.rooms
            .get(room_code)
            .map(|room| {
                room.iter()
                    .filter_map(|conn_id| self.connections.get(conn_id).map(|c| c.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Broadcast a message to all connections in a room
    pub async fn broadcast_to_room(&self, room_code: &str, message: SyncMessage) {
        let connections = self.get_room_connections(room_code);

        for conn in connections {
            if let Err(e) = conn.send_message(message.clone()).await {
                tracing::error!("Failed to send message to connection: {}", e);
            }
        }
    }

    /// Get a specific connection
    pub fn get_connection(&self, conn_id: &Uuid) -> Option<Arc<Connection>> {
        self.connections.get(conn_id).map(|c| c.clone())
    }

    /// Subscribe to broadcast messages
    pub fn subscribe(&self) -> broadcast::Receiver<(String, SyncMessage)> {
        self.broadcast.subscribe()
    }

    /// Send a broadcast message
    pub fn send_broadcast(&self, room_code: String, message: SyncMessage) {
        let _ = self.broadcast.send((room_code, message));
    }
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}
