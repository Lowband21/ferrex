use crate::infra::app_state::AppState;
use crate::infra::errors::{AppError, AppResult};
use axum::{
    Json,
    extract::{Extension, Path, State},
};
use ferrex_core::{
    sync_session::{
        CreateSyncSessionRequest, CreateSyncSessionResponse, JoinSyncSessionResponse, Participant,
        PlaybackState, SyncSession, SyncSessionError,
    },
    traits::prelude::MediaIDLike,
    user::User,
};
use uuid::Uuid;

/// POST /api/sync/sessions - Create a new sync session
pub async fn create_sync_session_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(req): Json<CreateSyncSessionRequest>,
) -> AppResult<Json<CreateSyncSessionResponse>> {
    // Generate room code
    let room_code = SyncSession::generate_room_code();

    // Create session
    let session = SyncSession {
        id: Uuid::now_v7(),
        room_code: room_code.clone(),
        host_id: user.id,
        media_id: req.media_id.to_uuid(),
        media_type: req.media_id.media_type(),
        state: PlaybackState {
            position: 0.0,
            is_playing: false,
            playback_rate: 1.0,
            last_sync: chrono::Utc::now().timestamp(),
        },
        participants: vec![Participant {
            user_id: user.id,
            display_name: user.display_name.clone(),
            is_ready: false,
            latency_ms: 0,
            last_ping: chrono::Utc::now().timestamp(),
        }],
        created_at: chrono::Utc::now().timestamp(),
        expires_at: chrono::Utc::now().timestamp() + 86400, // 24 hours
    };

    // Store in database
    state
        .unit_of_work
        .sync_sessions
        .create_sync_session(&session)
        .await
        .map_err(|e| AppError::internal(format!("Failed to create sync session: {}", e)))?;

    // Note: Connection to room will be handled when user connects via WebSocket

    // Return response
    Ok(Json(CreateSyncSessionResponse {
        session_id: session.id,
        room_code,
        websocket_url: "/api/sync/ws".to_string(),
    }))
}

/// GET /api/sync/sessions/join/:code - Join a sync session by room code
pub async fn join_sync_session_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(room_code): Path<String>,
) -> AppResult<Json<JoinSyncSessionResponse>> {
    // Get session from database
    let mut session = state
        .unit_of_work
        .sync_sessions
        .get_sync_session_by_code(&room_code)
        .await
        .map_err(|e| AppError::internal(format!("Failed to get sync session: {}", e)))?
        .ok_or_else(|| AppError::not_found("Invalid room code"))?;

    // Check if session is expired
    if session.is_expired() {
        return Err(AppError::bad_request("Session expired"));
    }

    // Add participant
    let participant = Participant {
        user_id: user.id,
        display_name: user.display_name.clone(),
        is_ready: false,
        latency_ms: 0,
        last_ping: chrono::Utc::now().timestamp(),
    };

    session
        .add_participant(participant.clone())
        .map_err(|e| match e {
            SyncSessionError::SessionFull => AppError::bad_request("Session is full"),
            _ => AppError::internal(format!("Failed to add participant: {}", e)),
        })?;

    // Update database
    state
        .unit_of_work
        .sync_sessions
        .add_sync_participant(session.id, &participant)
        .await
        .map_err(|e| AppError::internal(format!("Failed to add participant: {}", e)))?;

    // Note: Connection to room will be handled when user connects via WebSocket

    // Notify other participants
    state
        .websocket_manager
        .broadcast_to_room(
            &session.room_code,
            ferrex_core::sync_session::SyncMessage::UserJoined { participant },
        )
        .await;

    // Return response
    Ok(Json(JoinSyncSessionResponse {
        session_id: session.id,
        media_id: session.media_id,
        websocket_url: "/api/sync/ws".to_string(),
        current_state: session.state,
        participants: session.participants,
    }))
}

/// DELETE /api/sync/sessions/:id - Leave or end a sync session
pub async fn leave_sync_session_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(session_id): Path<Uuid>,
) -> AppResult<Json<()>> {
    // Get session from database
    let mut session = state
        .unit_of_work
        .sync_sessions
        .get_sync_session(session_id)
        .await
        .map_err(|e| AppError::internal(format!("Failed to get sync session: {}", e)))?
        .ok_or_else(|| AppError::not_found("Session not found"))?;

    // Remove participant
    session.remove_participant(user.id);

    // Update database
    state
        .unit_of_work
        .sync_sessions
        .remove_sync_participant(session_id, user.id)
        .await
        .map_err(|e| AppError::internal(format!("Failed to remove participant: {}", e)))?;

    // Note: Connection cleanup will be handled by WebSocket disconnect

    // Check if user was host
    if session.host_id == user.id {
        // Migrate host or end session
        if let Some(new_host) = session.participants.first() {
            session.host_id = new_host.user_id;
            state
                .unit_of_work
                .sync_sessions
                .update_sync_session(session_id, &session)
                .await
                .map_err(|e| AppError::internal(format!("Failed to update session: {}", e)))?;
        } else {
            // No participants left, end session
            state
                .unit_of_work
                .sync_sessions
                .end_sync_session(session_id)
                .await
                .map_err(|e| AppError::internal(format!("Failed to end session: {}", e)))?;
        }
    }

    Ok(Json(()))
}

/// GET /api/sync/sessions/:id/state - Get current sync session state
pub async fn get_sync_session_state_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(session_id): Path<Uuid>,
) -> AppResult<Json<PlaybackState>> {
    // Get session from database
    let session = state
        .unit_of_work
        .sync_sessions
        .get_sync_session(session_id)
        .await
        .map_err(|e| AppError::internal(format!("Failed to get sync session: {}", e)))?
        .ok_or_else(|| AppError::not_found("Session not found"))?;

    // Check if user is participant
    if !session.participants.iter().any(|p| p.user_id == user.id) {
        return Err(AppError::forbidden("Not a participant in this session"));
    }

    Ok(Json(session.state))
}
