use super::messages::Message;
use super::state::PlayerDomainState;
use crate::common::messages::{DomainMessage, DomainUpdateResult};
use crate::domains::ui;
use iced::Task;
use std::time::Duration;

/// Handle player domain messages
/// Returns a DomainUpdateResult containing both the task and any events to emit
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn update_player(
    app_state: &mut crate::state_refactored::State,
    message: Message,
) -> DomainUpdateResult {
    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    profiling::scope!(crate::infrastructure::profiling_scopes::scopes::PLAYER_UPDATE);

    // Convenience alias
    let state: &mut PlayerDomainState = &mut app_state.domains.player.state;
    let window_size = app_state.window_size;

    match message {
        Message::PlayMedia(media) => {
            // Fallback handler without MediaID - proceed without tracking
            log::info!("[Player] PlayMedia without ID - starting playback");
            // Delegate to PlayMediaWithId with no ID tracking
            update_player(
                app_state,
                Message::PlayMediaWithId(
                    media,
                    ferrex_core::MediaID::Movie(ferrex_core::MovieID::new_uuid()),
                ),
            )
        }

        Message::NavigateBack => {
            // Reset player state and navigate back
            state.reset();
            // Send direct UI domain message for navigation
            DomainUpdateResult::task(Task::done(DomainMessage::Ui(
                ui::messages::Message::NavigateBack,
            )))
        }

        Message::NavigateHome => {
            // Reset player state and navigate home
            state.reset();
            // Send direct UI domain message for navigation
            DomainUpdateResult::task(Task::done(DomainMessage::Ui(
                ui::messages::Message::NavigateHome,
            )))
        }

        Message::Play => {
            if let Some(video) = &mut state.video_opt {
                video.set_paused(false);
                DomainUpdateResult::task(Task::done(crate::common::messages::DomainMessage::Media(
                    crate::domains::media::messages::Message::SendProgressUpdateWithData(
                        state.current_media_id.unwrap(),
                        video.position().as_secs_f64(),
                        video.duration().as_secs_f64(),
                    ),
                )))
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }

        Message::Pause => {
            if let Some(video) = &mut state.video_opt {
                video.set_paused(true);
                DomainUpdateResult::task(Task::done(crate::common::messages::DomainMessage::Media(
                    crate::domains::media::messages::Message::SendProgressUpdateWithData(
                        state.current_media_id.unwrap(),
                        video.position().as_secs_f64(),
                        video.duration().as_secs_f64(),
                    ),
                )))
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }

        Message::PlayPause => {
            let task = if let Some(video) = &mut state.video_opt {
                let is_paused = video.paused();
                video.set_paused(!is_paused);
                DomainUpdateResult::task(Task::done(crate::common::messages::DomainMessage::Media(
                    crate::domains::media::messages::Message::SendProgressUpdateWithData(
                        state.current_media_id.unwrap(),
                        video.position().as_secs_f64(),
                        video.duration().as_secs_f64(),
                    ),
                )))
            } else {
                DomainUpdateResult::task(Task::none())
            };
            state.update_controls(true);
            task
        }

        Message::ResetAfterStop => {
            // Reset the player state after progress update has been sent
            state.reset();
            DomainUpdateResult::task(Task::none())
        }

        Message::Stop => {
            // Capture position and duration BEFORE reset
            let update_task = if let Some(media_id) = state.current_media_id {
                let position = state.last_valid_position;
                let duration = state.last_valid_duration;

                // Send final progress update with captured data
                Task::done(crate::common::messages::DomainMessage::Media(
                    crate::domains::media::messages::Message::SendProgressUpdateWithData(
                        media_id, position, duration,
                    ),
                ))
            } else {
                Task::none()
            };

            // Store tasks before reset
            let tasks = Task::batch(vec![
                update_task,
                Task::done(DomainMessage::Player(Message::ResetAfterStop)),
                Task::done(DomainMessage::Ui(ui::messages::Message::NavigateBack)),
            ]);

            // Return tasks without resetting yet
            DomainUpdateResult::task(tasks)
        }

        Message::Seek(position) => {
            // Just update UI position during drag, don't seek yet
            if let Some(_video) = &state.video_opt {
                state.dragging = true;
                state.last_valid_position = position;
                state.last_seek_position = Some(position);
                state.update_controls(true);
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::SeekRelease => {
            // Perform the seek on release
            if let (Some(video), Some(media_id)) = (&mut state.video_opt, state.current_media_id) {
                state.dragging = false;

                // Use pending seek position if available, otherwise use last seek position
                let final_seek_position = state.pending_seek_position.or(state.last_seek_position);

                if let Some(seek_position) = final_seek_position {
                    log::debug!("Starting seek to position: {:.2}s", seek_position);
                    state.seeking = true;
                    state.seek_started_time = Some(std::time::Instant::now());
                    let duration = Duration::try_from_secs_f64(seek_position).unwrap_or_default();
                    video.seek(duration, false);
                } else if let Some(seek_position) = state.last_seek_position {
                    // Update position immediately for better UX
                    state.last_valid_position = seek_position;
                    log::debug!("Seek initiated, position set to: {:.2}s", seek_position);
                }

                state.last_seek_position = None;
                state.pending_seek_position = None;
                state.last_seek_time = None;
                state.update_controls(true);

                // Send progress update after seek completes
                return DomainUpdateResult::task(Task::done(
                    crate::common::messages::DomainMessage::Media(
                        crate::domains::media::messages::Message::SendProgressUpdateWithData(
                            media_id,
                            state.last_valid_position,
                            state.last_valid_duration,
                        ),
                    ),
                ));
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::SeekBarPressed => {
            // Only start seeking if we have a valid seek position
            // (which means the mouse was within the seek bar's vertical hit zone)
            if let Some(_video) = &state.video_opt {
                // Check if we have a valid seek position from MouseMoved
                if let Some(seek_position) = state.last_seek_position {
                    // Start dragging
                    state.dragging = true;
                    // Update visual position
                    state.last_valid_position = seek_position;
                    state.update_controls(true);
                    log::debug!(
                        "Seek bar pressed - starting drag at position: {:.2}s",
                        seek_position
                    );
                } else {
                    // Mouse was outside the seek bar's vertical hit zone
                    log::debug!(
                        "Seek bar pressed but mouse is outside valid vertical zone - ignoring"
                    );
                }
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::SeekDone => {
            // Seek operation completed, clear seeking flag
            if let (Some(video), Some(media_id)) = (&mut state.video_opt, state.current_media_id) {
                let video_pos = video.position().as_secs_f64();
                log::debug!(
                    "SeekDone: Clearing seeking flag. Video position: {:.2}s, UI position: {:.2}s",
                    video_pos,
                    state.last_valid_position
                );
                state.seeking = false;
                state.seek_started_time = None;
                // Send progress update after seek completes
                DomainUpdateResult::task(Task::done(crate::common::messages::DomainMessage::Media(
                    crate::domains::media::messages::Message::SendProgressUpdateWithData(
                        media_id,
                        video_pos,
                        state.last_valid_duration,
                    ),
                )))
            } else {
                log::debug!("SeekDone: Clearing seeking flag (no video)");
                DomainUpdateResult::task(Task::none())
            }
        }

        Message::SeekRelative(secs) => {
            if let Some(video) = &mut state.video_opt {
                // Prefer backend position, then state.position, then last_valid_position
                let backend_pos = video.position().as_secs_f64();
                let base_pos = if backend_pos > 0.0 {
                    backend_pos
                } else {
                    state.last_valid_position
                };

                // Determine reliable duration for clamping
                let raw_duration = state.source_duration.unwrap_or(state.last_valid_duration);
                let clamp_duration = if raw_duration > 0.0 {
                    raw_duration
                } else if state.last_valid_duration > 0.0 {
                    state.last_valid_duration
                } else {
                    f64::INFINITY // avoid clamping to 0
                };

                // Calculate new position with bounds
                let mut new_position = (base_pos + secs).max(0.0);
                if clamp_duration.is_finite() {
                    new_position = new_position.min(clamp_duration);
                }

                // Perform the seek
                state.seeking = true;
                state.seek_started_time = Some(std::time::Instant::now());
                let seek_to = Duration::try_from_secs_f64(new_position).unwrap_or_default();
                video.seek(seek_to, false);

                // Update position immediately for better UX and remember as last valid

                if new_position > 0.0 {
                    state.last_valid_position = new_position;
                }

                state.update_controls(true);
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::SetVolume(volume) => {
            if let Some(video) = &mut state.video_opt {
                // Handle relative volume changes from keyboard
                let new_volume = if volume == 1.1 {
                    (state.volume + 0.05).clamp(0.0, 1.0)
                } else if volume == 0.9 {
                    (state.volume - 0.05).clamp(0.0, 1.0)
                } else {
                    volume.clamp(0.0, 1.0)
                };
                state.volume = new_volume;
                video.set_volume(new_volume);
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::ToggleMute => {
            if let Some(video) = &mut state.video_opt {
                state.is_muted = !state.is_muted;
                video.set_muted(state.is_muted);
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::VideoLoaded(success) => {
            if success {
                // Query available tracks
                state.update_available_tracks();
                app_state.domains.ui.state.view = ui::types::ViewState::Player;
                DomainUpdateResult::task(Task::none())
            } else {
                app_state.domains.ui.state.view = ui::types::ViewState::VideoError {
                    message: "Failed to load video".to_string(),
                };
                DomainUpdateResult::task(Task::none())
            }
        }

        Message::VideoReadyToPlay => {
            log::info!("[Player] Video ready to play - loading video directly in player");

            // Runtime option: allow external player; default false for now
            let use_external = false;
            if use_external {
                // Start external MPV
                let is_fullscreen = state.is_fullscreen;
                let window_size = Some((
                    app_state.window_size.width as u32,
                    app_state.window_size.height as u32,
                ));
                let window_position = app_state.window_position.map(|p| (p.x as i32, p.y as i32));
                let resume_position = state.pending_resume_position;

                // Activate external mode and set view to Player (internal view hidden if desired later)
                state.external_mpv_active = true;
                app_state.domains.ui.state.view = ui::types::ViewState::Player;

                // Kick off external playback and begin polling via PollExternalMpv subscription
                let handle_task = Task::perform(
                    async move { /* no-op; start synchronously below */ },
                    |_| DomainMessage::Player(Message::ExternalPlaybackStarted),
                );

                // Start external playback immediately
                let url = state
                    .current_url
                    .as_ref()
                    .map(|u| u.to_string())
                    .unwrap_or_default();
                match super::external_mpv::start_external_playback(
                    &url,
                    is_fullscreen,
                    window_size,
                    window_position,
                    resume_position,
                ) {
                    Ok(handle) => {
                        state.external_mpv_handle = Some(Box::new(handle));
                        // Begin polling every second (subscription is wired in player subscriptions; here we just return the started task)
                        DomainUpdateResult::with_events(handle_task, vec![])
                    }
                    Err(e) => {
                        log::error!("Failed to start external MPV: {}", e);
                        app_state.domains.ui.state.view = ui::types::ViewState::VideoError {
                            message: format!("Failed to start external player: {}", e),
                        };
                        DomainUpdateResult::task(Task::none())
                    }
                }
            } else {
                DomainUpdateResult::task(
                    crate::domains::player::video::load_video(app_state).map(DomainMessage::Player),
                )
            }
        }

        Message::EndOfStream => {
            log::info!("End of stream");

            // Capture position and duration for progress update
            if let Some(media_id) = state.current_media_id {
                let position = state.last_valid_position;
                let duration = state.last_valid_duration;

                // Send final progress update with captured data
                let progress_task = Task::done(crate::common::messages::DomainMessage::Media(
                    crate::domains::media::messages::Message::SendProgressUpdateWithData(
                        media_id, position, duration,
                    ),
                ));

                // Check if we should play next episode
                let next_episode_task = if let Some(media_id) = &state.current_media_id {
                    // Only auto-play next episode if this is an episode
                    if let ferrex_core::MediaID::Episode(_) = media_id {
                        log::info!("Current media is an episode, checking for next episode");
                        Task::done(crate::common::messages::DomainMessage::Media(
                            crate::domains::media::messages::Message::PlayNextEpisode,
                        ))
                    } else {
                        Task::none()
                    }
                // Prefer backend position if video is still present
                let (position, duration) = if let Some(video) = &mut state.video_opt {
                    (
                        video.position().as_secs_f64(),
                        video.duration().as_secs_f64(),
                    )
                } else {
                    Task::none()
                };

                DomainUpdateResult::task(Task::batch(vec![progress_task, next_episode_task]))
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }

        Message::NewFrame => {
            // Also advance transient notifications (e.g., track toast)
            state.update_track_notification();
            let mut update_tks = false;
            if let Some(video) = &mut state.video_opt {
                if state.is_loading_video {
                    state.is_loading_video = false;
                }

                let num_aud_tks = state.available_audio_tracks.len();
                let num_sub_tks = state.available_subtitle_tracks.len();

                if num_aud_tks <= 0 || num_sub_tks <=0 {
                    update_tks = true;
                }


                // Check for seek timeout (500ms)
                if state.seeking {
                    if let Some(start_time) = state.seek_started_time {
                        if start_time.elapsed() > Duration::from_millis(1000) {
                            log::warn!("Seek timeout: clearing seeking flag after 1s");
                            state.seeking = false;
                            state.seek_started_time = None;
                        }
                    }
                }

                // Update duration if it wasn't available during load
                if state.last_valid_duration <= 0.0 {
                    let new_duration = video.duration().as_secs_f64();
                    if new_duration > 0.0 {
                        log::info!("Duration now available: {} seconds", new_duration);
                        state.last_valid_duration = new_duration;
                        state.last_valid_duration = new_duration;
                    } else {
                        log::debug!("NewFrame: Duration still not available from video");
                    }
                }

                // Update position when not dragging or seeking
                if !state.dragging && !state.seeking {
                    // Normal position update
                    let new_position = video.position().as_secs_f64();
                    let old_position = state.last_valid_position;

                    // Only update if we got a valid position
                    if new_position > 0.0 {
                        state.last_valid_position = new_position;

                        // Log significant position changes
                        if (new_position - old_position).abs() > 0.5 {
                            log::debug!(
                                "NewFrame: Position updated from {:.2}s to {:.2}s (duration: {:.2}s, source_duration: {:?})",
                                old_position,
                                new_position,
                                state.last_valid_duration,
                                state.source_duration
                            );
                        }
                    } else {
                        log::trace!(
                            "NewFrame: No valid position update (current: {:.2}s, new: {:.2}s)",
                            state.last_valid_position,
                            new_position
                        );
                    }
                } else {
                    if state.seeking {
                        let video_pos = video.position().as_secs_f64();
                        log::debug!(
                            "NewFrame during seek: video reports {:.2}s, UI shows {:.2}s",
                            video_pos,
                            state.last_valid_position
                        );
                    }
                    log::trace!(
                        "NewFrame: Skipping position update (dragging: {}, seeking: {})",
                        state.dragging,
                        state.seeking
                    );
                }
            }
            if update_tks {
                state.update_available_tracks();
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::Reload => {
            // This is handled in main.rs as it calls load_video
            DomainUpdateResult::task(Task::none())
        }

        Message::ShowControls => {
            state.update_controls(true);
            DomainUpdateResult::task(Task::none())
        }

        Message::ToggleFullscreen => {
            state.is_fullscreen = !state.is_fullscreen;
            let mode = if state.is_fullscreen {
                iced::window::Mode::Fullscreen
            } else {
                iced::window::Mode::Windowed
            };

            // Emit SetWindowMode event instead of managing window directly
            DomainUpdateResult::with_events(
                Task::none(),
                vec![crate::common::messages::CrossDomainEvent::SetWindowMode(
                    mode,
                )],
            )
        }

        Message::ToggleSettings => {
            state.show_settings = !state.show_settings;
            // Close subtitle menu if open
            if state.show_settings {
                state.show_subtitle_menu = false;
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::MouseMoved(point) => {
            use std::time::{Duration, Instant};

            // Update controls visibility
            state.update_controls(true);

            // Track vertical position for seek bar validation
            state.last_mouse_y = Some(point.y);

            // Check if we're within the seek bar's vertical hit zone
            // The seek bar is positioned at the bottom of the screen
            let seek_bar_vertical_center = window_size.height
                - crate::infrastructure::constants::player_controls::SEEK_BAR_CENTER_FROM_BOTTOM;
            let max_vertical_distance = super::state::SEEK_BAR_VISUAL_HEIGHT
                * super::state::SEEK_BAR_CLICK_TOLERANCE_MULTIPLIER;
            let within_seek_zone =
                (point.y - seek_bar_vertical_center).abs() <= max_vertical_distance;

            // Update seek bar hover state
            state.seek_bar_hovered = within_seek_zone;

            // Only calculate seek position if within vertical bounds OR already dragging
            if within_seek_zone || state.dragging {
                let percentage = (point.x / window_size.width).clamp(0.0, 1.0) as f64;
                let duration = state.source_duration.unwrap_or(state.last_valid_duration);
                let seek_position = percentage * duration;
                // Store for potential click-to-seek
                state.last_seek_position = Some(seek_position);
            } else {
                // Clear seek position when outside zone and not dragging
                state.last_seek_position = None;
            }

            // If we're dragging the seek bar, update position and perform seek
            if state.dragging {
                // When dragging, always calculate position even if outside vertical zone
                let percentage = (point.x / window_size.width).clamp(0.0, 1.0) as f64;
                let duration = state.source_duration.unwrap_or(state.last_valid_duration);
                let seek_position = percentage * duration;

                // Update position immediately for responsive UI
                state.last_valid_position = seek_position;
                state.update_controls(true);

                // Throttle actual seeks to prevent overwhelming the network
                let should_seek = match state.last_seek_time {
                    Some(last_time) => last_time.elapsed() > Duration::from_millis(100),
                    None => true,
                };

                if should_seek {
                    // Perform the actual seek
                    if let Some(video) = state.video_opt.as_mut() {
                        let duration =
                            Duration::try_from_secs_f64(seek_position).unwrap_or_default();
                        video.seek(duration, false);
                        state.last_seek_time = Some(Instant::now());
                        // Clear pending seek since we just performed it
                        state.pending_seek_position = None;
                    }
                } else {
                    // Store pending seek position to be executed later
                    state.pending_seek_position = Some(seek_position);
                }
            }

            DomainUpdateResult::task(Task::none())
        }

        Message::VideoClicked => {
            let now = std::time::Instant::now();
            if let Some(last_click) = state.last_click_time {
                if now.duration_since(last_click).as_millis() < 300 {
                    // Double click detected
                    state.last_click_time = None;
                    update_player(app_state, Message::ToggleFullscreen)
                } else {
                    // Single click
                    state.last_click_time = Some(now);
                    update_player(app_state, Message::PlayPause)
                }
            } else {
                // First click
                state.last_click_time = Some(now);
                update_player(app_state, Message::PlayPause)
            }
        }

        Message::VideoDoubleClicked => update_player(app_state, Message::ToggleFullscreen),

        Message::SetPlaybackSpeed(speed) => {
            if let Some(video) = &mut state.video_opt {
                state.playback_speed = speed;
                let _ = video.set_speed(speed);
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::SetContentFit(fit) => {
            state.content_fit = fit;
            DomainUpdateResult::task(Task::none())
        }

        // Track selection messages
        Message::AudioTrackSelected(index) => {
            if let Err(e) = state.select_audio_track(index) {
                log::error!("{}", e);
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::SubtitleTrackSelected(index) => {
            if let Err(e) = state.select_subtitle_track(index) {
                log::error!("{}", e);
            }
            // Close subtitle menu after selection
            state.show_subtitle_menu = false;
            DomainUpdateResult::task(Task::none())
        }

        Message::ToggleSubtitles => {
            if let Err(e) = state.toggle_subtitles() {
                log::error!("{}", e);
            }
            // Close subtitle menu after toggling
            state.show_subtitle_menu = false;
            DomainUpdateResult::task(Task::none())
        }

        Message::ToggleSubtitleMenu => {
            state.show_subtitle_menu = !state.show_subtitle_menu;
            // Close settings if open
            if state.show_subtitle_menu {
                state.show_settings = false;
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::ToggleQualityMenu => {
            state.show_quality_menu = !state.show_quality_menu;
            // Close other menus if open
            if state.show_quality_menu {
                state.show_settings = false;
                state.show_subtitle_menu = false;
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::ToggleAppsinkBackend => {
            if let Some(video) = state.video_opt.as_mut() {
                if std::env::var("WAYLAND_DISPLAY").is_ok() {
                    let current = video.backend();
                    let target = match current {
                        subwave_unified::video::BackendPreference::ForceAppsink => {
                            subwave_unified::video::BackendPreference::ForceWayland
                        }
                        _ => subwave_unified::video::BackendPreference::ForceAppsink,
                    };
                    if let Err(e) = video.set_preference(target) {
                        log::error!("Failed to switch backend: {}", e);
                    } else {
                        log::info!("Switched backend to {:?}", target);
                    }
                } else {
                    // Not on Wayland; ensure Appsink
                    if let Err(e) = video
                        .set_preference(subwave_unified::video::BackendPreference::ForceAppsink)
                    {
                        log::error!("Failed to switch backend: {}", e);
                    }
                }
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::CycleAudioTrack => {
            if let Err(e) = state.cycle_audio_track() {
                log::error!("{}", e);
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::CycleSubtitleTrack => {
            if let Err(e) = state.cycle_subtitle_track() {
                log::error!("{}", e);
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::CycleSubtitleSimple => {
            if let Err(e) = state.cycle_subtitle_simple() {
                log::error!("{}", e);
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::TracksLoaded => {
            // Tracks have been loaded, update notification
            state.update_track_notification();
            DomainUpdateResult::task(Task::none())
        }

        Message::CheckControlsVisibility => {
            // Periodically clear notifications and hide controls if idle
            state.update_track_notification();
            if state.controls && state.controls_time.elapsed() > Duration::from_secs(3) {
                state.controls = false;
            }
            DomainUpdateResult::task(Task::none())
        }

        // New Phase 2 direct command handlers
        Message::SeekTo(duration) => {
            // Convert Duration to f64 seconds and delegate to existing Seek handler
            let position = duration.as_secs_f64();
            update_player(app_state, Message::Seek(position))
        }

        Message::ToggleShuffle => {
            // Toggle shuffle state
            state.is_shuffle_enabled = !state.is_shuffle_enabled;
            log::info!("Shuffle toggled to: {}", state.is_shuffle_enabled);
            DomainUpdateResult::task(Task::none())
        }

        Message::ToggleRepeat => {
            // Toggle repeat state
            state.is_repeat_enabled = !state.is_repeat_enabled;
            log::info!("Repeat toggled to: {}", state.is_repeat_enabled);
            DomainUpdateResult::task(Task::none())
        }

        Message::PlayMediaWithId(media, media_id) => {
            // Store current media and id
            state.current_media = Some(media.clone());
            state.current_media_id = Some(media_id.clone());

            // Transfer pending resume position from media domain if available
            state.pending_resume_position = app_state.domains.media.state.pending_resume_position;
            app_state.domains.media.state.pending_resume_position = None;

            // Set duration from media metadata if available
            if let Some(metadata) = &media.media_file_metadata {
                if let Some(duration) = metadata.duration {
                    state.last_valid_duration = duration;
                }
            }

            // HDR detection heuristics (copied from previous media handler)
            let is_hdr_content = if let Some(metadata) = &media.media_file_metadata {
                if let Some(bit_depth) = metadata.bit_depth {
                    bit_depth > 8
                } else if let Some(color_transfer) = &metadata.color_transfer {
                    ["smpte2084", "arib-std-b67", "smpte2086"]
                        .iter()
                        .any(|t| color_transfer.contains(t))
                } else if let Some(color_primaries) = &metadata.color_primaries {
                    color_primaries.contains("bt2020")
                } else {
                    false
                }
            } else {
                let f = media.filename.as_str();
                f.contains("2160p") || f.contains("UHD") || f.contains("HDR") || f.contains("DV")
            };
            state.is_hdr_content = is_hdr_content;

            // Build streaming URL from server_url and encoded media id
            let media_id_string = media.id.to_string();
            let encoded_media_id = urlencoding::encode(&media_id_string);
            let video_url = format!(
                "{}/api/v1/stream/{}",
                app_state.server_url, encoded_media_id
            );

            match url::Url::parse(&video_url) {
                Ok(url) => {
                    state.current_url = Some(url);
                    app_state.domains.ui.state.view =
                        ui::types::ViewState::LoadingVideo { url: video_url };
                    app_state.domains.ui.state.error_message = None;
                    // Immediately trigger internal player load (non-HLS path)
                    DomainUpdateResult::task(
                        crate::domains::player::video::load_video(app_state)
                            .map(DomainMessage::Player),
                    )
                }
                Err(e) => {
                    app_state.domains.ui.state.error_message = Some(format!("Invalid URL: {}", e));
                    app_state.domains.ui.state.view = ui::types::ViewState::VideoError {
                        message: format!("Invalid URL: {}", e),
                    };
                    DomainUpdateResult::task(Task::none())
                }
            }
        }

        // External MPV player messages
        Message::ExternalPlaybackStarted => {
            log::info!("External MPV playback started");
            DomainUpdateResult::task(Task::none())
        }

        Message::ExternalPlaybackUpdate { position, duration } => {
            // Update state with position from external MPV
            state.last_valid_position = position;
            state.last_valid_duration = duration;

            // Save watch progress
            if position > 0.0 && duration > 0.0 {
                state.last_progress_sent = position;
            }

            DomainUpdateResult::task(Task::none())
        }

        Message::ExternalPlaybackEnded => {
            log::info!("External MPV playback ended");

            // Save final position
            if let (Some(handle), Some(media_id)) =
                (&state.external_mpv_handle, state.current_media_id)
            {
                let final_position = handle.get_final_position();
                let final_fullscreen = handle.get_final_fullscreen();
                state.last_valid_position = final_position;

                // Send final progress update
                let progress_task = Task::done(crate::common::messages::DomainMessage::Media(
                    crate::domains::media::messages::Message::SendProgressUpdateWithData(
                        media_id,
                        final_position,
                        state.last_valid_duration,
                    ),
                ));

                // Clear external MPV state
                state.external_mpv_handle = None;
                state.external_mpv_active = false;

                // Emit RestoreWindow event
                DomainUpdateResult::with_events(
                    progress_task,
                    vec![crate::common::messages::CrossDomainEvent::RestoreWindow(
                        final_fullscreen,
                    )],
                )
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }

        Message::PollExternalMpv => {
            use iced::window;

            match (state.external_mpv_handle.take(), state.current_media_id) {
                (Some(mut handle), Some(media_id)) => {
                    // Check if MPV is still alive
                    if !handle.is_alive() {
                        log::info!("External MPV process has ended");

                        // Get final state before dropping the handle
                        let (position, duration) = handle.poll_position();
                        let final_fullscreen = handle.get_final_fullscreen();

                        log::info!("position: {:?}, duration: {:?}", position, duration);

                        state.last_valid_position = position;
                        state.is_fullscreen = final_fullscreen;

                        // Clear external MPV state
                        state.external_mpv_active = false;

                        // Send final progress update
                        let end_playback_task =
                            Task::done(crate::common::messages::DomainMessage::Player(
                                Message::ExternalPlaybackEnded,
                            ));
                        let progress_task = Task::done(crate::common::messages::DomainMessage::Media(
                        crate::domains::media::messages::Message::SendProgressUpdateWithData(
                            media_id, position, duration,
                        ),
                    ));

                        // Navigate back to previous view
                        let nav_task =
                            Task::done(DomainMessage::Ui(ui::messages::Message::NavigateBack));

                        // Emit RestoreWindow event and return tasks
                        DomainUpdateResult::with_events(
                            end_playback_task.chain(progress_task).chain(nav_task),
                            vec![crate::common::messages::CrossDomainEvent::RestoreWindow(
                                final_fullscreen,
                            )],
                        )
                    } else {
                        // Poll for position updates
                        let (position, duration) = handle.poll_position();

                        // Put the handle back
                        state.external_mpv_handle = Some(handle);

                        // Update state if we got valid data
                        if position >= 0.0 && duration > 0.0 {
                            update_player(
                                app_state,
                                Message::ExternalPlaybackUpdate { position, duration },
                            )
                        } else {
                            DomainUpdateResult::task(Task::none())
                        }
                    }
                }
                _ => DomainUpdateResult::task(Task::none()),
            }
        }

        Message::PlayExternal => {
            // Switch to external player at runtime
            let is_fullscreen = state.is_fullscreen;
            let window_size = Some((
                app_state.window_size.width as u32,
                app_state.window_size.height as u32,
            ));
            let window_position = app_state.window_position.map(|p| (p.x as i32, p.y as i32));
            // Ensure handoff starts at the current native player position
            let resume_position = if let Some(video) = state.video_opt.as_ref() {
                let pos = video.position().as_secs_f64();
                if pos > 0.0 {
                    Some(pos as f32)
                } else {
                    state.pending_resume_position
                }
            } else if state.last_valid_position > 0.0 {
                Some(state.last_valid_position as f32)
            } else {
                state.pending_resume_position
            };

            let url = state
                .current_url
                .as_ref()
                .map(|u| u.to_string())
                .unwrap_or_default();

            // Stop native playback if running
            state.stop_native_playback();

            match super::external_mpv::start_external_playback(
                &url,
                is_fullscreen,
                window_size,
                window_position,
                resume_position,
            ) {
                Ok(handle) => {
                    state.external_mpv_active = true;
                    state.external_mpv_handle = Some(Box::new(handle));
                    app_state.domains.ui.state.view = ui::types::ViewState::Player;
                    DomainUpdateResult::task(Task::done(DomainMessage::Player(
                        Message::ExternalPlaybackStarted,
                    )))
                }
                Err(e) => {
                    log::error!("Failed to start external MPV: {}", e);
                    app_state.domains.ui.state.view = ui::types::ViewState::VideoError {
                        message: format!("Failed to start external player: {}", e),
                    };
                    DomainUpdateResult::task(Task::none())
                }
            }
        }
    }
}
