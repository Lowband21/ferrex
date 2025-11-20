use crate::{messages::media, player::messages::PlayerMessage, state::State};
use iced::Task;

/// Handle media domain messages by routing to appropriate handlers
pub fn update_media(state: &mut State, message: media::Message) -> Task<media::Message> {
    // Handle internal cross-domain event emission first (before message is moved)
    if let media::Message::_EmitCrossDomainEvent(event) = message {
        // Pass through the cross-domain event
        return Task::done(media::Message::_EmitCrossDomainEvent(event));
    }

    // Check if this is a player message that should be handled by the player module
    if PlayerMessage::is_player_message(&message) {
        // Convert to PlayerMessage and delegate to player module
        if let Some(player_msg) = PlayerMessage::from_main_message(message.clone()) {
            let task = state.player.update(player_msg);
            // Convert the result back to media::Message
            return task.map(|player_msg| {
                // Convert PlayerMessage back to media::Message
                // For now, just return NoOp since we need to implement proper conversion
                media::Message::Play // TODO: Implement proper conversion
            });
        }
    }

    // Handle non-player media messages
    match message {
        // These are handled by player module through delegation above
        media::Message::Play
        | media::Message::Pause
        | media::Message::PlayPause
        | media::Message::Stop
        | media::Message::BackToLibrary
        | media::Message::Seek(_)
        | media::Message::SeekRelative(_)
        | media::Message::SeekRelease
        | media::Message::SeekBarPressed
        | media::Message::SeekDone
        | media::Message::SeekForward
        | media::Message::SeekBackward
        | media::Message::SetVolume(_)
        | media::Message::ToggleMute
        | media::Message::EndOfStream
        | media::Message::NewFrame
        | media::Message::Reload
        | media::Message::ShowControls
        | media::Message::AudioTrackSelected(_)
        | media::Message::SubtitleTrackSelected(_)
        | media::Message::ToggleSubtitles
        | media::Message::ToggleSubtitleMenu
        | media::Message::CycleAudioTrack
        | media::Message::CycleSubtitleTrack
        | media::Message::CycleSubtitleSimple
        | media::Message::TracksLoaded
        | media::Message::SetPlaybackSpeed(_)
        | media::Message::ToggleSettings
        | media::Message::SetAspectRatio(_)
        | media::Message::ToggleFullscreen
        | media::Message::ExitFullscreen
        | media::Message::MouseMoved
        | media::Message::VideoClicked
        | media::Message::VideoDoubleClicked
        | media::Message::TranscodingStarted
        | media::Message::ToggleToneMapping(_)
        | media::Message::SetToneMappingPreset(_)
        | media::Message::SetToneMappingAlgorithm(_)
        | media::Message::SetToneMappingWhitePoint(_)
        | media::Message::SetToneMappingExposure(_)
        | media::Message::SetToneMappingSaturation(_)
        | media::Message::SetHableShoulderStrength(_)
        | media::Message::SetHableLinearStrength(_)
        | media::Message::SetHableLinearAngle(_)
        | media::Message::SetHableToeStrength(_)
        | media::Message::SetMonitorBrightness(_)
        | media::Message::SetToneMappingBrightness(_)
        | media::Message::SetToneMappingContrast(_)
        | media::Message::SetToneMappingSaturationBoost(_) => {
            // Should have been handled by player module delegation above
            unreachable!("Player message {:?} should have been delegated", message)
        }

        // This variant is handled at the beginning of the function
        media::Message::_EmitCrossDomainEvent(_) => {
            unreachable!("_EmitCrossDomainEvent should have been handled earlier")
        }

        // Handle CheckControlsVisibility directly until player module uses domain messages
        media::Message::CheckControlsVisibility => {
            use std::time::Duration;

            // Check if controls should be hidden based on inactivity
            if state.player.controls
                && state.player.controls_time.elapsed() > Duration::from_secs(3)
            {
                state.player.controls = false;
            }

            Task::none()
        }

        // SeekBarMoved needs special handling with window dimensions
        media::Message::SeekBarMoved(point) => {
            use std::time::{Duration, Instant};

            // Calculate seek position based on window width
            // Assume seek bar spans full window width
            let percentage = (point.x / state.window_size.width).clamp(0.0, 1.0) as f64;

            // Use source duration if available (for HLS this is the full media duration)
            let duration = state
                .player
                .source_duration
                .unwrap_or(state.player.duration);
            let seek_position = percentage * duration;

            // Always store the position for potential clicks
            state.player.last_seek_position = Some(seek_position);

            // If dragging, throttle seeks to prevent overwhelming the network
            if state.player.dragging {
                // Update visual position immediately for responsive UI
                state.player.position = seek_position;
                state.player.update_controls(true);

                // Check if we should perform actual seek (throttle to ~100ms intervals)
                let should_seek = match state.player.last_seek_time {
                    Some(last_time) => last_time.elapsed() > Duration::from_millis(100),
                    None => true,
                };

                if should_seek {
                    // Perform the actual seek
                    if let Some(video) = state.player.video_opt.as_mut() {
                        let duration =
                            Duration::try_from_secs_f64(seek_position).unwrap_or_default();
                        if let Err(e) = video.seek(duration, false) {
                            log::error!("Seek failed: {:?}", e);
                        } else {
                            state.player.last_seek_time = Some(Instant::now());
                            // Clear pending seek since we just performed it
                            state.player.pending_seek_position = None;
                        }
                    }
                } else {
                    // Store pending seek position to be executed later
                    state.player.pending_seek_position = Some(seek_position);
                }
            }

            Task::none()
        }

        // Non-player media messages
        media::Message::PlayMedia(media_file) => {
            // Clear MediaId since this is the old-style PlayMedia without ID
            state.player.current_media_id = None;
            super::play_media::handle_play_media(state, media_file)
        }

        media::Message::PlayMediaWithId(media_file, media_id) => {
            // Store the MediaId for watch status tracking
            state.player.current_media_id = Some(media_id);
            super::play_media::handle_play_media(state, media_file)
        }

        media::Message::VideoLoaded(success) => {
            // VideoLoaded is not a player message, handle it here
            if success {
                log::info!("Video loaded successfully");
                state.view = crate::state::ViewState::Player;

                // Start playback automatically
                Task::done(media::Message::Play)
            } else {
                log::error!("Video failed to load");
                state.view = crate::state::ViewState::VideoError {
                    message: "Failed to load video".to_string(),
                };
                Task::none()
            }
        }

        media::Message::VideoCreated(result) => {
            match result {
                Ok(video_arc) => {
                    log::info!("Video object created successfully");

                    // Try to extract the video from the Arc
                    match std::sync::Arc::try_unwrap(video_arc) {
                        Ok(video) => {
                            state.player.video_opt = Some(video);
                            // Notify that video is loaded
                            Task::done(media::Message::VideoLoaded(true))
                        }
                        Err(_) => {
                            log::error!("Failed to unwrap Arc<Video> - multiple references exist");
                            Task::done(media::Message::VideoLoaded(false))
                        }
                    }
                }
                Err(error) => {
                    log::error!("Failed to create video: {}", error);
                    state.error_message = Some(error.clone());
                    state.view = crate::state::ViewState::VideoError { message: error };
                    Task::none()
                }
            }
        }

        media::Message::MediaAvailabilityChecked(media_file) => {
            log::info!("Media availability confirmed for: {}", media_file.filename);

            // Proceed with playing the media
            Task::done(media::Message::PlayMedia(media_file))
        }

        media::Message::MediaUnavailable(reason, message) => {
            super::play_media::handle_media_unavailable(state, reason, message)
        }

        // Quality control messages that aren't player messages
        media::Message::ToggleQualityMenu => {
            state.player.show_quality_menu = !state.player.show_quality_menu;

            // Hide other menus
            if state.player.show_quality_menu {
                state.player.show_subtitle_menu = false;
                state.player.show_settings = false;
            }

            Task::none()
        }

        media::Message::QualityVariantSelected(profile_name) => {
            log::info!("Quality variant selected: {}", profile_name);

            // TODO: Implement quality switching for HLS streams
            // TODO: Store selected quality and apply it
            state.player.show_quality_menu = false;

            Task::none()
        }

        // Internal message for loading video
        media::Message::_LoadVideo => {
            // Import the load_video function
            use crate::player::video::load_video;
            load_video(state)
        }

        // Transcoding started message
        media::Message::TranscodingStarted => {
            log::info!("Transcoding started");
            // No action needed, just acknowledge
            Task::none()
        }
    }
}
