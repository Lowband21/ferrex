use axum::{
    extract::{
        Extension, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
};
use ferrex_core::sync_session::SyncMessage;
use ferrex_core::user::User;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::infra::{
    app_state::AppState,
    websocket::{Connection, messages},
};

/// Handle WebSocket upgrade request
pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Extension(user): Extension<User>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state, user))
}

/// Handle an individual WebSocket connection
async fn handle_socket(socket: WebSocket, state: AppState, user: User) {
    let (ws_sender, mut ws_receiver) = socket.split();
    let (tx, mut rx) = mpsc::channel::<SyncMessage>(100);

    // Create connection
    let connection = Arc::new(Connection::new(user.clone(), tx));
    let conn_id = connection.id;

    // Register connection
    state
        .websocket_manager
        .add_connection(conn_id, connection.clone());

    // Spawn task to handle outgoing messages
    let mut ws_sender = ws_sender;
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Ok(ws_msg) = messages::sync_to_websocket(&msg) {
                if ws_sender.send(ws_msg).await.is_err() {
                    break;
                }
            }
        }
    });

    // Handle incoming messages
    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                if let Ok(sync_msg) = serde_json::from_str::<SyncMessage>(text.as_str()) {
                    if let Err(e) = handle_sync_message(sync_msg, &state, conn_id, &user).await {
                        tracing::error!("Error handling sync message: {}", e);
                    }
                }
            }
            Ok(Message::Binary(bin)) => {
                if let Ok(sync_msg) = serde_json::from_slice::<SyncMessage>(bin.as_ref()) {
                    if let Err(e) = handle_sync_message(sync_msg, &state, conn_id, &user).await {
                        tracing::error!("Error handling sync message: {}", e);
                    }
                }
            }
            Ok(Message::Ping(_)) => {
                // Update last ping time
                if let Some(conn) = state.websocket_manager.get_connection(&conn_id) {
                    conn.update_ping().await;
                }
            }
            Ok(Message::Close(_)) => {
                break;
            }
            Err(e) => {
                tracing::error!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }

    // Clean up on disconnect
    handle_disconnect(&state, conn_id, &user).await;
}

/// Handle host command - verify sender is host and update state
async fn handle_host_command<F>(
    state: &AppState,
    conn_id: Uuid,
    user: &User,
    msg: &SyncMessage,
    update_fn: F,
) -> anyhow::Result<()>
where
    F: FnOnce(&mut ferrex_core::sync_session::PlaybackState),
{
    if let Some(conn) = state.websocket_manager.get_connection(&conn_id) {
        if let Some(room_code) = conn.get_room_code().await {
            // Verify sender is host
            if let Some(session) = state
                .db
                .backend()
                .get_sync_session_by_code(&room_code)
                .await?
            {
                if session.host_id == user.id {
                    // Update session state
                    let mut new_state = session.state.clone();
                    update_fn(&mut new_state);

                    // Update database
                    state
                        .db
                        .backend()
                        .update_sync_session_state(session.id, &new_state)
                        .await?;

                    // Broadcast to all participants
                    state
                        .websocket_manager
                        .broadcast_to_room(&room_code, msg.clone())
                        .await;
                }
            }
        }
    }
    Ok(())
}

