use super::messages::PlayerMessage;
use super::state::PlayerState;
use crate::Message;
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
                        self.seeking = true;
                        let duration =
                            Duration::try_from_secs_f64(seek_position).unwrap_or_default();
                        if let Err(e) = video.seek(duration, false) {
                            log::error!("Seek failed: {:?}", e);
                            self.seeking = false;
                        } else {
                            // Update position immediately for better UX
                            self.position = seek_position;
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
                // SeekBarMoved is handled in main.rs where we have access to window width
                Task::none()
            }

            PlayerMessage::SeekRelative(secs) => {
                // Relative seek implementation
                if let Some(video) = &mut self.video_opt {
                    // Get current position first
                    self.position = video.position().as_secs_f64();

                    // Calculate new position with bounds
                    let mut new_position = (self.position + secs).max(0.0);
                    if self.duration > 0.0 {
                        new_position = new_position.min(self.duration);
                    }

                    // Perform the seek
                    self.seeking = true;
                    let duration = Duration::try_from_secs_f64(new_position).unwrap_or_default();
                    if let Err(e) = video.seek(duration, false) {
                        log::error!("Relative seek failed: {:?}", e);
                        self.seeking = false;
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
                if let Some(video) = &self.video_opt {
                    // Update duration if it wasn't available during load
                    if self.duration <= 0.0 {
                        let new_duration = video.duration().as_secs_f64();
                        if new_duration > 0.0 {
                            log::info!("Duration now available: {} seconds", new_duration);
                            self.duration = new_duration;
                        }
                    }

                    // Update position when not dragging or seeking
                    if !self.dragging && !self.seeking {
                        // Normal position update
                        let new_position = video.position().as_secs_f64();
                        // Only update if we got a valid position
                        if new_position > 0.0 || self.position == 0.0 {
                            self.position = new_position;
                        }
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
                iced::window::get_latest().and_then(|id| iced::window::toggle_maximize(id))
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
        }
    }
}
