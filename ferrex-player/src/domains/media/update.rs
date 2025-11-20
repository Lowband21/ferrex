use std::time::Duration;

use super::messages::Message;
use crate::common::messages::{DomainMessage, DomainUpdateResult};
use crate::infrastructure::services::api::ApiService;
use crate::state_refactored::State;
use ferrex_core::{MediaIDLike, UpdateProgressRequest};
use iced::Task;

/// Handle media domain messages - focused on media management, not playback
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn update_media(state: &mut State, message: Message) -> DomainUpdateResult {
    match message {
        // Media management messages
        Message::PlayMedia(media_file) => {
            super::update_handlers::play_media::handle_play_media(state, media_file)
        }

        Message::PlayMediaWithId(media, media_id) => {
            // Log the incoming media file's metadata
            log::info!(
                "PlayMediaWithId: Received media file '{}' with duration = {:?}",
                media.filename,
                media.media_file_metadata.as_ref().and_then(|m| m.duration)
            );

            // Store the MediaID for watch status tracking
            state.domains.media.state.current_media_id = Some(media_id.clone());
            state.domains.player.state.current_media_id = Some(media_id.clone());

            // Check if we have watch state and get resume position
            let resume_position =
                if let Some(watch_state) = &state.domains.media.state.user_watch_state {
                    watch_state
                        .get_by_media_id(media_id.as_uuid())
                        .map(|item| item.position)
                } else {
                    None
                };

            // Store resume position for use when video loads
            state.domains.media.state.pending_resume_position = resume_position;

            if let Some(pos) = resume_position {
                log::info!("Will resume playback at position: {:.1}s", pos);
            }

            super::update_handlers::play_media::handle_play_media(state, media)
        }

        Message::LoadMediaById(media_id) => {
            log::info!("Loading media by ID: {:?}", media_id);

            let media_result = state.domains.media.state.repo_accessor.get(&media_id);

            if let Ok(media_ref) = media_result {
                let mediafile_opt = match media_ref {
                    ferrex_core::Media::Movie(movie_reference) => Some(movie_reference.file),
                    ferrex_core::Media::Series(series_reference) => None,
                    ferrex_core::Media::Season(season_reference) => None,
                    ferrex_core::Media::Episode(episode_reference) => Some(episode_reference.file),
                };
                if let Some(mediafile) = mediafile_opt {
                    // Log the core file's metadata
                    log::info!(
                        "Core MediaFile metadata: duration = {:?}",
                        mediafile
                            .media_file_metadata
                            .as_ref()
                            .and_then(|m| m.duration)
                    );

                    // Play the media with ID tracking
                    update_media(state, Message::PlayMediaWithId(mediafile, media_id))
                } else {
                    log::error!(
                        "MediaID does not reference a playable media: {:?}",
                        media_id
                    );
                    // Return an error view
                    state.domains.ui.state.view =
                        crate::domains::ui::types::ViewState::VideoError {
                            message: format!("Media not playable: {:?}", media_id),
                        };
                    DomainUpdateResult::task(Task::none())
                }
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

                    // Get duration from the video
                    let duration = video_arc.duration().unwrap_or(Duration::ZERO).as_secs_f64();
                    if duration > 0.0 {
                        log::info!("Video duration: {:.1}s", duration);
                        state.domains.player.state.duration = duration;
                    } else {
                        log::warn!("Video duration not available yet");
                    }

                    // Check for resume position and seek if needed
                    if let Some(resume_pos) = state.domains.player.state.pending_resume_position {
                        log::info!("Resuming playback at position: {:.1}s", resume_pos);
                        // Convert to Duration for seeking
                        let resume_duration = std::time::Duration::from_secs_f32(resume_pos);
                        video_arc.seek(resume_duration, false);
                        // Clear the pending resume position
                        state.domains.player.state.pending_resume_position = None;
                    }

                    // Store the video Arc
                    state.domains.player.state.video_opt = Some(video_arc);

                    // Notify that video is loaded
                    state.domains.ui.state.view = crate::domains::ui::types::ViewState::Player;

                    // Start playing immediately
                    if let Some(video) = &state.domains.player.state.video_opt {
                        video.set_paused(false);
                        if video.position() != Duration::from_secs(0) {
                            state.domains.player.state.is_loading_video = false;
                        }
                    }

                    DomainUpdateResult::task(Task::done(DomainMessage::Media(
                        Message::VideoLoaded(true),
                    )))
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
            let load_video_task = DomainUpdateResult::task(
                crate::domains::player::video::load_video(state).map(DomainMessage::Media),
            );

            #[cfg(feature = "external-mpv-player")]
            let load_video_task = {
                // Check if we should use external MPV for HDR content
                if state.domains.player.state.is_hdr_content
                    && !state.domains.player.state.external_mpv_active
                {
                    log::info!(
                        "HDR content detected - using external MPV player for HDR passthrough"
                    );

                    let window_position_task =
                        iced::window::get_latest().and_then(|id| iced::window::get_position(id));

                    DomainUpdateResult::with_events(
                        Task::chain(
                            window_position_task
                                .map(|position| {
                                    log::info!("window position {:?}", position);
                                    crate::domains::ui::messages::Message::WindowMoved(position)
                                })
                                .map(DomainMessage::Ui),
                            crate::domains::player::video::load_external_video(state)
                                .map(DomainMessage::Media),
                        ),
                        vec![crate::common::messages::CrossDomainEvent::HideWindow],
                    )
                } else {
                    // SDR content or external MPV already active - use internal player
                    if state.domains.player.state.is_hdr_content {
                        log::info!("External MPV already active, skipping duplicate load");
                    } else {
                        log::info!("SDR content detected - using internal iced_video_player");
                    }
                    DomainUpdateResult::task(
                        crate::domains::player::video::load_video(state).map(DomainMessage::Media),
                    )
                }
            };

            load_video_task
        }

        Message::Noop => {
            // No-op message, used for task chaining
            DomainUpdateResult::task(Task::none())
        }

        Message::WatchProgressFetched(_media_id, _resume_position) => {
            // This message is for future use when we fetch watch progress asynchronously
            // Currently handled synchronously in PlayMediaWithId
            DomainUpdateResult::task(Task::none())
        }

        Message::PlayNextEpisode => {
            /* TODO: Reimplement this
            // Check if current media is an episode and play the next one
            if let Some(current_media_id) = &state.domains.media.state.current_media_id {
                if let ferrex_core::MediaID::Episode(episode_id) = current_media_id {
                    // Use series progress service to find next episode
                    //use crate::domains::media::services::SeriesProgressService;

                    let media_store = state.domains.media.state.media_store.clone();
                    let service = SeriesProgressService::new(media_store);

                    if let Some(next_episode) = service.get_next_episode(episode_id) {
                        log::info!(
                            "Auto-playing next episode: S{:02}E{:02}",
                            next_episode.season_number.value(),
                            next_episode.episode_number.value()
                        );

                        // Convert to MediaFile and play
                        let media_file = crate::domains::media::library::MediaFile::from(
                            next_episode.file.clone(),
                        );
                        let media_id = ferrex_core::MediaID::Episode(next_episode.id.clone());

                        // Clear any pending resume position for fresh start of next episode
                        state.domains.media.state.pending_resume_position = None;

                        // Reuse the PlayMediaWithId handler
                        update_media(state, Message::PlayMediaWithId(media_file, media_id))
                    } else {
                        log::info!("No next episode found, playback complete");
                        // Could navigate back to series view or show completion message
                        DomainUpdateResult::task(Task::none())
                    }
                } else {
                    // Not an episode, nothing to auto-play
                    DomainUpdateResult::task(Task::none())
                }
            } else {
                DomainUpdateResult::task(Task::none())
            } */
            DomainUpdateResult::task(Task::none())
        }

        Message::MediaAvailabilityChecked(media_file) => {
            log::info!("Media availability confirmed for: {}", media_file.filename);
            // Proceed with playing the media
            DomainUpdateResult::task(Task::done(DomainMessage::Media(Message::PlayMedia(
                media_file,
            ))))
        }

        Message::MediaUnavailable(reason, message) => {
            super::update_handlers::play_media::handle_media_unavailable(state, reason, message)
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
        Message::ProgressUpdateSent(id, position, duration) => {
            // Update the last sent position
            state.domains.player.state.last_progress_sent = position;
            state.domains.player.state.last_progress_update = Some(std::time::Instant::now());

            // Update local watch state to reflect in UI immediately
            let should_refresh_ui = if let Some(media_id) =
                &state.domains.media.state.current_media_id
            {
                let duration = state.domains.player.state.duration;

                // Only update watch state if we have a valid duration
                if duration > 0.0 {
                    // Update or create watch state if needed
                    if state.domains.media.state.user_watch_state.is_none() {
                        state.domains.media.state.user_watch_state =
                            Some(ferrex_core::watch_status::UserWatchState::new());
                    }

                    if let Some(watch_state) = &mut state.domains.media.state.user_watch_state {
                        // Update progress in local watch state
                        watch_state.update_progress(
                            media_id.to_uuid(),
                            position as f32,
                            duration as f32,
                        );
                        log::info!(
                            "Updated local watch state for {:?}: {:.1}s/{:.1}s",
                            media_id,
                            position,
                            duration
                        );

                        // Debug: Check what's actually in the watch state now
                        if let Some(item) = watch_state.in_progress.get(media_id.as_uuid()) {
                            log::debug!(
                                "Watch state verification - MediaID {:?} has position: {:.1}s, duration: {:.1}s",
                                media_id, item.position, item.duration
                            );
                        } else if watch_state.completed.contains(media_id.as_uuid()) {
                            log::debug!(
                                "Watch state verification - MediaID {:?} is marked as completed",
                                media_id
                            );
                        } else {
                            log::warn!(
                                "Watch state verification - MediaID {:?} not found after update!",
                                media_id
                            );
                        }

                        // Check if enough time has passed since last UI refresh (debounce)
                        // Refresh at most once every 2 seconds to avoid excessive updates
                        let should_refresh = if let Some(last_refresh) =
                            state.domains.media.state.last_ui_refresh_for_progress
                        {
                            last_refresh.elapsed() > std::time::Duration::from_secs(2)
                        } else {
                            true // First update, always refresh
                        };

                        if should_refresh {
                            state.domains.media.state.last_ui_refresh_for_progress =
                                Some(std::time::Instant::now());
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    // Duration is 0.0, don't update watch state
                    log::warn!("Skipping watch state update - duration is 0.0");
                    false
                }
            } else {
                false
            };

            // If watch state was updated and debounce allows, trigger a UI refresh
            if should_refresh_ui {
                log::debug!("Triggering UI refresh for watch progress update");
                // Use UpdateViewModelFilters for a lightweight refresh
                DomainUpdateResult::task(Task::done(DomainMessage::Ui(
                    crate::domains::ui::messages::Message::UpdateViewModelFilters,
                )))
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }

        Message::ProgressUpdateFailed => {
            // Log was already done in subscription, just track the failure
            log::debug!("Progress update failed, will retry on next interval");
            DomainUpdateResult::task(Task::none())
        }

        Message::SendProgressUpdateWithData(media_id, position, duration) => {
            log::debug!("SendProgressUpdateWithData: Starting progress update with captured data");

            let duration = duration;

            // Send an immediate progress update using the captured data
            return if let Some(api_service) = &state.domains.media.state.api_service {
                log::debug!(
                    "SendProgressUpdateWithData: Media {:?}, Position: {:.1}s, Duration: {:.1}s",
                    media_id,
                    position,
                    duration
                );

                if position > 0.0 && duration > 0.0 {
                    let api_service = api_service.clone();

                    DomainUpdateResult::task(Task::perform(
                        async move {
                            let request = UpdateProgressRequest {
                                media_id: media_id.to_uuid(),
                                media_type: media_id.media_type(),
                                position: position as f32,
                                duration: duration as f32,
                            };
                            api_service
                                .update_progress(&request)
                                .await
                                .map(|_| position)
                        },
                        move |result| match result {
                            Ok(pos) => DomainMessage::Media(Message::ProgressUpdateSent(
                                media_id, pos, duration,
                            )),
                            Err(e) => {
                                log::warn!("Failed to send progress update: {}", e);
                                DomainMessage::Media(Message::ProgressUpdateFailed)
                            }
                        },
                    ))
                } else {
                    log::warn!(
                        "Skipping progress update - invalid data: position={:.1}s, duration={:.1}s",
                        position,
                        duration
                    );
                    DomainUpdateResult::task(Task::none())
                }
            } else {
                DomainUpdateResult::task(Task::none())
            };
        }

        // All other messages are now handled by player domain
        _ => {
            log::warn!("Media domain received unhandled message: {:?}", message);
            DomainUpdateResult::task(Task::none())
        }
    }
}
