use super::messages::PlayerMessage;
use super::state::PlayerState;
use crate::messages::media::Message;
use iced::Task;
use std::time::Duration;

impl PlayerState {
    /// Handle player messages and return a task if needed
    pub fn update(&mut self, message: PlayerMessage) -> Task<Message> {
        match message {
            PlayerMessage::PlayMedia(_media) => {
                // This is handled in main.rs as it needs access to server_url
                Task::none()
            }

            PlayerMessage::BackToLibrary => {
                self.reset();
                Task::none()
            }

            PlayerMessage::Play => {
                if let Some(video) = &mut self.video_opt {
                    video.set_paused(false);
                }
                Task::none()
            }

            PlayerMessage::Pause => {
                if let Some(video) = &mut self.video_opt {
                    video.set_paused(true);
                }
                Task::none()
            }

            PlayerMessage::PlayPause => {
                if let Some(video) = &mut self.video_opt {
                    let is_paused = video.paused();
                    video.set_paused(!is_paused);
                    self.update_controls(true);
                }
                Task::none()
            }

            PlayerMessage::Stop => {
                self.reset();
                Task::none()
            }

            PlayerMessage::Seek(position) => {
                // Just update UI position during drag, don't seek yet
                if let Some(_video) = &self.video_opt {
                    self.dragging = true;
                    self.position = position;
                    self.last_seek_position = Some(position);
                    self.update_controls(true);
                }
                Task::none()
            }

            PlayerMessage::SeekRelease => {
                // Perform the seek on release
                if let Some(video) = &mut self.video_opt {
                    self.dragging = false;

                    // Use pending seek position if available, otherwise use last seek position
                    let final_seek_position =
                        self.pending_seek_position.or(self.last_seek_position);

                    if let Some(seek_position) = final_seek_position {
                        log::debug!("Starting seek to position: {:.2}s", seek_position);
                        self.seeking = true;
                        self.seek_started_time = Some(std::time::Instant::now());
                        let duration =
                            Duration::try_from_secs_f64(seek_position).unwrap_or_default();
                        if let Err(e) = video.seek(duration, false) {
                            log::error!("Seek failed: {:?}", e);
                            self.seeking = false;
                            self.seek_started_time = None;
                        } else {
                            // Update position immediately for better UX
                            self.position = seek_position;
                            log::debug!("Seek initiated, position set to: {:.2}s", seek_position);
                        }
                    }

                    self.last_seek_position = None;
                    self.pending_seek_position = None;
                    self.last_seek_time = None;
                    self.update_controls(true);
                }
                Task::none()
            }

            PlayerMessage::SeekBarPressed => {
                // Start seeking - use the last known position if we have one
                if let Some(_video) = &self.video_opt {
                    self.dragging = true;

                    // If we have a last_seek_position (from mouse move), update visual position
                    if let Some(seek_position) = self.last_seek_position {
                        self.position = seek_position;
                    }

                    self.update_controls(true);
                }
                Task::none()
            }

            PlayerMessage::SeekBarMoved(_point) => {
                // SeekBarMoved should not reach here as it's handled in update_media.rs
                // where we have access to window dimensions
                log::warn!("SeekBarMoved reached player update - this should be handled in update_media.rs");
                Task::none()
            }

            PlayerMessage::SeekDone => {
                // Seek operation completed, clear seeking flag
                if let Some(video) = &mut self.video_opt {
                    let video_pos = video.position().as_secs_f64();
                    log::debug!("SeekDone: Clearing seeking flag. Video position: {:.2}s, UI position: {:.2}s",
                        video_pos, self.position);
                } else {
                    log::debug!("SeekDone: Clearing seeking flag (no video)");
                }
                self.seeking = false;
                self.seek_started_time = None;
                Task::none()
            }

            PlayerMessage::SeekRelative(secs) => {
                // Relative seek implementation
                if let Some(video) = &mut self.video_opt {
                    // Get current position first
                    self.position = video.position().as_secs_f64();

                    // Calculate new position with bounds
                    // Use source duration if available (for HLS this is the full media duration)
                    let duration = self.source_duration.unwrap_or(self.duration);
                    let mut new_position = (self.position + secs).max(0.0);
                    if duration > 0.0 {
                        new_position = new_position.min(duration);
                    }

                    // Perform the seek
                    self.seeking = true;
                    self.seek_started_time = Some(std::time::Instant::now());
                    let duration = Duration::try_from_secs_f64(new_position).unwrap_or_default();
                    if let Err(e) = video.seek(duration, false) {
                        log::error!("Relative seek failed: {:?}", e);
                        self.seeking = false;
                        self.seek_started_time = None;
                    } else {
                        // Update position immediately for better UX
                        self.position = new_position;
                    }

                    self.update_controls(true);
                }
                Task::none()
            }

            PlayerMessage::SeekForward => self.update(PlayerMessage::SeekRelative(15.0)),

            PlayerMessage::SeekBackward => self.update(PlayerMessage::SeekRelative(-15.0)),

            PlayerMessage::SetVolume(volume) => {
                if let Some(video) = &mut self.video_opt {
                    // Handle relative volume changes from keyboard
                    let new_volume = if volume == 1.1 {
                        (self.volume + 0.05).clamp(0.0, 1.0)
                    } else if volume == 0.9 {
                        (self.volume - 0.05).clamp(0.0, 1.0)
                    } else {
                        volume.clamp(0.0, 1.0)
                    };
                    self.volume = new_volume;
                    video.set_volume(new_volume);
                }
                Task::none()
            }

            PlayerMessage::ToggleMute => {
                if let Some(video) = &mut self.video_opt {
                    self.is_muted = !self.is_muted;
                    video.set_muted(self.is_muted);
                }
                Task::none()
            }

            PlayerMessage::VideoLoaded(success) => {
                if success {
                    // Query available tracks
                    self.update_available_tracks();
                }
                Task::none()
            }

            PlayerMessage::EndOfStream => {
                log::info!("End of stream");
                Task::none()
            }

            PlayerMessage::NewFrame => {
                if let Some(video) = &mut self.video_opt {
                    // Check for seek timeout (500ms)
                    if self.seeking {
                        if let Some(start_time) = self.seek_started_time {
                            if start_time.elapsed() > Duration::from_millis(500) {
                                log::warn!("Seek timeout: clearing seeking flag after 500ms");
                                self.seeking = false;
                                self.seek_started_time = None;
                            }
                        }
                    }

                    // Update duration if it wasn't available during load
                    if self.duration <= 0.0 {
                        let new_duration = video.duration().as_secs_f64();
                        if new_duration > 0.0 {
                            log::info!("Duration now available: {} seconds", new_duration);
                            self.duration = new_duration;
                        } else {
                            log::debug!("NewFrame: Duration still not available from video");
                        }
                    }

                    // Update position when not dragging or seeking
                    if !self.dragging && !self.seeking {
                        // Normal position update
                        let new_position = video.position().as_secs_f64();
                        let old_position = self.position;

                        // Only update if we got a valid position
                        if new_position > 0.0 || self.position == 0.0 {
                            self.position = new_position;

                            // Log significant position changes
                            if (new_position - old_position).abs() > 0.5 {
                                log::debug!("NewFrame: Position updated from {:.2}s to {:.2}s (duration: {:.2}s, source_duration: {:?})",
                                    old_position, new_position, self.duration, self.source_duration);
                            }
                        } else {
                            log::trace!(
                                "NewFrame: No valid position update (current: {:.2}s, new: {:.2}s)",
                                self.position,
                                new_position
                            );
                        }
                    } else {
                        if self.seeking {
                            let video_pos = video.position().as_secs_f64();
                            log::debug!(
                                "NewFrame during seek: video reports {:.2}s, UI shows {:.2}s",
                                video_pos,
                                self.position
                            );
                        }
                        log::trace!(
                            "NewFrame: Skipping position update (dragging: {}, seeking: {})",
                            self.dragging,
                            self.seeking
                        );
                    }
                }
                Task::none()
            }

            PlayerMessage::Reload => {
                // This is handled in main.rs as it calls load_video
                Task::none()
            }

            PlayerMessage::ShowControls => {
                self.update_controls(true);
                Task::none()
            }

            PlayerMessage::ToggleFullscreen => {
                self.is_fullscreen = !self.is_fullscreen;
                let mode = if self.is_fullscreen {
                    iced::window::Mode::Fullscreen
                } else {
                    iced::window::Mode::Windowed
                };
                // Use toggle_maximize for now as change_mode might not be available in this Iced version
                // TODO: Update to use proper fullscreen mode when Iced is updated
                //iced::Settings::fullscreen;
                iced::window::get_latest().and_then(move |id| iced::window::set_mode(id, mode))
            }

            PlayerMessage::ToggleSettings => {
                self.show_settings = !self.show_settings;
                // Close subtitle menu if open
                if self.show_settings {
                    self.show_subtitle_menu = false;
                }
                Task::none()
            }

            PlayerMessage::MouseMoved => {
                self.update_controls(true);
                Task::none()
            }

            PlayerMessage::VideoClicked => {
                let now = std::time::Instant::now();
                if let Some(last_click) = self.last_click_time {
                    if now.duration_since(last_click).as_millis() < 300 {
                        // Double click detected
                        self.last_click_time = None;
                        self.update(PlayerMessage::ToggleFullscreen)
                    } else {
                        // Single click
                        self.last_click_time = Some(now);
                        self.update(PlayerMessage::PlayPause)
                    }
                } else {
                    // First click
                    self.last_click_time = Some(now);
                    self.update(PlayerMessage::PlayPause)
                }
            }

            PlayerMessage::VideoDoubleClicked => self.update(PlayerMessage::ToggleFullscreen),

            PlayerMessage::SetPlaybackSpeed(speed) => {
                if let Some(video) = &mut self.video_opt {
                    self.playback_speed = speed;
                    let _ = video.set_speed(speed);
                }
                Task::none()
            }

            PlayerMessage::SetAspectRatio(ratio) => {
                self.aspect_ratio = ratio;
                Task::none()
            }

            // Track selection messages
            PlayerMessage::AudioTrackSelected(index) => {
                if let Err(e) = self.select_audio_track(index) {
                    log::error!("{}", e);
                }
                Task::none()
            }

            PlayerMessage::SubtitleTrackSelected(index) => {
                if let Err(e) = self.select_subtitle_track(index) {
                    log::error!("{}", e);
                }
                // Close subtitle menu after selection
                self.show_subtitle_menu = false;
                Task::none()
            }

            PlayerMessage::ToggleSubtitles => {
                if let Err(e) = self.toggle_subtitles() {
                    log::error!("{}", e);
                }
                // Close subtitle menu after toggling
                self.show_subtitle_menu = false;
                Task::none()
            }

            PlayerMessage::ToggleSubtitleMenu => {
                self.show_subtitle_menu = !self.show_subtitle_menu;
                // Close settings if open
                if self.show_subtitle_menu {
                    self.show_settings = false;
                }
                Task::none()
            }

            PlayerMessage::CycleAudioTrack => {
                if let Err(e) = self.cycle_audio_track() {
                    log::error!("{}", e);
                }
                Task::none()
            }

            PlayerMessage::CycleSubtitleTrack => {
                if let Err(e) = self.cycle_subtitle_track() {
                    log::error!("{}", e);
                }
                Task::none()
            }

            PlayerMessage::CycleSubtitleSimple => {
                if let Err(e) = self.cycle_subtitle_simple() {
                    log::error!("{}", e);
                }
                Task::none()
            }

            PlayerMessage::TracksLoaded => {
                // Tracks have been loaded, update notification
                self.update_track_notification();
                Task::none()
            }

            // Tone mapping controls
            PlayerMessage::ToggleToneMapping(enabled) => {
                self.tone_mapping_config.enabled = enabled;
                if let Some(video) = &mut self.video_opt {
                    video.set_tone_mapping_config(self.tone_mapping_config.clone());
                }
                Task::none()
            }

            PlayerMessage::SetToneMappingPreset(preset) => {
                self.tone_mapping_config.apply_preset(preset);
                if let Some(video) = &mut self.video_opt {
                    video.set_tone_mapping_config(self.tone_mapping_config.clone());
                }
                Task::none()
            }

            PlayerMessage::SetToneMappingAlgorithm(algorithm) => {
                self.tone_mapping_config.algorithm = algorithm;
                self.tone_mapping_config.preset = iced_video_player::ToneMappingPreset::Custom;
                // Update algorithm params based on the selected algorithm
                use iced_video_player::{AlgorithmParams, ToneMappingAlgorithm};
                self.tone_mapping_config.algorithm_params = match algorithm {
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
                if let Some(video) = &mut self.video_opt {
                    video.set_tone_mapping_config(self.tone_mapping_config.clone());
                }
                Task::none()
            }

            PlayerMessage::SetToneMappingWhitePoint(value) => {
                if let iced_video_player::AlgorithmParams::ReinhardExtended {
                    ref mut white_point,
                    ..
                } = &mut self.tone_mapping_config.algorithm_params
                {
                    *white_point = value;
                }
                self.tone_mapping_config.preset = iced_video_player::ToneMappingPreset::Custom;
                if let Some(video) = &mut self.video_opt {
                    video.set_tone_mapping_config(self.tone_mapping_config.clone());
                }
                Task::none()
            }

            PlayerMessage::SetToneMappingExposure(value) => {
                if let iced_video_player::AlgorithmParams::ReinhardExtended {
                    ref mut exposure,
                    ..
                } = &mut self.tone_mapping_config.algorithm_params
                {
                    *exposure = value;
                }
                self.tone_mapping_config.exposure_adjustment = value;
                self.tone_mapping_config.preset = iced_video_player::ToneMappingPreset::Custom;
                if let Some(video) = &mut self.video_opt {
                    video.set_tone_mapping_config(self.tone_mapping_config.clone());
                }
                Task::none()
            }

            PlayerMessage::SetToneMappingSaturation(value) => {
                self.tone_mapping_config.saturation_adjustment = value;
                self.tone_mapping_config.preset = iced_video_player::ToneMappingPreset::Custom;
                if let Some(video) = &mut self.video_opt {
                    video.set_tone_mapping_config(self.tone_mapping_config.clone());
                }
                Task::none()
            }

            PlayerMessage::SetToneMappingSaturationBoost(value) => {
                if let iced_video_player::AlgorithmParams::ReinhardExtended {
                    ref mut saturation_boost,
                    ..
                } = &mut self.tone_mapping_config.algorithm_params
                {
                    *saturation_boost = value;
                }
                self.tone_mapping_config.preset = iced_video_player::ToneMappingPreset::Custom;
                if let Some(video) = &mut self.video_opt {
                    video.set_tone_mapping_config(self.tone_mapping_config.clone());
                }
                Task::none()
            }

            PlayerMessage::SetHableShoulderStrength(value) => {
                if let iced_video_player::AlgorithmParams::Hable {
                    ref mut shoulder_strength,
                    ..
                } = &mut self.tone_mapping_config.algorithm_params
                {
                    *shoulder_strength = value;
                }
                self.tone_mapping_config.preset = iced_video_player::ToneMappingPreset::Custom;
                if let Some(video) = &mut self.video_opt {
                    video.set_tone_mapping_config(self.tone_mapping_config.clone());
                }
                Task::none()
            }

            PlayerMessage::SetHableLinearStrength(value) => {
                if let iced_video_player::AlgorithmParams::Hable {
                    ref mut linear_strength,
                    ..
                } = &mut self.tone_mapping_config.algorithm_params
                {
                    *linear_strength = value;
                }
                self.tone_mapping_config.preset = iced_video_player::ToneMappingPreset::Custom;
                if let Some(video) = &mut self.video_opt {
                    video.set_tone_mapping_config(self.tone_mapping_config.clone());
                }
                Task::none()
            }

            PlayerMessage::SetHableLinearAngle(value) => {
                if let iced_video_player::AlgorithmParams::Hable {
                    ref mut linear_angle,
                    ..
                } = &mut self.tone_mapping_config.algorithm_params
                {
                    *linear_angle = value;
                }
                self.tone_mapping_config.preset = iced_video_player::ToneMappingPreset::Custom;
                if let Some(video) = &mut self.video_opt {
                    video.set_tone_mapping_config(self.tone_mapping_config.clone());
                }
                Task::none()
            }

            PlayerMessage::SetHableToeStrength(value) => {
                if let iced_video_player::AlgorithmParams::Hable {
                    ref mut toe_strength,
                    ..
                } = &mut self.tone_mapping_config.algorithm_params
                {
                    *toe_strength = value;
                }
                self.tone_mapping_config.preset = iced_video_player::ToneMappingPreset::Custom;
                if let Some(video) = &mut self.video_opt {
                    video.set_tone_mapping_config(self.tone_mapping_config.clone());
                }
                Task::none()
            }

            PlayerMessage::SetMonitorBrightness(value) => {
                self.tone_mapping_config.monitor_brightness = value;
                if let Some(video) = &mut self.video_opt {
                    video.set_tone_mapping_config(self.tone_mapping_config.clone());
                }
                Task::none()
            }

            PlayerMessage::SetToneMappingBrightness(value) => {
                self.tone_mapping_config.brightness_adjustment = value;
                self.tone_mapping_config.preset = iced_video_player::ToneMappingPreset::Custom;
                if let Some(video) = &mut self.video_opt {
                    video.set_tone_mapping_config(self.tone_mapping_config.clone());
                }
                Task::none()
            }

            PlayerMessage::SetToneMappingContrast(value) => {
                self.tone_mapping_config.contrast_adjustment = value;
                self.tone_mapping_config.preset = iced_video_player::ToneMappingPreset::Custom;
                if let Some(video) = &mut self.video_opt {
                    video.set_tone_mapping_config(self.tone_mapping_config.clone());
                }
                Task::none()
            }
        }
    }
}
