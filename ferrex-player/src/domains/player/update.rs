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
    state: &mut PlayerDomainState,
    message: Message,
    window_size: iced::Size,
) -> DomainUpdateResult {
    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    profiling::scope!(crate::infrastructure::profiling_scopes::scopes::PLAYER_UPDATE);

    match message {
        Message::PlayMedia(_media) => {
            // This is handled in main.rs as it needs access to server_url
            DomainUpdateResult::task(Task::none())
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
                        video.duration().unwrap_or(Duration::ZERO).as_secs_f64(),
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
                        video.duration().unwrap_or(Duration::ZERO).as_secs_f64(),
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
                        video.duration().unwrap_or(Duration::ZERO).as_secs_f64(),
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
                let position = state.position;
                let duration = state.duration;

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
                state.position = position;
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
                    state.position = seek_position;
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
                            state.position,
                            state.duration,
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
                    state.position = seek_position;
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
                    state.position
                );
                state.seeking = false;
                state.seek_started_time = None;
                // Send progress update after seek completes
                DomainUpdateResult::task(Task::done(crate::common::messages::DomainMessage::Media(
                    crate::domains::media::messages::Message::SendProgressUpdateWithData(
                        media_id,
                        video_pos,
                        state.duration,
                    ),
                )))
            } else {
                log::debug!("SeekDone: Clearing seeking flag (no video)");
                DomainUpdateResult::task(Task::none())
            }
        }

        Message::SeekRelative(secs) => {
            // Relative seek implementation
            if let Some(video) = &mut state.video_opt {
                // Get current position first
                state.position = video.position().as_secs_f64();

                // Calculate new position with bounds
                // Use source duration if available (for HLS this is the full media duration)
                let duration = state.source_duration.unwrap_or(state.duration);
                let mut new_position = (state.position + secs).max(0.0);
                if duration > 0.0 {
                    new_position = new_position.min(duration);
                }

                // Perform the seek
                state.seeking = true;
                state.seek_started_time = Some(std::time::Instant::now());
                let duration = Duration::try_from_secs_f64(new_position).unwrap_or_default();
                video.seek(duration, false);
                // Update position immediately for better UX
                state.position = new_position;

                state.update_controls(true);
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::SeekForward => update_player(state, Message::SeekRelative(15.0), window_size),

        Message::SeekBackward => update_player(state, Message::SeekRelative(-15.0), window_size),

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
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::VideoReadyToPlay => {
            // For now, forward to Media domain which incorrectly handles player functionality
            // TODO: This should be handled directly in Player domain after migration
            log::info!("[Player] Video ready to play - forwarding to Media domain (temporary)");
            DomainUpdateResult::task(Task::done(DomainMessage::Media(
                crate::domains::media::messages::Message::_LoadVideo,
            )))
        }

        Message::EndOfStream => {
            log::info!("End of stream");

            // Capture position and duration for progress update
            if let Some(media_id) = state.current_media_id {
                let position = state.position;
                let duration = state.duration;

                // Send final progress update with captured data
                let progress_task = Task::done(crate::common::messages::DomainMessage::Media(
                    crate::domains::media::messages::Message::SendProgressUpdateWithData(
                        media_id, position, duration,
                    ),
                ));

                // Check if we should play next episode
                let next_episode_task = if let Some(media_id) = &state.current_media_id {
                    // Only auto-play next episode if this is an episode
                    if let ferrex_core::api_types::MediaId::Episode(_) = media_id {
                        log::info!("Current media is an episode, checking for next episode");
                        Task::done(crate::common::messages::DomainMessage::Media(
                            crate::domains::media::messages::Message::PlayNextEpisode,
                        ))
                    } else {
                        Task::none()
                    }
                } else {
                    Task::none()
                };

                DomainUpdateResult::task(Task::batch(vec![progress_task, next_episode_task]))
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }

        Message::NewFrame => {
            if let Some(video) = &mut state.video_opt {
                // Check for seek timeout (500ms)
                if state.seeking {
                    if let Some(start_time) = state.seek_started_time {
                        if start_time.elapsed() > Duration::from_millis(500) {
                            log::warn!("Seek timeout: clearing seeking flag after 500ms");
                            state.seeking = false;
                            state.seek_started_time = None;
                        }
                    }
                }

                // Update duration if it wasn't available during load
                if state.duration <= 0.0 {
                    if let Some(new_duration) = video.duration() {
                        let new_duration = new_duration.as_secs_f64();
                        if new_duration > 0.0 {
                            log::info!("Duration now available: {} seconds", new_duration);
                            state.duration = new_duration;
                        }
                    } else {
                        log::debug!("NewFrame: Duration still not available from video");
                    }
                }

                // Update position when not dragging or seeking
                if !state.dragging && !state.seeking {
                    // Normal position update
                    let new_position = video.position().as_secs_f64();
                    let old_position = state.position;

                    // Only update if we got a valid position
                    if new_position > 0.0 || state.position == 0.0 {
                        state.position = new_position;

                        // Log significant position changes
                        if (new_position - old_position).abs() > 0.5 {
                            log::debug!("NewFrame: Position updated from {:.2}s to {:.2}s (duration: {:.2}s, source_duration: {:?})",
                                    old_position, new_position, state.duration, state.source_duration);
                        }
                    } else {
                        log::trace!(
                            "NewFrame: No valid position update (current: {:.2}s, new: {:.2}s)",
                            state.position,
                            new_position
                        );
                    }
                } else {
                    if state.seeking {
                        let video_pos = video.position().as_secs_f64();
                        log::debug!(
                            "NewFrame during seek: video reports {:.2}s, UI shows {:.2}s",
                            video_pos,
                            state.position
                        );
                    }
                    log::trace!(
                        "NewFrame: Skipping position update (dragging: {}, seeking: {})",
                        state.dragging,
                        state.seeking
                    );
                }
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
                let duration = state.source_duration.unwrap_or(state.duration);
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
                let duration = state.source_duration.unwrap_or(state.duration);
                let seek_position = percentage * duration;

                // Update position immediately for responsive UI
                state.position = seek_position;
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
                    update_player(state, Message::ToggleFullscreen, window_size)
                } else {
                    // Single click
                    state.last_click_time = Some(now);
                    update_player(state, Message::PlayPause, window_size)
                }
            } else {
                // First click
                state.last_click_time = Some(now);
                update_player(state, Message::PlayPause, window_size)
            }
        }

        Message::VideoDoubleClicked => update_player(state, Message::ToggleFullscreen, window_size),

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

        // Tone mapping controls
        Message::ToggleToneMapping(enabled) => {
            state.tone_mapping_config.enabled = enabled;
            if let Some(video) = &mut state.video_opt {
                video.set_tone_mapping_config(state.tone_mapping_config.clone());
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::SetToneMappingPreset(preset) => {
            state.tone_mapping_config.apply_preset(preset);
            if let Some(video) = &mut state.video_opt {
                video.set_tone_mapping_config(state.tone_mapping_config.clone());
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::SetToneMappingAlgorithm(algorithm) => {
            state.tone_mapping_config.algorithm = algorithm;
            state.tone_mapping_config.preset =
                crate::domains::player::video_backend::ToneMappingPreset::Custom;
            // Update algorithm params based on the selected algorithm
            use crate::domains::player::video_backend::{AlgorithmParams, ToneMappingAlgorithm};
            state.tone_mapping_config.algorithm_params = match algorithm {
                ToneMappingAlgorithm::ReinhardExtended => AlgorithmParams::ReinhardExtended {
                    white_point: 4.0,
                    exposure: 1.5,
                    saturation_boost: 1.2,
                },
                ToneMappingAlgorithm::Hable => AlgorithmParams::Hable {
                    shoulder_strength: 0.15,
                    linear_strength: 0.5,
                    linear_angle: 0.01,
                    toe_strength: 0.2,
                },
                _ => AlgorithmParams::None,
            };
            if let Some(video) = &mut state.video_opt {
                video.set_tone_mapping_config(state.tone_mapping_config.clone());
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::SetToneMappingWhitePoint(value) => {
            if let iced_video_player::AlgorithmParams::ReinhardExtended {
                ref mut white_point,
                ..
            } = &mut state.tone_mapping_config.algorithm_params
            {
                *white_point = value;
            }
            state.tone_mapping_config.preset =
                crate::domains::player::video_backend::ToneMappingPreset::Custom;
            if let Some(video) = &mut state.video_opt {
                video.set_tone_mapping_config(state.tone_mapping_config.clone());
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::SetToneMappingExposure(value) => {
            if let iced_video_player::AlgorithmParams::ReinhardExtended {
                ref mut exposure, ..
            } = &mut state.tone_mapping_config.algorithm_params
            {
                *exposure = value;
            }
            state.tone_mapping_config.exposure_adjustment = value;
            state.tone_mapping_config.preset =
                crate::domains::player::video_backend::ToneMappingPreset::Custom;
            if let Some(video) = &mut state.video_opt {
                video.set_tone_mapping_config(state.tone_mapping_config.clone());
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::SetToneMappingSaturation(value) => {
            state.tone_mapping_config.saturation_adjustment = value;
            state.tone_mapping_config.preset =
                crate::domains::player::video_backend::ToneMappingPreset::Custom;
            if let Some(video) = &mut state.video_opt {
                video.set_tone_mapping_config(state.tone_mapping_config.clone());
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::SetToneMappingSaturationBoost(value) => {
            if let iced_video_player::AlgorithmParams::ReinhardExtended {
                ref mut saturation_boost,
                ..
            } = &mut state.tone_mapping_config.algorithm_params
            {
                *saturation_boost = value;
            }
            state.tone_mapping_config.preset =
                crate::domains::player::video_backend::ToneMappingPreset::Custom;
            if let Some(video) = &mut state.video_opt {
                video.set_tone_mapping_config(state.tone_mapping_config.clone());
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::SetHableShoulderStrength(value) => {
            if let iced_video_player::AlgorithmParams::Hable {
                ref mut shoulder_strength,
                ..
            } = &mut state.tone_mapping_config.algorithm_params
            {
                *shoulder_strength = value;
            }
            state.tone_mapping_config.preset =
                crate::domains::player::video_backend::ToneMappingPreset::Custom;
            if let Some(video) = &mut state.video_opt {
                video.set_tone_mapping_config(state.tone_mapping_config.clone());
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::SetHableLinearStrength(value) => {
            if let iced_video_player::AlgorithmParams::Hable {
                ref mut linear_strength,
                ..
            } = &mut state.tone_mapping_config.algorithm_params
            {
                *linear_strength = value;
            }
            state.tone_mapping_config.preset =
                crate::domains::player::video_backend::ToneMappingPreset::Custom;
            if let Some(video) = &mut state.video_opt {
                video.set_tone_mapping_config(state.tone_mapping_config.clone());
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::SetHableLinearAngle(value) => {
            if let iced_video_player::AlgorithmParams::Hable {
                ref mut linear_angle,
                ..
            } = &mut state.tone_mapping_config.algorithm_params
            {
                *linear_angle = value;
            }
            state.tone_mapping_config.preset =
                crate::domains::player::video_backend::ToneMappingPreset::Custom;
            if let Some(video) = &mut state.video_opt {
                video.set_tone_mapping_config(state.tone_mapping_config.clone());
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::SetHableToeStrength(value) => {
            if let iced_video_player::AlgorithmParams::Hable {
                ref mut toe_strength,
                ..
            } = &mut state.tone_mapping_config.algorithm_params
            {
                *toe_strength = value;
            }
            state.tone_mapping_config.preset =
                crate::domains::player::video_backend::ToneMappingPreset::Custom;
            if let Some(video) = &mut state.video_opt {
                video.set_tone_mapping_config(state.tone_mapping_config.clone());
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::SetMonitorBrightness(value) => {
            state.tone_mapping_config.monitor_brightness = value;
            if let Some(video) = &mut state.video_opt {
                video.set_tone_mapping_config(state.tone_mapping_config.clone());
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::SetToneMappingBrightness(value) => {
            state.tone_mapping_config.brightness_adjustment = value;
            state.tone_mapping_config.preset =
                crate::domains::player::video_backend::ToneMappingPreset::Custom;
            if let Some(video) = &mut state.video_opt {
                video.set_tone_mapping_config(state.tone_mapping_config.clone());
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::SetToneMappingContrast(value) => {
            state.tone_mapping_config.contrast_adjustment = value;
            state.tone_mapping_config.preset =
                crate::domains::player::video_backend::ToneMappingPreset::Custom;
            if let Some(video) = &mut state.video_opt {
                video.set_tone_mapping_config(state.tone_mapping_config.clone());
            }
            DomainUpdateResult::task(Task::none())
        }

        // New Phase 2 direct command handlers
        Message::SeekTo(duration) => {
            // Convert Duration to f64 seconds and delegate to existing Seek handler
            let position = duration.as_secs_f64();
            update_player(state, Message::Seek(position), window_size)
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

        Message::LoadTrack(media_id) => {
            // Load a specific track by MediaId
            log::info!("Loading track with ID: {:?}", media_id);
            // This will be connected to the media store in Task 2.7
            // For now, just acknowledge the command
            DomainUpdateResult::task(Task::done(DomainMessage::Media(
                crate::domains::media::messages::Message::LoadMediaById(media_id),
            )))
        }

        // External MPV player messages
        #[cfg(feature = "external-mpv-player")]
        Message::ExternalPlaybackStarted => {
            log::info!("External MPV playback started");
            DomainUpdateResult::task(Task::none())
        }

        #[cfg(feature = "external-mpv-player")]
        Message::ExternalPlaybackUpdate { position, duration } => {
            // Update state with position from external MPV
            state.position = position;
            state.duration = duration;

            // Save watch progress
            if position > 0.0 && duration > 0.0 {
                state.last_progress_sent = position;
            }

            DomainUpdateResult::task(Task::none())
        }

        #[cfg(feature = "external-mpv-player")]
        Message::ExternalPlaybackEnded => {
            log::info!("External MPV playback ended");

            // Save final position
            if let (Some(handle), Some(media_id)) =
                (&state.external_mpv_handle, state.current_media_id)
            {
                let final_position = handle.get_final_position();
                let final_fullscreen = handle.get_final_fullscreen();
                state.position = final_position;

                // Send final progress update
                let progress_task = Task::done(crate::common::messages::DomainMessage::Media(
                    crate::domains::media::messages::Message::SendProgressUpdateWithData(
                        media_id,
                        final_position,
                        state.duration,
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

        #[cfg(feature = "external-mpv-player")]
        Message::PollExternalMpv => {
            use iced::window;

            if let (Some(mut handle), Some(media_id)) =
                (state.external_mpv_handle.take(), state.current_media_id)
            {
                // Check if MPV is still alive
                if !handle.is_alive() {
                    log::info!("External MPV process has ended");

                    // Get final state before dropping the handle
                    let (position, duration) = handle.poll_position();
                    let final_fullscreen = handle.get_final_fullscreen();

                    log::info!("position: {:?}, duration: {:?}", position, duration);

                    state.position = position;
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
                            state,
                            Message::ExternalPlaybackUpdate { position, duration },
                            window_size,
                        )
                    } else {
                        DomainUpdateResult::task(Task::none())
                    }
                }
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }
    }
}
