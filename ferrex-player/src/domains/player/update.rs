use super::{messages::PlayerMessage, state::PlayerDomainState};

use crate::{
    common::messages::{CrossDomainEvent, DomainMessage, DomainUpdateResult},
    domains::{
        media::{
            self,
            messages::MediaMessage,
            selectors::{
                next_episode_by_order_with_repo,
                previous_episode_by_order_with_repo,
            },
        },
        player::video::load_video,
        ui::{
            self, messages::UiMessage, playback_ui::PlaybackMessage,
            shell_ui::UiShellMessage,
        },
    },
    infra::constants::player_controls,
};

use ferrex_core::player_prelude::{MediaID, MovieID};

use subwave_unified::video::BackendPreference;

use iced::{Task, window::Mode};
use log::{debug, error, info, trace, warn};
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
    app_state: &mut crate::state::State,
    message: PlayerMessage,
) -> DomainUpdateResult {
    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    profiling::scope!(crate::infra::profiling_scopes::scopes::PLAYER_UPDATE);

    // Convenience alias
    let state: &mut PlayerDomainState = &mut app_state.domains.player.state;
    let window_size = app_state.window_size;

    match message {
        PlayerMessage::PlayMedia(media) => {
            // TODO: Refactor ID passthrough
            // Fallback handler without MediaID - proceed without tracking
            info!("[Player] PlayMedia without ID - starting playback");
            // Delegate to PlayMediaWithId with no ID tracking
            update_player(
                app_state,
                PlayerMessage::PlayMediaWithId(
                    media,
                    MediaID::Movie(MovieID::new_uuid()),
                ),
            )
        }

        PlayerMessage::NavigateBack => {
            let update_task = if let Some(media_id) = state.current_media_id {
                let position = if let Some(video) = &mut state.video_opt {
                    video.position().as_secs_f64()
                } else {
                    state.last_valid_position
                };
                let duration = if let Some(video) = &mut state.video_opt {
                    video.duration().as_secs_f64()
                } else {
                    state.last_valid_duration
                };
                Task::done(DomainMessage::Media(
                    media::messages::MediaMessage::SendProgressUpdateWithData(
                        media_id, position, duration,
                    ),
                ))
            } else {
                Task::none()
            };

            let tasks = Task::batch(vec![
                update_task,
                Task::done(DomainMessage::Player(
                    PlayerMessage::ResetAfterStop,
                )),
                Task::done(DomainMessage::Ui(
                    UiShellMessage::NavigateBack.into(),
                )),
            ]);

            DomainUpdateResult::task(tasks)
        }

        PlayerMessage::NavigateHome => {
            let update_task = if let Some(media_id) = state.current_media_id {
                let position = if let Some(video) = &mut state.video_opt {
                    video.position().as_secs_f64()
                } else {
                    state.last_valid_position
                };
                let duration = if let Some(video) = &mut state.video_opt {
                    video.duration().as_secs_f64()
                } else {
                    state.last_valid_duration
                };
                Task::done(DomainMessage::Media(
                    media::messages::MediaMessage::SendProgressUpdateWithData(
                        media_id, position, duration,
                    ),
                ))
            } else {
                Task::none()
            };

            let tasks = Task::batch(vec![
                update_task,
                Task::done(DomainMessage::Player(
                    PlayerMessage::ResetAfterStop,
                )),
                Task::done(DomainMessage::Ui(
                    UiShellMessage::NavigateHome.into(),
                )),
            ]);

            DomainUpdateResult::task(tasks)
        }

        PlayerMessage::Play => {
            if let Some(video) = &mut state.video_opt {
                video.set_paused(false);
                DomainUpdateResult::task(Task::done(DomainMessage::Media(
                    media::messages::MediaMessage::SendProgressUpdateWithData(
                        state.current_media_id.unwrap(),
                        video.position().as_secs_f64(),
                        video.duration().as_secs_f64(),
                    ),
                )))
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }

        PlayerMessage::Pause => {
            if let Some(video) = &mut state.video_opt {
                video.set_paused(true);
                DomainUpdateResult::task(Task::done(DomainMessage::Media(
                    media::messages::MediaMessage::SendProgressUpdateWithData(
                        state.current_media_id.unwrap(),
                        video.position().as_secs_f64(),
                        video.duration().as_secs_f64(),
                    ),
                )))
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }

        PlayerMessage::PlayPause => {
            let task = if let Some(video) = &mut state.video_opt {
                let is_paused = video.paused();
                video.set_paused(!is_paused);
                DomainUpdateResult::task(Task::done(DomainMessage::Media(
                    media::messages::MediaMessage::SendProgressUpdateWithData(
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

        PlayerMessage::ResetAfterStop => {
            // Reset the player state after progress update has been sent
            state.reset();
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::Stop => {
            // Capture position and duration BEFORE reset
            let update_task = if let Some(media_id) = state.current_media_id {
                let position = state.last_valid_position;
                let duration = state.last_valid_duration;

                // Send final progress update with captured data
                Task::done(DomainMessage::Media(
                    media::messages::MediaMessage::SendProgressUpdateWithData(
                        media_id, position, duration,
                    ),
                ))
            } else {
                Task::none()
            };

            // Store tasks before reset
            let tasks = Task::batch(vec![
                update_task,
                Task::done(DomainMessage::Player(
                    PlayerMessage::ResetAfterStop,
                )),
                Task::done(DomainMessage::Ui(
                    UiShellMessage::NavigateBack.into(),
                )),
            ]);

            // Return tasks without resetting yet
            DomainUpdateResult::task(tasks)
        }

        PlayerMessage::Seek(position) => {
            // Just update UI position during drag, don't seek yet
            if let Some(_video) = &state.video_opt {
                state.dragging = true;
                state.last_valid_position = position;
                state.last_seek_position = Some(position);
                state.update_controls(true);
            }
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::SeekRelease => {
            // Perform the seek on release
            if let (Some(video), Some(media_id)) =
                (&mut state.video_opt, state.current_media_id)
            {
                state.dragging = false;

                // Use pending seek position if available, otherwise use last seek position
                let final_seek_position =
                    state.pending_seek_position.or(state.last_seek_position);

                if let Some(seek_position) = final_seek_position {
                    log::debug!(
                        "Starting seek to position: {:.2}s",
                        seek_position
                    );
                    state.seeking = true;
                    state.seek_started_time = Some(std::time::Instant::now());
                    let duration = Duration::try_from_secs_f64(seek_position)
                        .unwrap_or_default();
                    if let Err(err) = video.seek(duration, false) {
                        error!(
                            "Failed to seek video to {:.3}s: {}",
                            duration.as_secs_f64(),
                            err
                        );
                    }
                } else if let Some(seek_position) = state.last_seek_position {
                    // Update position immediately for better UX
                    state.last_valid_position = seek_position;
                    debug!(
                        "Seek initiated, position set to: {:.2}s",
                        seek_position
                    );
                }

                state.last_seek_position = None;
                state.pending_seek_position = None;
                state.last_seek_time = None;
                state.update_controls(true);

                // Send progress update after seek completes
                return DomainUpdateResult::task(Task::done(
                    DomainMessage::Media(
                        media::messages::MediaMessage::SendProgressUpdateWithData(
                            media_id,
                            state.last_valid_position,
                            state.last_valid_duration,
                        ),
                    ),
                ));
            }
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::SeekBarPressed => {
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
                    debug!(
                        "Seek bar pressed - starting drag at position: {:.2}s",
                        seek_position
                    );
                } else {
                    // Mouse was outside the seek bar's vertical hit zone
                    debug!(
                        "Seek bar pressed but mouse is outside valid vertical zone - ignoring"
                    );
                }
            }
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::SeekDone => {
            // Seek operation completed, clear seeking flag
            if let (Some(video), Some(media_id)) =
                (&mut state.video_opt, state.current_media_id)
            {
                let video_pos = video.position().as_secs_f64();
                debug!(
                    "SeekDone: Clearing seeking flag. Video position: {:.2}s, UI position: {:.2}s",
                    video_pos, state.last_valid_position
                );
                state.seeking = false;
                state.seek_started_time = None;
                // Send progress update after seek completes
                DomainUpdateResult::task(Task::done(DomainMessage::Media(
                    media::messages::MediaMessage::SendProgressUpdateWithData(
                        media_id,
                        video_pos,
                        state.last_valid_duration,
                    ),
                )))
            } else {
                debug!("SeekDone: Clearing seeking flag (no video)");
                DomainUpdateResult::task(Task::none())
            }
        }

        PlayerMessage::SeekRelative(secs) => {
            if let Some(video) = &mut state.video_opt {
                // Prefer backend position, then state.position, then last_valid_position
                let backend_pos = video.position().as_secs_f64();
                let base_pos = if backend_pos > 0.0 {
                    backend_pos
                } else {
                    state.last_valid_position
                };

                // Determine reliable duration for clamping
                let raw_duration =
                    state.source_duration.unwrap_or(state.last_valid_duration);
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
                let seek_to = Duration::try_from_secs_f64(new_position)
                    .unwrap_or_default();
                if let Err(err) = video.seek(seek_to, false) {
                    error!(
                        "Failed to seek video to {:.3}s: {}",
                        seek_to.as_secs_f64(),
                        err
                    );
                }

                // Update position immediately for better UX and remember as last valid

                if new_position > 0.0 {
                    state.last_valid_position = new_position;
                }

                state.update_controls(true);
            }
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::SetVolume(volume) => {
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

        PlayerMessage::ToggleMute => {
            if let Some(video) = &mut state.video_opt {
                state.is_muted = !state.is_muted;
                video.set_muted(state.is_muted);
            }
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::VideoLoaded(success) => {
            if success {
                // Query available tracks
                state.update_available_tracks();
                app_state.domains.ui.state.view = ui::types::ViewState::Player;
                DomainUpdateResult::task(Task::none())
            } else {
                app_state.domains.ui.state.view =
                    ui::types::ViewState::VideoError {
                        message: "Failed to load video".to_string(),
                    };
                DomainUpdateResult::task(Task::none())
            }
        }

        PlayerMessage::VideoReadyToPlay => {
            info!(
                "[Player] Video ready to play - loading with internal backend"
            );
            // Keep VideoReadyToPlay on the internal provider; explicit MPV
            // handoff is triggered via UI::PlayMediaWithIdInMpv / Player::PlayExternal
            DomainUpdateResult::task(
                load_video(app_state).map(DomainMessage::Player),
            )
        }

        PlayerMessage::EndOfStream => {
            info!("End of stream - finalizing playback");

            // Capture position and duration for final progress update
            if let Some(media_id) = state.current_media_id {
                let (position, duration) =
                    if let Some(video) = &mut state.video_opt {
                        (
                            video.position().as_secs_f64(),
                            video.duration().as_secs_f64(),
                        )
                    } else {
                        (state.last_valid_position, state.last_valid_duration)
                    };

                // If current is an episode, attempt to play the next; else exit
                if let MediaID::Episode(current_ep) = media_id {
                    let next_opt = crate::domains::media::selectors::next_episode_by_order_with_repo(
                        &app_state.domains.ui.state.repo_accessor,
                        current_ep,
                    );

                    if let Some(next_ep) = next_opt {
                        // Persist final progress, then start next episode (internal provider)
                        let tasks = Task::batch(vec![
                            Task::done(DomainMessage::Media(
                                media::messages::MediaMessage::SendProgressUpdateWithData(
                                    MediaID::Episode(current_ep),
                                    position,
                                    duration,
                                ),
                            )),
                            Task::done(DomainMessage::Ui(
                                UiMessage::Playback(
                                    PlaybackMessage::PlayMediaWithId(
                                        MediaID::Episode(next_ep),
                                    ),
                                ),
                            )),
                        ]);
                        return DomainUpdateResult::task(tasks);
                    }
                }

                // Fallback: no next episode -> reset and navigate back
                let tasks = Task::batch(vec![
                    Task::done(DomainMessage::Media(
                        media::messages::MediaMessage::SendProgressUpdateWithData(
                            media_id, position, duration,
                        ),
                    )),
                    Task::done(DomainMessage::Player(
                        PlayerMessage::ResetAfterStop,
                    )),
                    Task::done(DomainMessage::Ui(
                        UiShellMessage::NavigateBack.into(),
                    )),
                ]);
                DomainUpdateResult::task(tasks)
            } else {
                // No media id - just reset and navigate back
                let tasks = Task::batch(vec![
                    Task::done(DomainMessage::Player(
                        PlayerMessage::ResetAfterStop,
                    )),
                    Task::done(DomainMessage::Ui(
                        UiShellMessage::NavigateBack.into(),
                    )),
                ]);
                DomainUpdateResult::task(tasks)
            }
        }

        PlayerMessage::NewFrame => {
            // Also advance transient notifications (e.g., track toast)
            state.update_track_notification();
            let mut update_tks = false;
            if let Some(video) = &mut state.video_opt {
                if state.is_loading_video {
                    state.is_loading_video = false;
                }

                let num_aud_tks = state.available_audio_tracks.len();
                let num_sub_tks = state.available_subtitle_tracks.len();

                if num_aud_tks == 0 || num_sub_tks == 0 {
                    update_tks = true;
                }

                // Check for seek timeout (500ms)
                if state.seeking
                    && let Some(start_time) = state.seek_started_time
                    && start_time.elapsed() > Duration::from_millis(1000)
                {
                    warn!("Seek timeout: clearing seeking flag after 1s");
                    state.seeking = false;
                    state.seek_started_time = None;
                }

                // Update duration if it wasn't available during load
                if state.last_valid_duration <= 0.0 {
                    let new_duration = video.duration().as_secs_f64();
                    if new_duration > 0.0 {
                        info!(
                            "Duration now available: {} seconds",
                            new_duration
                        );
                        state.last_valid_duration = new_duration;
                        state.last_valid_duration = new_duration;
                    } else {
                        debug!(
                            "NewFrame: Duration still not available from video"
                        );
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
                            debug!(
                                "NewFrame: Position updated from {:.2}s to {:.2}s (duration: {:.2}s, source_duration: {:?})",
                                old_position,
                                new_position,
                                state.last_valid_duration,
                                state.source_duration
                            );
                        }
                    } else {
                        trace!(
                            "NewFrame: No valid position update (current: {:.2}s, new: {:.2}s)",
                            state.last_valid_position, new_position
                        );
                    }
                } else {
                    if state.seeking {
                        let video_pos = video.position().as_secs_f64();
                        debug!(
                            "NewFrame during seek: video reports {:.2}s, UI shows {:.2}s",
                            video_pos, state.last_valid_position
                        );
                    }
                    trace!(
                        "NewFrame: Skipping position update (dragging: {}, seeking: {})",
                        state.dragging, state.seeking
                    );
                }
            }
            if update_tks {
                state.update_available_tracks();
            }
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::Reload => {
            // This is handled in main.rs as it calls load_video
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::ShowControls => {
            state.update_controls(true);
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::ToggleFullscreen => {
            state.is_fullscreen = !state.is_fullscreen;
            let mode = if state.is_fullscreen {
                Mode::Fullscreen
            } else {
                Mode::Windowed
            };

            // Emit SetWindowMode event instead of managing window directly
            DomainUpdateResult::with_events(
                Task::none(),
                vec![CrossDomainEvent::SetWindowMode(mode)],
            )
        }

        PlayerMessage::DisableFullscreen => {
            if state.is_fullscreen {
                DomainUpdateResult::with_events(
                    Task::none(),
                    vec![CrossDomainEvent::SetWindowMode(Mode::Windowed)],
                )
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }

        PlayerMessage::ToggleSettings => {
            state.show_settings = !state.show_settings;

            // Close subtitle menu if open
            if state.show_settings {
                state.show_subtitle_menu = false;
            }
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::MouseMoved(point) => {
            use std::time::{Duration, Instant};

            // Update controls visibility
            state.update_controls(true);

            // Track vertical position for seek bar validation
            state.last_mouse_y = Some(point.y);

            // Check if we're within the seek bar's vertical hit zone
            // The seek bar is positioned at the bottom of the screen
            let seek_bar_vertical_center = window_size.height
                - player_controls::SEEK_BAR_CENTER_FROM_BOTTOM;
            let max_vertical_distance = super::state::SEEK_BAR_VISUAL_HEIGHT
                * super::state::SEEK_BAR_CLICK_TOLERANCE_MULTIPLIER;
            let within_seek_zone = (point.y - seek_bar_vertical_center).abs()
                <= max_vertical_distance;

            // Update seek bar hover state
            state.seek_bar_hovered = within_seek_zone;

            // Only calculate seek position if within vertical bounds OR already dragging
            if within_seek_zone || state.dragging {
                let percentage =
                    (point.x / window_size.width).clamp(0.0, 1.0) as f64;
                let duration =
                    state.source_duration.unwrap_or(state.last_valid_duration);
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
                let percentage =
                    (point.x / window_size.width).clamp(0.0, 1.0) as f64;
                let duration =
                    state.source_duration.unwrap_or(state.last_valid_duration);
                let seek_position = percentage * duration;

                // Update position immediately for responsive UI
                state.last_valid_position = seek_position;
                state.update_controls(true);

                // Throttle actual seeks to prevent overwhelming the network
                let should_seek = match state.last_seek_time {
                    Some(last_time) => {
                        last_time.elapsed() > Duration::from_millis(100)
                    }
                    None => true,
                };

                if should_seek {
                    // Perform the actual seek
                    if let Some(video) = state.video_opt.as_mut() {
                        let duration =
                            Duration::try_from_secs_f64(seek_position)
                                .unwrap_or_default();
                        if let Err(err) = video.seek(duration, false) {
                            error!(
                                "Failed to seek video to {:.3}s while dragging: {}",
                                duration.as_secs_f64(),
                                err
                            );
                        }
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

        PlayerMessage::VideoClicked => {
            let now = std::time::Instant::now();
            if let Some(last_click) = state.last_click_time {
                if now.duration_since(last_click).as_millis() < 300 {
                    // Double click detected
                    state.last_click_time = None;
                    update_player(app_state, PlayerMessage::ToggleFullscreen)
                } else {
                    // Single click
                    state.last_click_time = Some(now);
                    update_player(app_state, PlayerMessage::PlayPause)
                }
            } else {
                // First click
                state.last_click_time = Some(now);
                update_player(app_state, PlayerMessage::PlayPause)
            }
        }

        PlayerMessage::VideoDoubleClicked => {
            update_player(app_state, PlayerMessage::ToggleFullscreen)
        }

        PlayerMessage::SetPlaybackSpeed(speed) => {
            if let Some(video) = &mut state.video_opt {
                state.playback_speed = speed;
                let _ = video.set_speed(speed);
            }
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::SetContentFit(fit) => {
            state.content_fit = fit;
            DomainUpdateResult::task(Task::none())
        }

        // Track selection messages
        PlayerMessage::AudioTrackSelected(index) => {
            if let Err(e) = state.select_audio_track(index) {
                error!("{}", e);
            }
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::SubtitleTrackSelected(index) => {
            if let Err(e) = state.select_subtitle_track(index) {
                error!("{}", e);
            }
            // Close subtitle menu after selection
            state.show_subtitle_menu = false;
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::ToggleSubtitles => {
            if let Err(e) = state.toggle_subtitles() {
                error!("{}", e);
            }
            // Close subtitle menu after toggling
            state.show_subtitle_menu = false;
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::ToggleSubtitleMenu => {
            state.show_subtitle_menu = !state.show_subtitle_menu;
            // Close settings if open
            if state.show_subtitle_menu {
                state.show_settings = false;
            }
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::ToggleQualityMenu => {
            state.show_quality_menu = !state.show_quality_menu;
            // Close other menus if open
            if state.show_quality_menu {
                state.show_settings = false;
                state.show_subtitle_menu = false;
            }
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::ToggleAppsinkBackend => {
            if let Some(video) = state.video_opt.as_mut() {
                if std::env::var("WAYLAND_DISPLAY").is_ok() {
                    let current = video.backend();
                    let target = match current {
                        BackendPreference::ForceAppsink => {
                            BackendPreference::ForceWayland
                        }
                        _ => BackendPreference::ForceAppsink,
                    };
                    if let Err(e) = video.set_preference(target) {
                        error!("Failed to switch backend: {}", e);
                    } else {
                        info!("Switched backend to {:?}", target);
                    }
                } else {
                    // Not on Wayland; ensure Appsink
                    if let Err(e) = video.set_preference(
                        subwave_unified::video::BackendPreference::ForceAppsink,
                    ) {
                        error!("Failed to switch backend: {}", e);
                    }
                }
            }
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::CycleAudioTrack => {
            if let Err(e) = state.cycle_audio_track() {
                error!("{}", e);
            }
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::CycleSubtitleTrack => {
            if let Err(e) = state.cycle_subtitle_track() {
                error!("{}", e);
            }
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::CycleSubtitleSimple => {
            if let Err(e) = state.cycle_subtitle_simple() {
                error!("{}", e);
            }
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::TracksLoaded => {
            // Tracks have been loaded, update notification
            state.update_track_notification();
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::CheckControlsVisibility => {
            // Periodically clear notifications and hide controls if idle
            state.update_track_notification();
            if state.controls
                && state.controls_time.elapsed() > Duration::from_secs(3)
            {
                state.controls = false;
            }
            DomainUpdateResult::task(Task::none())
        }

        // New Phase 2 direct command handlers
        PlayerMessage::SeekTo(duration) => {
            // Convert Duration to f64 seconds and delegate to existing Seek handler
            let position = duration.as_secs_f64();
            update_player(app_state, PlayerMessage::Seek(position))
        }

        PlayerMessage::ToggleShuffle => {
            // Toggle shuffle state
            state.is_shuffle_enabled = !state.is_shuffle_enabled;
            info!("Shuffle toggled to: {}", state.is_shuffle_enabled);
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::ToggleRepeat => {
            // Toggle repeat state
            state.is_repeat_enabled = !state.is_repeat_enabled;
            info!("Repeat toggled to: {}", state.is_repeat_enabled);
            DomainUpdateResult::task(Task::none())
        }

        // Episode navigation: Next by ordering, Prev = restart or previous by ordering (<5%)
        PlayerMessage::NextEpisode => {
            let (current_episode_id, external_active, mid_opt, pos, dur) = {
                if let Some(ferrex_core::player_prelude::MediaID::Episode(ep)) =
                    state.current_media_id
                {
                    let (p, d) = if let Some(video) = &mut state.video_opt {
                        (
                            video.position().as_secs_f64(),
                            video.duration().as_secs_f64(),
                        )
                    } else {
                        (state.last_valid_position, state.last_valid_duration)
                    };
                    (
                        ep,
                        state.external_mpv_active,
                        state.current_media_id,
                        p,
                        d,
                    )
                } else {
                    return DomainUpdateResult::task(Task::none());
                }
            };

            // Resolve next strictly by ordering using repo accessor
            let next_opt = next_episode_by_order_with_repo(
                &app_state.domains.ui.state.repo_accessor,
                current_episode_id,
            );

            if let Some(next_ep_id) = next_opt {
                let progress_task = if let Some(mid) = mid_opt {
                    Task::done(DomainMessage::Media(
                        media::messages::MediaMessage::SendProgressUpdateWithData(
                            mid, pos, dur,
                        ),
                    ))
                } else {
                    Task::none()
                };

                let play_msg = if external_active {
                    UiMessage::Playback(PlaybackMessage::PlayMediaWithIdInMpv(
                        ferrex_core::player_prelude::MediaID::Episode(
                            next_ep_id,
                        ),
                    ))
                } else {
                    UiMessage::Playback(PlaybackMessage::PlayMediaWithId(
                        ferrex_core::player_prelude::MediaID::Episode(
                            next_ep_id,
                        ),
                    ))
                };

                let tasks = Task::batch(vec![
                    progress_task,
                    Task::done(DomainMessage::Ui(play_msg)),
                ]);
                DomainUpdateResult::task(tasks)
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }

        PlayerMessage::PreviousEpisode => {
            // Only valid for episodes
            let current_episode_id = match state.current_media_id {
                Some(ferrex_core::player_prelude::MediaID::Episode(ep)) => ep,
                _ => return DomainUpdateResult::task(Task::none()),
            };

            // Determine progress ratio using the most reliable numbers
            let (position, mut duration) =
                if let Some(video) = &mut state.video_opt {
                    (
                        video.position().as_secs_f64(),
                        video.duration().as_secs_f64(),
                    )
                } else {
                    (state.last_valid_position, state.last_valid_duration)
                };
            if let Some(src) = state.source_duration
                && src > 0.0
            {
                duration = src;
            }
            let ratio = if duration > 0.0 {
                position / duration
            } else {
                1.0
            };

            if ratio >= 0.05 {
                // Restart current episode from beginning
                if let Some(base) = prepare_restart_current_episode(
                    state,
                    state.external_mpv_active,
                    position,
                ) {
                    // Use immediate relative seek to 0 for internal player
                    update_player(app_state, PlayerMessage::SeekRelative(-base))
                } else {
                    DomainUpdateResult::task(Task::none())
                }
            } else {
                // Less than 5% watched: go to previous episode by ordering
                let (external_active, mid_opt, p, d) = {
                    let (p, d) = if let Some(video) = &mut state.video_opt {
                        (
                            video.position().as_secs_f64(),
                            video.duration().as_secs_f64(),
                        )
                    } else {
                        (state.last_valid_position, state.last_valid_duration)
                    };
                    (state.external_mpv_active, state.current_media_id, p, d)
                };

                let prev_opt = previous_episode_by_order_with_repo(
                    &app_state.domains.ui.state.repo_accessor,
                    current_episode_id,
                );

                if let Some(prev_ep_id) = prev_opt {
                    let progress_task = if let Some(mid) = mid_opt {
                        Task::done(DomainMessage::Media(
                            media::messages::MediaMessage::SendProgressUpdateWithData(
                                mid, p, d,
                            ),
                        ))
                    } else {
                        Task::none()
                    };

                    let play_msg = if external_active {
                        UiMessage::Playback(
                            PlaybackMessage::PlayMediaWithIdInMpv(
                                ferrex_core::player_prelude::MediaID::Episode(
                                    prev_ep_id,
                                ),
                            ),
                        )
                    } else {
                        UiMessage::Playback(PlaybackMessage::PlayMediaWithId(
                            ferrex_core::player_prelude::MediaID::Episode(
                                prev_ep_id,
                            ),
                        ))
                    };

                    let tasks = Task::batch(vec![
                        progress_task,
                        Task::done(DomainMessage::Ui(play_msg)),
                    ]);
                    DomainUpdateResult::task(tasks)
                } else {
                    // No previous episode -> restart current instead
                    if let Some(base) = prepare_restart_current_episode(
                        state,
                        external_active,
                        p,
                    ) {
                        update_player(
                            app_state,
                            PlayerMessage::SeekRelative(-base),
                        )
                    } else {
                        DomainUpdateResult::task(Task::none())
                    }
                }
            }
        }

        PlayerMessage::PlayMediaWithId(media, media_id) => {
            // Store current media and id
            state.current_media = Some(media.clone());
            state.current_media_id = Some(media_id);

            // Transfer pending resume position from media domain if available
            state.pending_resume_position =
                app_state.domains.media.state.pending_resume_position;
            app_state.domains.media.state.pending_resume_position = None;

            // Seed playback UI with the position we expect to resume from (or clear if none)
            state.last_valid_position = state
                .pending_resume_position
                .map(|pos| pos as f64)
                .unwrap_or(0.0);

            // Set duration from media metadata if available
            if let Some(metadata) = &media.media_file_metadata
                && let Some(duration) = metadata.duration
            {
                state.last_valid_duration = duration;
            }

            // HDR detection heuristics
            let is_hdr_content = if let Some(metadata) =
                &media.media_file_metadata
            {
                if let Some(bit_depth) = metadata.bit_depth {
                    bit_depth > 8
                } else if let Some(color_transfer) = &metadata.color_transfer {
                    ["smpte2084", "arib-std-b67", "smpte2086"]
                        .iter()
                        .any(|t| color_transfer.contains(t))
                } else if let Some(color_primaries) = &metadata.color_primaries
                {
                    color_primaries.contains("bt2020")
                } else {
                    false
                }
            } else {
                let f = media.filename.as_str();
                f.contains("2160p")
                    || f.contains("UHD")
                    || f.contains("HDR")
                    || f.contains("DV")
            };
            state.is_hdr_content = is_hdr_content;

            // Build secure streaming URL with access_token query
            let server_url = app_state.server_url.clone();
            let media_id_string = media.id.to_string();
            let api = app_state.api_service.clone();
            DomainUpdateResult::task(Task::perform(
                async move {
                    // URL-encode path component
                    let encoded_media_id =
                        urlencoding::encode(&media_id_string);
                    let base = format!(
                        "{}/api/v1/stream/{}",
                        server_url, encoded_media_id
                    );

                    // Request a short-lived playback ticket via authenticated API
                    let token_opt: Option<String> =
                        match api.fetch_playback_ticket(&media_id_string).await
                        {
                            Ok(token) => Some(token),
                            Err(e) => {
                                warn!("Failed to fetch playback ticket: {}", e);
                                None
                            }
                        };

                    // Attach token if present
                    let final_url = if let Some(token) = token_opt {
                        format!(
                            "{}?access_token={}",
                            base,
                            urlencoding::encode(&token)
                        )
                    } else {
                        base
                    };
                    Ok::<String, String>(final_url)
                },
                |url| match url {
                    Ok(u) => {
                        DomainMessage::Player(PlayerMessage::SetStreamUrl(u))
                    }
                    Err(e) => {
                        error!("Failed to construct stream URL: {}", e);
                        DomainMessage::Player(PlayerMessage::SetStreamUrl(
                            String::new(),
                        ))
                    }
                },
            ))
        }

        // External MPV player messages
        PlayerMessage::ExternalPlaybackStarted => {
            info!("External MPV playback started");
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::ExternalPlaybackUpdate { position, duration } => {
            // Update state with position from external MPV
            state.last_valid_position = position;
            state.last_valid_duration = duration;

            // Save watch progress
            if position > 0.0 && duration > 0.0 {
                state.last_progress_sent = position;
            }

            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::ExternalPlaybackEnded => {
            info!("External MPV playback ended");

            // Save final position and fullscreen state
            if let (Some(handle), Some(media_id)) =
                (&state.external_mpv_handle, state.current_media_id)
            {
                let final_position = handle.get_final_position();
                let final_fullscreen = handle.get_final_fullscreen();
                state.last_valid_position = final_position;
                state.is_fullscreen = final_fullscreen;

                // Decide: next episode (external) or exit
                if let MediaID::Episode(current_ep) = media_id {
                    let next_opt = next_episode_by_order_with_repo(
                        &app_state.domains.ui.state.repo_accessor,
                        current_ep,
                    );

                    // Clear external MPV state before proceeding
                    state.external_mpv_handle = None;
                    state.external_mpv_active = false;

                    if let Some(next_ep) = next_opt {
                        // Persist final progress, then start next in external MPV
                        let tasks = Task::batch(vec![
                            Task::done(DomainMessage::Media(
                                MediaMessage::SendProgressUpdateWithData(
                                    MediaID::Episode(current_ep),
                                    final_position,
                                    state.last_valid_duration,
                                ),
                            )),
                            Task::done(DomainMessage::Ui(UiMessage::Playback(
                                PlaybackMessage::PlayMediaWithIdInMpv(
                                    MediaID::Episode(next_ep),
                                ),
                            ))),
                        ]);
                        return DomainUpdateResult::task(tasks);
                    }

                    // No next episode -> fall through to exit path below
                }

                // Fallback: no next episode or not an episode -> send progress and exit
                let progress_task = Task::done(DomainMessage::Media(
                    MediaMessage::SendProgressUpdateWithData(
                        media_id,
                        final_position,
                        state.last_valid_duration,
                    ),
                ));

                // Clear external MPV state
                state.external_mpv_handle = None;
                state.external_mpv_active = false;

                let all_tasks = Task::batch(vec![
                    progress_task,
                    Task::done(DomainMessage::Player(
                        PlayerMessage::ResetAfterStop,
                    )),
                    Task::done(DomainMessage::Ui(
                        UiShellMessage::NavigateBack.into(),
                    )),
                ]);

                DomainUpdateResult::with_events(
                    all_tasks,
                    vec![CrossDomainEvent::RestoreWindow(final_fullscreen)],
                )
            } else {
                let tasks = Task::batch(vec![
                    Task::done(DomainMessage::Player(
                        PlayerMessage::ResetAfterStop,
                    )),
                    Task::done(DomainMessage::Ui(
                        UiShellMessage::NavigateBack.into(),
                    )),
                ]);
                DomainUpdateResult::task(tasks)
            }
        }

        PlayerMessage::ProgressHeartbeat => {
            // Periodic progress checkpoint from internal player
            if let Some(video) = state.video_opt.as_mut()
                && let Some(media_id) = state.current_media_id
            {
                let position = video.position().as_secs_f64();
                let duration = video.duration().as_secs_f64();

                if position > 0.0 && duration > 0.0 {
                    state.last_valid_position = position;
                    state.last_valid_duration = duration;

                    return DomainUpdateResult::task(Task::done(
                        DomainMessage::Media(
                            MediaMessage::SendProgressUpdateWithData(
                                media_id, position, duration,
                            ),
                        ),
                    ));
                }
            }
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::PollExternalMpv => {
            match (state.external_mpv_handle.take(), state.current_media_id) {
                (Some(mut handle), Some(media_id)) => {
                    // Check if MPV is still alive
                    if !handle.is_alive() {
                        info!("External MPV process has ended");

                        // Get final state before dropping the handle
                        let (position, duration) = handle.poll_position();
                        let final_fullscreen = handle.get_final_fullscreen();

                        info!(
                            "position: {:?}, duration: {:?}",
                            position, duration
                        );

                        state.last_valid_position = position;
                        state.is_fullscreen = final_fullscreen;

                        // Clear external MPV state
                        state.external_mpv_active = false;

                        // Send final progress update
                        let end_playback_task =
                            Task::done(DomainMessage::Player(
                                PlayerMessage::ExternalPlaybackEnded,
                            ));
                        let progress_task = Task::done(DomainMessage::Media(
                            MediaMessage::SendProgressUpdateWithData(
                                media_id, position, duration,
                            ),
                        ));

                        // Navigate back to previous view
                        let nav_task = Task::done(DomainMessage::Ui(
                            UiShellMessage::NavigateBack.into(),
                        ));

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
                                PlayerMessage::ExternalPlaybackUpdate {
                                    position,
                                    duration,
                                },
                            )
                        } else {
                            DomainUpdateResult::task(Task::none())
                        }
                    }
                }
                _ => DomainUpdateResult::task(Task::none()),
            }
        }

        PlayerMessage::PlayExternal => {
            start_external_mpv_with_current_url(app_state)
        }

        // Accept resolved URL and kick off playback
        PlayerMessage::SetStreamUrl(video_url) => {
            if video_url.is_empty() {
                // Should not happen; guard to avoid parsing panics
                app_state.domains.ui.state.error_message =
                    Some("Failed to resolve stream URL".to_string());
                app_state.domains.ui.state.view =
                    ui::types::ViewState::VideoError {
                        message: "Failed to resolve stream URL".to_string(),
                    };
                return DomainUpdateResult::task(Task::none());
            }

            match url::Url::parse(&video_url) {
                Ok(url) => {
                    // Set the new URL for the player
                    state.current_url = Some(url);

                    // If we're already in the Player view (e.g., next/prev episode while playing),
                    // keep the Player view and swap streams seamlessly without showing the loading page.
                    // Otherwise (e.g., initial play from library), show the loading view.
                    let in_player_already =
                        matches!(
                            app_state.domains.ui.state.view,
                            ui::types::ViewState::Player
                        ) || app_state.domains.player.state.video_opt.is_some();

                    // Clear any previous error
                    app_state.domains.ui.state.error_message = None;

                    if in_player_already {
                        // Ensure we stay on the Player view for near-instant transitions
                        app_state.domains.ui.state.view =
                            ui::types::ViewState::Player;
                        // Explicitly close the existing provider so load_video doesn't early-return
                        crate::domains::player::video::close_video(app_state);
                    } else {
                        // First-time play or not currently in player: show loading view briefly
                        app_state.domains.ui.state.view =
                            ui::types::ViewState::LoadingVideo {
                                url: video_url,
                            };
                    }

                    // Load the new video URL
                    DomainUpdateResult::task(
                        crate::domains::player::video::load_video(app_state)
                            .map(DomainMessage::Player),
                    )
                }
                Err(e) => {
                    app_state.domains.ui.state.error_message =
                        Some(format!("Invalid URL: {}", e));
                    app_state.domains.ui.state.view =
                        ui::types::ViewState::VideoError {
                            message: format!("Invalid URL: {}", e),
                        };
                    DomainUpdateResult::task(Task::none())
                }
            }
        }
    }
}

/// Prepare a restart of the current episode.
/// If `external_active` is true, seeks external MPV to the start and updates state,
/// returning None (no further action needed). If using the internal player and a
/// video is loaded, returns the normalized seek base so the caller can issue a
/// relative seek via `update_player`.
fn prepare_restart_current_episode(
    state: &mut PlayerDomainState,
    external_active: bool,
    position: f64,
) -> Option<f64> {
    if external_active {
        if let Some(handle) = state.external_mpv_handle.as_mut() {
            if let Err(e) = handle.seek_absolute(0.0) {
                error!("Failed to seek external MPV to start: {}", e);
            } else {
                state.last_valid_position = 0.0;
            }
        }
        None
    } else if state.video_opt.is_some() {
        Some(position.max(0.0))
    } else {
        None
    }
}

/// Start external MPV playback using the current URL and UI/window state.
/// Falls back to internal playback if MPV cannot be launched.
fn start_external_mpv_with_current_url(
    app_state: &mut crate::state::State,
) -> DomainUpdateResult {
    use crate::domains::player::messages::PlayerMessage;
    use crate::domains::ui;

    let state = &mut app_state.domains.player.state;

    // Resolve window attributes
    let is_fullscreen = state.is_fullscreen;
    let window_size = Some((
        app_state.window_size.width as u32,
        app_state.window_size.height as u32,
    ));
    let window_position =
        app_state.window_position.map(|p| (p.x as i32, p.y as i32));

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

    if url.is_empty() {
        // URL not ready yet (e.g., tokenization async); retry shortly
        info!("External MPV requested before stream URL resolved; retrying...");
        return DomainUpdateResult::task(Task::perform(
            async {
                tokio::time::sleep(tokio::time::Duration::from_millis(100))
                    .await;
            },
            |_| DomainMessage::Player(PlayerMessage::PlayExternal),
        ));
    }

    // Stop internal playback if running before handoff
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
                PlayerMessage::ExternalPlaybackStarted,
            )))
        }
        Err(e) => {
            // Fallback to internal provider
            error!(
                "Failed to start external MPV (falling back to internal): {}",
                e
            );
            state.external_mpv_active = false;
            DomainUpdateResult::task(
                load_video(app_state).map(DomainMessage::Player),
            )
        }
    }
}