/// Handle a sync message
async fn handle_sync_message(
    msg: SyncMessage,
    state: &AppState,
    conn_id: Uuid,
    user: &User,
) -> anyhow::Result<()> {
    use SyncMessage::*;

    match msg {
        // Host commands - verify sender is host and broadcast to room
        Play {
            position,
            timestamp: _,
        } => {
            handle_host_command(state, conn_id, user, &msg, |state| {
                state.position = position;
                state.is_playing = true;
                state.last_sync = chrono::Utc::now().timestamp();
            })
            .await?;
        }
        Pause { position } => {
            handle_host_command(state, conn_id, user, &msg, |state| {
                state.position = position;
                state.is_playing = false;
                state.last_sync = chrono::Utc::now().timestamp();
            })
            .await?;
        }
        Seek { position } => {
            handle_host_command(state, conn_id, user, &msg, |state| {
                state.position = position;
                state.last_sync = chrono::Utc::now().timestamp();
            })
            .await?;
        }
        SetRate { rate } => {
            handle_host_command(state, conn_id, user, &msg, |state| {
                state.playback_rate = rate;
            })
            .await?;
        }

        // Participant status updates
        Ready { .. } | NotReady { .. } => {
            if let Some(conn) = state.websocket_manager.get_connection(&conn_id) {
                if let Some(room_code) = conn.get_room_code().await {
                    // Update participant ready status
                    if let Some(mut session) = state
                        .db
                        .backend()
                        .get_sync_session_by_code(&room_code)
                        .await?
                    {
                        for participant in &mut session.participants {
                            if participant.user_id == user.id {
                                participant.is_ready = matches!(msg, Ready { .. });
                                break;
                            }
                        }

                        // Update database
                        state
                            .db
                            .backend()
                            .update_sync_session(session.id, &session)
                            .await?;

                        // Notify host
                        if let Some(host_conn) = state
                            .websocket_manager
                            .get_room_connections(&room_code)
                            .into_iter()
                            .find(|c| c.user.id == session.host_id)
                        {
                            host_conn.send_message(msg).await?;
                        }
                    }
                }
            }
        }

        // Request sync - send current state
        RequestSync => {
            if let Some(conn) = state.websocket_manager.get_connection(&conn_id) {
                if let Some(room_code) = conn.get_room_code().await {
                    if let Some(session) = state
                        .db
                        .backend()
                        .get_sync_session_by_code(&room_code)
                        .await?
                    {
                        conn.send_message(SyncState {
                            state: session.state,
                        })
                        .await?;
                    }
                }
            }
        }

        // Handle ping/pong
        Ping { timestamp } => {
            if let Some(conn) = state.websocket_manager.get_connection(&conn_id) {
                conn.update_ping().await;
                conn.send_message(Pong { timestamp }).await?;
            }
        }

        Pong { .. } => {
            // Update last ping time
            if let Some(conn) = state.websocket_manager.get_connection(&conn_id) {
                conn.update_ping().await;
            }
        }

        // Server-initiated messages should not come from clients
        UserJoined { .. } | UserLeft { .. } | SyncState { .. } => {
            tracing::warn!("Client sent server-only message type");
        }
    }

    Ok(())
}

/// Handle user disconnect
async fn handle_disconnect(state: &AppState, conn_id: Uuid, user: &User) {
    // Get room code before removing connection
    let room_code = match state.websocket_manager.get_connection(&conn_id) {
        Some(conn) => conn.get_room_code().await,
        _ => None,
    };

    // Remove connection
    state.websocket_manager.remove_connection(conn_id);

    // Handle room cleanup if needed
    if let Some(room_code) = room_code {
        // Notify other participants
        state
            .websocket_manager
            .broadcast_to_room(&room_code, SyncMessage::UserLeft { user_id: user.id })
            .await;

        // Check if host left and migrate if needed
        if let Some(mut session) = state
            .db
            .backend()
            .get_sync_session_by_code(&room_code)
            .await
            .ok()
            .flatten()
        {
            if session.host_id == user.id {
                // Remove leaving user from participants
                session.remove_participant(user.id);

                // Migrate host to next participant if any
                if let Some(new_host) = session.participants.first() {
                    session.host_id = new_host.user_id;

                    // Update database
                    let _ = state
                        .db
                        .backend()
                        .update_sync_session(session.id, &session)
                        .await;

                    tracing::info!("Migrated host to user {}", new_host.user_id);
                } else {
                    // No participants left, end session
                    let _ = state.db.backend().end_sync_session(session.id).await;

                    tracing::info!("Ended session {} - no participants", session.id);
                }
            } else {
                // Just remove participant
                session.remove_participant(user.id);
                let _ = state
                    .db
                    .backend()
                    .update_sync_session(session.id, &session)
                    .await;
            }
        }
    }
}
