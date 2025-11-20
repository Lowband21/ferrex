use super::messages::Message;
use crate::state_refactored::State;
use crate::common::messages::{DomainMessage, DomainUpdateResult};
use crate::infrastructure::services::api::ApiService;
use iced::Task;

/// Handle media domain messages - focused on media management, not playback
pub fn update_media(state: &mut State, message: Message) -> DomainUpdateResult {

    match message {
        // Media management messages
        Message::PlayMedia(media_file) => {
            super::update_handlers::play_media::handle_play_media(state, media_file)
        }

        Message::PlayMediaWithId(media, media_id) => {
            // Store the MediaId for watch status tracking
            state.domains.media.state.current_media_id = Some(media_id.clone());
            state.domains.player.state.current_media_id = Some(media_id);
            super::update_handlers::play_media::handle_play_media(state, media)
        }
        
        Message::LoadMediaById(media_id) => {
            // Load a media file by its ID from the media store - O(1) efficient lookup
            log::info!("Loading media by ID: {:?}", media_id);
            
            // Efficient O(1) lookup in media store (with read lock)
            let core_media_file = {
                let store = state.domains.media.state.media_store.read().unwrap();
                store.get_media_file_by_id(&media_id)
            };
            
            if let Some(core_file) = core_media_file {
                // Convert from core type to player type (temporary until migration complete)
                let player_media_file = crate::domains::media::library::MediaFile::from(core_file);
                
                // Play the media with ID tracking
                update_media(state, Message::PlayMediaWithId(player_media_file, media_id))
            } else {
                log::error!("Media not found for ID: {:?}", media_id);
                // Return an error view
                state.domains.ui.state.view = crate::domains::ui::types::ViewState::VideoError {
                    message: format!("Media not found: {:?}", media_id),
                };
                DomainUpdateResult::task(Task::none())
            }
        }

        Message::VideoLoaded(success) => {
            if success {
                log::info!("Video loaded successfully");
                state.domains.ui.state.view = crate::domains::ui::types::ViewState::Player;
                DomainUpdateResult::task(Task::none())
            } else {
                log::error!("Video failed to load");
                state.domains.ui.state.view = crate::domains::ui::types::ViewState::VideoError {
                    message: "Failed to load video".to_string(),
                };
                DomainUpdateResult::task(Task::none())
            }
        }

        Message::VideoCreated(result) => {
            match result {
                Ok(video_arc) => {
                    log::info!("Video object created successfully");
                    // Try to extract the video from the Arc
                    match std::sync::Arc::try_unwrap(video_arc) {
                        Ok(video) => {
                            state.domains.player.state.video_opt = Some(video);
                            state.domains.player.state.is_loading_video = false;
                            // Notify that video is loaded
                            state.domains.ui.state.view = crate::domains::ui::types::ViewState::Player;
                            // Start playing immediately
                            if let Some(video) = &mut state.domains.player.state.video_opt {
                                video.set_paused(false);
                            }
                            DomainUpdateResult::task(Task::done(DomainMessage::Media(Message::VideoLoaded(true))))
                        }
                        Err(_) => {
                            log::error!("Failed to unwrap Arc<Video> - multiple references exist");
                            DomainUpdateResult::task(Task::done(DomainMessage::Media(Message::VideoLoaded(false))))
                        }
                    }
                }
                Err(error) => {
                    log::error!("Failed to create video: {}", error);
                    state.domains.ui.state.error_message = Some(error.clone());
                    state.domains.ui.state.view =
                        crate::domains::ui::types::ViewState::VideoError { message: error };
                    DomainUpdateResult::task(Task::none())
                }
            }
        }

        Message::_LoadVideo => {
            // Load the video directly
            DomainUpdateResult::task(crate::domains::player::video::load_video(state).map(DomainMessage::Media))
        }

        Message::MediaAvailabilityChecked(media_file) => {
            log::info!("Media availability confirmed for: {}", media_file.filename);
            // Proceed with playing the media
            DomainUpdateResult::task(Task::done(DomainMessage::Media(Message::PlayMedia(media_file))))
        }

        Message::MediaUnavailable(reason, message) => {
            super::update_handlers::play_media::handle_media_unavailable(state, reason, message)
        }

        // SeekBarMoved is now handled in the player domain via global mouse tracking
        Message::SeekBarMoved(_point) => {
            log::warn!("SeekBarMoved should be handled in player domain, not media domain");
            DomainUpdateResult::task(Task::none())
        }

        // Handle CheckControlsVisibility - bridge to player domain
        Message::CheckControlsVisibility => {
            use std::time::Duration;

            // Check if controls should be hidden based on inactivity
            if state.domains.player.state.controls
                && state.domains.player.state.controls_time.elapsed() > Duration::from_secs(3)
            {
                state.domains.player.state.controls = false;
            }

            DomainUpdateResult::task(Task::none())
        }
        
        // Handle watch progress tracking
        Message::ProgressUpdateSent(position) => {
            // Update the last sent position
            state.domains.player.state.last_progress_sent = position;
            state.domains.player.state.last_progress_update = Some(std::time::Instant::now());
            DomainUpdateResult::task(Task::none())
        }
        
        Message::ProgressUpdateFailed => {
            // Log was already done in subscription, just track the failure
            log::debug!("Progress update failed, will retry on next interval");
            DomainUpdateResult::task(Task::none())
        }
        
        Message::SendProgressUpdate => {
            // Send an immediate progress update
            if let (Some(api_service), Some(media_id)) = (
                &state.domains.media.state.api_service,
                &state.domains.media.state.current_media_id,
            ) {
                let position = state.domains.player.state.position;
                let duration = state.domains.player.state.duration;
                
                if position > 0.0 && duration > 0.0 {
                    let api_service = api_service.clone();
                    let media_id = media_id.clone();
                    let position_copy = position; // Copy for closure
                    
                    return DomainUpdateResult::task(Task::perform(
                        async move {
                            let request = ferrex_core::watch_status::UpdateProgressRequest {
                                media_id,
                                position: position_copy as f32,
                                duration: duration as f32,
                            };
                            api_service.update_progress(&request).await.map(|_| position_copy)
                        },
                        |result| match result {
                            Ok(pos) => DomainMessage::Media(Message::ProgressUpdateSent(pos)),
                            Err(e) => {
                                log::warn!("Failed to send progress update: {}", e);
                                DomainMessage::Media(Message::ProgressUpdateFailed)
                            }
                        }
                    ));
                }
            }
            DomainUpdateResult::task(Task::none())
        }

        // All other messages are now handled by player domain
        _ => {
            log::warn!("Media domain received unhandled message: {:?}", message);
            DomainUpdateResult::task(Task::none())
        }
    }
}
