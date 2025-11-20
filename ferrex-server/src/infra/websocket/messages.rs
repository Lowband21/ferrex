use anyhow::Result;
use axum::extract::ws::{Message, Utf8Bytes};
use ferrex_core::sync_session::SyncMessage;

/// Convert a SyncMessage to a WebSocket message
pub fn sync_to_websocket(msg: &SyncMessage) -> Result<Message> {
    let json = serde_json::to_string(msg)?;
    Ok(Message::Text(Utf8Bytes::from(json)))
}

/// Convert a WebSocket message to a SyncMessage
pub fn websocket_to_sync(msg: Message) -> Result<SyncMessage> {
    match msg {
        Message::Text(text) => {
            let sync_msg: SyncMessage = serde_json::from_str(text.as_str())?;
            Ok(sync_msg)
        }
        Message::Binary(bin) => {
            let sync_msg: SyncMessage = serde_json::from_slice(bin.as_ref())?;
            Ok(sync_msg)
        }
        _ => Err(anyhow::anyhow!("Unsupported message type")),
    }
}

/// Create a ping message
pub fn create_ping() -> SyncMessage {
    SyncMessage::Ping {
        timestamp: chrono::Utc::now().timestamp_millis(),
    }
}

/// Create a pong response
pub fn create_pong() -> SyncMessage {
    SyncMessage::Pong {
        timestamp: chrono::Utc::now().timestamp_millis(),
    }
}
