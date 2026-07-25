//! UI-agnostic playback reducer logic.
//!
//! Reducers update playback state and emit Iced tasks while keeping app-shell
//! navigation, persistence, and window effects outside this crate.

use crate::{
    constants::player_controls,
    contract::{
        BackendKind, BackendRequest, DurationDelta, EndReason, PlaybackCommand,
        PlaybackContentFit, PlaybackSnapshot, PlaybackSource, PlaybackState,
    },
    diagnostics::redact_playback_url,
    messages::PlayerMessage,
    state::PlayerDomainState,
    video::{close_video, load_video, media_file_metadata_indicates_hdr},
};
use ferrex_core::player_prelude::{EpisodeID, MediaID, MovieID};
use ferrex_player_api::services::api::ApiService;
use ferrex_player_foundation::domain::DomainUpdateResult;
use iced::{Task, window::Mode};
use log::{debug, error, info, warn};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

/// App-shell playback route used when playback asks the shell to start another item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackStartMode {
    Internal,
    MpvNativeWindow,
    External,
}

/// Window side effects emitted by the playback state machine.
#[derive(Debug, Clone)]
pub enum PlaybackWindowEvent {
    SetWindowMode(Mode),
    RestoreWindow(bool),
    /// A platform presenter has attached the hidden controls host and it is
    /// now safe for the shell to perform the single-visible-window handoff.
    NativePresenterAttached,
    /// Integrated presentation is unavailable or failed. Dismiss the hidden
    /// controls host while the selected native-window fallback keeps playing.
    NativePresenterUnavailable,
    /// Playback has fully exited. App shells use this idempotent signal to
    /// dismiss any dedicated controls host and restore the retained main
    /// window without coupling the playback reducer to a concrete window
    /// manager.
    PlaybackExited,
}

/// UI/view side effects required by video playback without depending on the final app facade.
pub trait PlaybackUiShell {
    fn is_player_view(&self) -> bool;
    fn set_player_view(&mut self);
    fn set_loading_video_view(&mut self, url: String);
    fn set_video_error(&mut self, message: String);
    fn clear_error(&mut self);
}

/// Watch-progress hints consumed by playback startup.
pub trait PlaybackWatchProgressPort {
    fn take_pending_resume_position(&mut self) -> Option<f32>;
}

/// Episode navigation queries needed by next/previous playback controls.
pub trait PlaybackEpisodeNavigator {
    fn next_episode(&self, current: EpisodeID) -> Option<EpisodeID>;
    fn previous_episode(&self, current: EpisodeID) -> Option<EpisodeID>;
}

/// Message factories supplied by the embedding app shell.
pub trait PlaybackUpdatePort {
    type AppMessage: Send + 'static;

    fn playback_message(message: PlayerMessage) -> Self::AppMessage;
    fn send_progress_update(
        media_id: MediaID,
        position: f64,
        duration: f64,
    ) -> Self::AppMessage;
    fn navigate_back() -> Self::AppMessage;
    fn navigate_home() -> Self::AppMessage;
    fn play_media_with_id(
        media_id: MediaID,
        mode: PlaybackStartMode,
    ) -> Self::AppMessage;
}

/// Runtime context passed from the app shell into the playback state machine.
#[allow(missing_debug_implementations)]
pub struct PlaybackUpdateContext<'a> {
    pub playback: &'a mut PlayerDomainState,
    pub watch_progress: &'a mut dyn PlaybackWatchProgressPort,
    pub ui: &'a mut dyn PlaybackUiShell,
    pub episodes: &'a dyn PlaybackEpisodeNavigator,
    pub api_service: Arc<dyn ApiService>,
    pub server_url: &'a str,
    pub window_size: iced::Size,
    pub window_position: Option<iced::Point>,
}

fn take_native_presenter_window_event(
    state: &mut PlayerDomainState,
) -> Option<PlaybackWindowEvent> {
    let (generation, event) = {
        let snapshot = state.playback_snapshot()?;
        let event = if snapshot.target
            == crate::contract::PlaybackTarget::MPV_INTEGRATED
        {
            match snapshot.presenter {
                crate::contract::PresenterState::Attached
                | crate::contract::PresenterState::Hidden
                | crate::contract::PresenterState::Suspended => {
                    Some(PlaybackWindowEvent::NativePresenterAttached)
                }
                crate::contract::PresenterState::Failed => {
                    Some(PlaybackWindowEvent::NativePresenterUnavailable)
                }
                crate::contract::PresenterState::Detached
                | crate::contract::PresenterState::AwaitingHost
                | crate::contract::PresenterState::AwaitingVideoOutput => None,
            }
        } else {
            snapshot
                .fallback_chain
                .iter()
                .any(|reason| {
                    reason.from
                        == Some(crate::contract::PlaybackTarget::MPV_INTEGRATED)
                })
                .then_some(PlaybackWindowEvent::NativePresenterUnavailable)
        }?;
        (snapshot.generation, event)
    };

    let emitted_generation = match event {
        PlaybackWindowEvent::NativePresenterAttached => {
            &mut state.native_presenter_attached_generation
        }
        PlaybackWindowEvent::NativePresenterUnavailable => {
            &mut state.native_presenter_unavailable_generation
        }
        PlaybackWindowEvent::SetWindowMode(_)
        | PlaybackWindowEvent::RestoreWindow(_)
        | PlaybackWindowEvent::PlaybackExited => {
            unreachable!(
                "native presenter reducer produced a non-presenter event"
            )
        }
    };

    if *emitted_generation == Some(generation) {
        None
    } else {
        *emitted_generation = Some(generation);
        Some(event)
    }
}

async fn resolve_playback_stream_source(
    api: Arc<dyn ApiService>,
    server_url: String,
    media_id_string: String,
) -> Result<PlaybackSource, String> {
    let base = build_protected_stream_url(&server_url, &media_id_string);
    let uri = url::Url::parse(&base).map_err(|_| {
        warn!("Could not construct a valid protected playback URL");
        "Could not prepare playback. Check the configured server URL."
            .to_string()
    })?;
    let token = Zeroizing::new(
        api.fetch_playback_ticket(&media_id_string)
            .await
            .map_err(|error| {
                let error = error.to_string();
                warn!(
                    "Failed to authorize playback stream: {}",
                    redact_playback_url(&error)
                );
                playback_ticket_failure_message(&error)
            })?,
    );

    if token.trim().is_empty() {
        warn!("Playback ticket endpoint returned an empty access token");
        return Err(
            "Could not authorize playback. Retry playback in a moment."
                .to_string(),
        );
    }
    if token.bytes().any(|byte| byte.is_ascii_control()) {
        warn!("Playback ticket endpoint returned an invalid access token");
        return Err(
            "Could not authorize playback. Retry playback in a moment."
                .to_string(),
        );
    }

    // The Ferrex stream endpoint accepts playback-scoped bearer tokens. Keep
    // the credential out of URLs and process arguments for every in-process
    // backend; the explicit legacy external-player boundary converts it only
    // when that compatibility path is selected.
    Ok(PlaybackSource::new(uri)
        .with_header("Authorization", format!("Bearer {}", token.as_str())))
}

fn build_protected_stream_url(
    server_url: &str,
    media_id_string: &str,
) -> String {
    let encoded_media_id = urlencoding::encode(media_id_string);
    format!(
        "{}/api/v1/stream/{}",
        server_url.trim_end_matches('/'),
        encoded_media_id
    )
}

fn playback_ticket_failure_message(error: &str) -> String {
    let lower = error.to_ascii_lowercase();
    if lower.contains("unauthorized")
        || lower.contains("please login")
        || lower.contains("login again")
        || lower.contains("token refresh")
    {
        "Playback authorization expired. Sign in again, then retry playback."
            .to_string()
    } else {
        "Could not authorize playback. Check your connection and retry."
            .to_string()
    }
}

fn drag_seek_is_due(last_dispatch: Option<Instant>, now: Instant) -> bool {
    last_dispatch.is_none_or(|last_dispatch| {
        now.saturating_duration_since(last_dispatch)
            >= crate::constants::seeking::SEEK_DRAG_THROTTLE
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnapshotDelivery {
    LegacyPolled,
    EventDriven,
}

fn snapshot_track_notifications(
    state: &PlayerDomainState,
    snapshot: &PlaybackSnapshot,
) -> Vec<String> {
    // Initial discovery and replacement-file loading are not user-visible
    // track changes. Once a catalog is established, selection changes from
    // scripts, demuxer updates, or backend policy use the same notification as
    // an Iced-issued selection.
    if matches!(snapshot.state, PlaybackState::Idle | PlaybackState::Loading)
        || state.track_catalog_generation != Some(snapshot.generation)
    {
        return Vec::new();
    }

    let mut notifications = Vec::new();
    if !state.available_audio_tracks.is_empty()
        && state.current_audio_track != snapshot.tracks.selected_audio
    {
        notifications.push(
            snapshot
                .tracks
                .selected_audio
                .as_ref()
                .and_then(|selected| {
                    snapshot
                        .tracks
                        .audio
                        .iter()
                        .find(|track| &track.id == selected)
                })
                .map(crate::track_selection::format_audio_track)
                .map(|track| format!("Audio: {track}"))
                .unwrap_or_else(|| "Audio: Unavailable".to_string()),
        );
    }

    if !state.available_subtitle_tracks.is_empty()
        && state.current_subtitle_track != snapshot.tracks.selected_subtitle
    {
        notifications.push(
            snapshot
                .tracks
                .selected_subtitle
                .as_ref()
                .and_then(|selected| {
                    snapshot
                        .tracks
                        .subtitles
                        .iter()
                        .find(|track| &track.id == selected)
                })
                .map(crate::track_selection::format_subtitle_track)
                .map(|track| format!("Subtitles: {track}"))
                .unwrap_or_else(|| "Subtitles: Disabled".to_string()),
        );
    }
    notifications
}

/// Project one backend-owned snapshot into the temporary UI compatibility
/// fields. Event-driven backends also own seek completion; the legacy adapter
/// retains its bounded timeout because Subwave does not expose a seeking
/// property.
fn apply_snapshot_to_domain(
    state: &mut PlayerDomainState,
    snapshot: &PlaybackSnapshot,
    delivery: SnapshotDelivery,
) {
    let track_notifications = snapshot_track_notifications(state, snapshot);
    let backend_seeking = snapshot.state == PlaybackState::Seeking;
    let terminal = matches!(
        snapshot.state,
        PlaybackState::Ended
            | PlaybackState::Failed
            | PlaybackState::Terminated
    );
    let position_is_previewed = state.dragging
        || match delivery {
            SnapshotDelivery::LegacyPolled => state.seeking,
            SnapshotDelivery::EventDriven => backend_seeking,
        };

    if !position_is_previewed
        && (snapshot.position > Duration::ZERO
            || state.last_valid_position <= 0.0)
    {
        state.last_valid_position = snapshot.position.as_secs_f64();
    }
    if let Some(duration) = snapshot.duration
        && duration > Duration::ZERO
    {
        state.last_valid_duration = duration.as_secs_f64();
    }

    state.buffered_percentage = snapshot.buffer.percentage.unwrap_or_default();
    state.available_audio_tracks = snapshot.tracks.audio.clone();
    state.current_audio_track = snapshot.tracks.selected_audio.clone();
    state.available_subtitle_tracks = snapshot.tracks.subtitles.clone();
    if !snapshot.tracks.audio.is_empty()
        || !snapshot.tracks.subtitles.is_empty()
    {
        state.track_catalog_generation = Some(snapshot.generation);
    }

    if let Some(selected) = snapshot.tracks.selected_subtitle.as_ref() {
        state.last_subtitle_track = Some(selected.clone());
    } else if let Some(previous) = state.current_subtitle_track.as_ref() {
        state.last_subtitle_track = Some(previous.clone());
    }
    state.current_subtitle_track = snapshot.tracks.selected_subtitle.clone();
    state.subtitles_enabled = snapshot.tracks.selected_subtitle.is_some();
    if !track_notifications.is_empty() {
        state.show_track_notification(track_notifications.join(" • "));
    }

    state.volume = snapshot.volume;
    state.is_muted = snapshot.muted;
    state.playback_speed = snapshot.speed;
    state.content_fit = match snapshot.content_fit {
        PlaybackContentFit::Contain => iced::ContentFit::Contain,
        PlaybackContentFit::Cover => iced::ContentFit::Cover,
        PlaybackContentFit::Fill => iced::ContentFit::Fill,
        PlaybackContentFit::None => iced::ContentFit::None,
        PlaybackContentFit::ScaleDown => iced::ContentFit::ScaleDown,
    };
    state.is_fullscreen = snapshot.fullscreen;
    state.is_loading_video = snapshot.state == PlaybackState::Loading;

    if delivery == SnapshotDelivery::EventDriven {
        state.seeking = backend_seeking;
        if !backend_seeking {
            state.seek_started_time = None;
        }
    }
    if terminal {
        state.seeking = false;
        state.seek_started_time = None;
        state.dragging = false;
    }
}

/// Handle a terminal snapshot exactly once for its generation. `Some` means
/// the snapshot was terminal, including terminal states that intentionally
/// produce no follow-up message.
fn handle_synchronized_terminal<P>(
    state: &mut PlayerDomainState,
    ui: &mut dyn PlaybackUiShell,
    snapshot: &PlaybackSnapshot,
) -> Option<Task<P::AppMessage>>
where
    P: PlaybackUpdatePort + 'static,
{
    if !matches!(
        snapshot.state,
        PlaybackState::Ended
            | PlaybackState::Failed
            | PlaybackState::Terminated
    ) {
        return None;
    }
    if state.terminal_generation_handled == Some(snapshot.generation) {
        return Some(Task::none());
    }
    state.terminal_generation_handled = Some(snapshot.generation);

    match snapshot.state {
        PlaybackState::Ended => match snapshot.end_reason {
            Some(EndReason::Eof) => Some(Task::done(P::playback_message(
                PlayerMessage::EndOfStream,
            ))),
            Some(EndReason::Closed) | Some(EndReason::BackendTerminated) => {
                Some(finish_terminated_playback::<P>(state, snapshot))
            }
            Some(EndReason::Stopped) | Some(EndReason::Replaced) | None => {
                Some(Task::none())
            }
        },
        PlaybackState::Terminated => {
            Some(finish_terminated_playback::<P>(state, snapshot))
        }
        PlaybackState::Failed
            if snapshot.target.backend == BackendKind::Mpv =>
        {
            let reason = snapshot
                .last_error
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "unknown mpv failure".to_string());
            warn!(
                "playback_fallback code=backend_failure from=mpv-native-window to=gstreamer-auto detail={reason}"
            );
            let final_progress = final_snapshot_progress(state, snapshot);
            let progress = final_progress.map_or_else(
                Task::none,
                |(media_id, position, duration)| {
                    Task::done(P::send_progress_update(
                        media_id, position, duration,
                    ))
                },
            );
            if let Some((_, position, _)) = final_progress
                && position > 0.0
            {
                state.pending_resume_position = Some(position as f32);
            }
            close_video(state);
            state.backend_request = BackendRequest::Auto;
            let fallback = load_video::<P>(state, ui);
            Some(sequence_tasks([progress, fallback]))
        }
        PlaybackState::Failed => {
            let message = snapshot
                .last_error
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "Playback backend failed".to_string());
            let progress = final_snapshot_progress(state, snapshot)
                .map_or_else(Task::none, |(media_id, position, duration)| {
                    Task::done(P::send_progress_update(
                        media_id, position, duration,
                    ))
                });
            close_video(state);
            ui.set_video_error(message);
            Some(progress)
        }
        _ => Some(Task::none()),
    }
}

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
pub fn update_player<P>(
    context: &mut PlaybackUpdateContext<'_>,
    message: PlayerMessage,
) -> DomainUpdateResult<Task<P::AppMessage>, PlaybackWindowEvent>
where
    P: PlaybackUpdatePort + 'static,
{
    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    profiling::scope!("Playback::Update");

    // Convenience alias
    let state: &mut PlayerDomainState = context.playback;
    let window_size = context.window_size;

    match message {
        PlayerMessage::PlayMedia(media) => {
            // Fallback handler without MediaID - proceed without tracking
            info!("[Player] PlayMedia without ID - starting playback");
            // Delegate to PlayMediaWithId with no ID tracking
            update_player::<P>(
                context,
                PlayerMessage::PlayMediaWithId(
                    media,
                    MediaID::Movie(MovieID::new_uuid()),
                ),
            )
        }

        PlayerMessage::NavigateBack => {
            let progress = current_playback_progress(state);
            let update_task = if let Some(media_id) = state.current_media_id {
                Task::done(P::send_progress_update(
                    media_id, progress.0, progress.1,
                ))
            } else {
                Task::none()
            };

            let tasks = sequence_tasks([
                update_task,
                Task::done(P::playback_message(PlayerMessage::ResetAfterStop)),
                Task::done(P::navigate_back()),
            ]);

            DomainUpdateResult::task(tasks)
        }

        PlayerMessage::NavigateHome => {
            let progress = current_playback_progress(state);
            let update_task = if let Some(media_id) = state.current_media_id {
                Task::done(P::send_progress_update(
                    media_id, progress.0, progress.1,
                ))
            } else {
                Task::none()
            };

            let tasks = sequence_tasks([
                update_task,
                Task::done(P::playback_message(PlayerMessage::ResetAfterStop)),
                Task::done(P::navigate_home()),
            ]);

            DomainUpdateResult::task(tasks)
        }

        PlayerMessage::Play => {
            let Some(video) = state.video_opt.as_mut() else {
                return DomainUpdateResult::task(Task::none());
            };
            video.set_paused(false);
            DomainUpdateResult::task(progress_checkpoint_task::<P>(state))
        }

        PlayerMessage::Pause => {
            let Some(video) = state.video_opt.as_mut() else {
                return DomainUpdateResult::task(Task::none());
            };
            video.set_paused(true);
            DomainUpdateResult::task(progress_checkpoint_task::<P>(state))
        }

        PlayerMessage::PlayPause => {
            let task = if let Some(video) = &mut state.video_opt {
                let is_paused = video.paused();
                video.set_paused(!is_paused);
                progress_checkpoint_task::<P>(state)
            } else {
                Task::none()
            };
            state.update_controls(true);
            DomainUpdateResult::task(task)
        }

        PlayerMessage::ResetAfterStop => {
            // Reset only after the final progress task has been constructed.
            // Every completed exit path funnels through this message, so one
            // idempotent shell event also restores a dedicated native-player
            // host for stop, EOF, close, back, and home navigation.
            state.reset();
            DomainUpdateResult::with_events(
                Task::none(),
                vec![PlaybackWindowEvent::PlaybackExited],
            )
        }

        PlayerMessage::Stop => {
            // Capture position and duration BEFORE reset
            let update_task = if let Some(media_id) = state.current_media_id {
                let position = state.last_valid_position;
                let duration = state.last_valid_duration;

                // Send final progress update with captured data
                Task::done(P::send_progress_update(
                    media_id, position, duration,
                ))
            } else {
                Task::none()
            };

            // Serialize progress, reset, and navigation messages so the shell
            // cannot tear down the player before persistence is dispatched.
            let tasks = sequence_tasks([
                update_task,
                Task::done(P::playback_message(PlayerMessage::ResetAfterStop)),
                Task::done(P::navigate_back()),
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
                    P::send_progress_update(
                        media_id,
                        state.last_valid_position,
                        state.last_valid_duration,
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
                DomainUpdateResult::task(Task::done(P::send_progress_update(
                    media_id,
                    video_pos,
                    state.last_valid_duration,
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
                let base_pos = if state.seeking {
                    state.last_valid_position
                } else if backend_pos > 0.0 {
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

                let effective_delta = new_position - base_pos;
                let Some(delta) = DurationDelta::from_seconds(effective_delta)
                else {
                    return DomainUpdateResult::task(Task::none());
                };
                if delta.magnitude() == Duration::ZERO {
                    state.last_valid_position = new_position;
                    state.update_controls(true);
                    return DomainUpdateResult::task(Task::none());
                }

                if let Err(err) =
                    video.apply_command(PlaybackCommand::SeekRelative(delta))
                {
                    error!(
                        "Failed to seek video by {:.3}s: {}",
                        effective_delta, err
                    );
                } else {
                    state.seeking = true;
                    state.seek_started_time = Some(Instant::now());
                    // Update position immediately for responsive controls. The
                    // event-driven snapshot confirms completion and replaces
                    // this prediction with the observed position.
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
                context.ui.set_player_view();
                let mut result = DomainUpdateResult::task(Task::none());
                if let Some(event) = take_native_presenter_window_event(state) {
                    result = result.add_event(event);
                }
                result
            } else {
                context
                    .ui
                    .set_video_error("Failed to load video".to_string());
                DomainUpdateResult::task(Task::none())
            }
        }

        PlayerMessage::VideoReadyToPlay => {
            info!(
                "[Player] Video ready to play - loading with internal backend"
            );
            // Load the selected in-process provider. The separate external
            // process handoff remains explicit through Player::PlayExternal.
            DomainUpdateResult::task(load_video::<P>(state, context.ui))
        }

        PlayerMessage::EndOfStream => {
            info!("End of stream - finalizing playback");

            // Capture position and duration for final progress update
            if let Some(media_id) = state.current_media_id {
                let (position, duration) = current_playback_progress(state);

                // If current is an episode, attempt to play the next; else exit
                if let MediaID::Episode(current_ep) = media_id {
                    let next_opt = context.episodes.next_episode(current_ep);

                    if let Some(next_ep) = next_opt {
                        let mode = current_playback_start_mode(state);
                        // Persist final progress, then retain the selected
                        // backend for the next episode.
                        let tasks = sequence_tasks([
                            Task::done(P::send_progress_update(
                                MediaID::Episode(current_ep),
                                position,
                                duration,
                            )),
                            Task::done(P::play_media_with_id(
                                MediaID::Episode(next_ep),
                                mode,
                            )),
                        ]);
                        return DomainUpdateResult::task(tasks);
                    }
                }

                // Fallback: no next episode -> reset and navigate back
                let tasks = sequence_tasks([
                    Task::done(P::send_progress_update(
                        media_id, position, duration,
                    )),
                    Task::done(P::playback_message(
                        PlayerMessage::ResetAfterStop,
                    )),
                    Task::done(P::navigate_back()),
                ]);
                DomainUpdateResult::task(tasks)
            } else {
                // No media id - just reset and navigate back
                let tasks = sequence_tasks([
                    Task::done(P::playback_message(
                        PlayerMessage::ResetAfterStop,
                    )),
                    Task::done(P::navigate_back()),
                ]);
                DomainUpdateResult::task(tasks)
            }
        }

        PlayerMessage::PlaybackSnapshotTick => {
            if state
                .video_opt
                .as_ref()
                .is_some_and(|video| video.uses_event_driven_snapshots())
            {
                return DomainUpdateResult::task(Task::none());
            }

            state.update_track_notification();
            if state.seeking
                && let Some(start_time) = state.seek_started_time
                && start_time.elapsed() > Duration::from_secs(1)
            {
                warn!("Seek timeout: clearing seeking flag after 1s");
                state.seeking = false;
                state.seek_started_time = None;
            }

            let refresh_tracks = state.available_audio_tracks.is_empty()
                || state.available_subtitle_tracks.is_empty();
            let snapshot = state.video_opt.as_mut().map(|video| {
                video.synchronize_snapshot();
                if refresh_tracks {
                    video.refresh_tracks();
                }
                video.snapshot().clone()
            });
            let Some(snapshot) = snapshot else {
                return DomainUpdateResult::task(Task::none());
            };

            apply_snapshot_to_domain(
                state,
                &snapshot,
                SnapshotDelivery::LegacyPolled,
            );
            if let Some(task) =
                handle_synchronized_terminal::<P>(state, context.ui, &snapshot)
            {
                DomainUpdateResult::task(task)
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }

        PlayerMessage::PlaybackEventsReady => {
            let snapshot = state.video_opt.as_mut().map(|video| {
                video.synchronize_snapshot();
                video.snapshot().clone()
            });
            let Some(snapshot) = snapshot else {
                return DomainUpdateResult::task(Task::none());
            };

            apply_snapshot_to_domain(
                state,
                &snapshot,
                SnapshotDelivery::EventDriven,
            );
            let mut result = if let Some(task) =
                handle_synchronized_terminal::<P>(state, context.ui, &snapshot)
            {
                DomainUpdateResult::task(task)
            } else {
                DomainUpdateResult::task(Task::none())
            };
            if let Some(event) = take_native_presenter_window_event(state) {
                result = result.add_event(event);
            }
            result
        }

        PlayerMessage::CaptureNativeVideoHost(window_id) => {
            #[cfg(feature = "ui")]
            {
                return DomainUpdateResult::task(
                    crate::native_video_slot::capture_iced_native_host(
                        window_id,
                    )
                    .map(move |result| {
                        P::playback_message(
                            PlayerMessage::NativeVideoHostCaptured {
                                window_id,
                                result: result
                                    .map(|_| ())
                                    .map_err(|error| error.to_string()),
                            },
                        )
                    }),
                );
            }

            #[cfg(not(feature = "ui"))]
            {
                let _ = window_id;
                DomainUpdateResult::task(Task::none())
            }
        }

        PlayerMessage::NativeVideoHostCaptured { window_id, result } => {
            log::debug!(
                "native presenter host capture task completed: success={}",
                result.is_ok()
            );
            if let Err(detail) = result
                && let Some(video) = state.video_opt.as_mut()
            {
                video.native_host_capture_failed(window_id, detail);
                video.synchronize_snapshot();
            }
            let mut update = DomainUpdateResult::task(Task::none());
            if let Some(event) = take_native_presenter_window_event(state) {
                update = update.add_event(event);
            }
            update
        }

        PlayerMessage::NativePresenterUpdated => {
            if let Some(video) = state.video_opt.as_mut() {
                video.synchronize_snapshot();
            }
            let mut result = DomainUpdateResult::task(Task::none());
            if let Some(event) = take_native_presenter_window_event(state) {
                result = result.add_event(event);
            }
            result
        }

        PlayerMessage::NativePresenterRefresh => {
            let before = state.playback_snapshot().map(|snapshot| {
                (
                    snapshot.target,
                    snapshot.presenter,
                    snapshot.fallback_chain.len(),
                )
            });
            if let Some(video) = state.video_opt.as_mut() {
                video.refresh_native_presenter();
            }
            let mut result = DomainUpdateResult::task(Task::none());
            let after = state.playback_snapshot().map(|snapshot| {
                (
                    snapshot.target,
                    snapshot.presenter,
                    snapshot.fallback_chain.len(),
                )
            });
            if before != after
                && let Some(event) = take_native_presenter_window_event(state)
            {
                result = result.add_event(event);
            }
            result
        }

        PlayerMessage::Reload => {
            // This is handled in main.rs as it calls load_video
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::ShowControls => {
            state.update_controls(true);
            if state
                .video_opt
                .as_ref()
                .is_some_and(|video| !video.uses_event_driven_snapshots())
            {
                update_player::<P>(context, PlayerMessage::PlaybackSnapshotTick)
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }

        PlayerMessage::ToggleFullscreen => {
            let native_mpv_fullscreen =
                state.playback_snapshot().and_then(|snapshot| {
                    (snapshot.target.backend == BackendKind::Mpv)
                        .then_some(snapshot.fullscreen)
                });

            if let Some(confirmed_fullscreen) = native_mpv_fullscreen {
                let requested_fullscreen = !confirmed_fullscreen;
                if let Some(video) = state.video_opt.as_mut()
                    && let Err(error) = video.apply_command(
                        crate::contract::PlaybackCommand::SetFullscreen(
                            requested_fullscreen,
                        ),
                    )
                {
                    warn!("Could not change mpv fullscreen state: {error}");
                }
                DomainUpdateResult::task(Task::none())
            } else {
                state.is_fullscreen = !state.is_fullscreen;
                let mode = if state.is_fullscreen {
                    Mode::Fullscreen
                } else {
                    Mode::Windowed
                };

                // Emit SetWindowMode event instead of managing window directly
                DomainUpdateResult::with_events(
                    Task::none(),
                    vec![PlaybackWindowEvent::SetWindowMode(mode)],
                )
            }
        }

        PlayerMessage::DisableFullscreen => {
            let native_mpv_fullscreen =
                state.playback_snapshot().and_then(|snapshot| {
                    (snapshot.target.backend == BackendKind::Mpv)
                        .then_some(snapshot.fullscreen)
                });
            if native_mpv_fullscreen == Some(true) {
                if let Some(video) = state.video_opt.as_mut()
                    && let Err(error) = video.apply_command(
                        crate::contract::PlaybackCommand::SetFullscreen(false),
                    )
                {
                    warn!("Could not leave mpv fullscreen state: {error}");
                }
                DomainUpdateResult::task(Task::none())
            } else if native_mpv_fullscreen.is_some() {
                DomainUpdateResult::task(Task::none())
            } else if state.is_fullscreen {
                DomainUpdateResult::with_events(
                    Task::none(),
                    vec![PlaybackWindowEvent::SetWindowMode(Mode::Windowed)],
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
            // Update controls visibility
            state.update_controls(true);

            // Track vertical position for seek bar validation
            state.last_mouse_y = Some(point.y);

            // Check if we're within the seek bar's vertical hit zone
            // The seek bar is positioned at the bottom of the screen
            let seek_bar_vertical_center = window_size.height
                - player_controls::SEEK_BAR_CENTER_FROM_BOTTOM;
            let max_vertical_distance = crate::state::SEEK_BAR_VISUAL_HEIGHT
                * crate::state::SEEK_BAR_CLICK_TOLERANCE_MULTIPLIER;
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

                // Throttle preview seeks at the UI boundary. Native adapters
                // also coalesce in-flight absolute seeks so delayed async
                // replies cannot turn pointer motion into an unbounded queue.
                let now = Instant::now();
                if drag_seek_is_due(state.last_seek_time, now) {
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
                        state.last_seek_time = Some(now);
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
                    update_player::<P>(context, PlayerMessage::ToggleFullscreen)
                } else {
                    // Single click
                    state.last_click_time = Some(now);
                    update_player::<P>(context, PlayerMessage::PlayPause)
                }
            } else {
                // First click
                state.last_click_time = Some(now);
                update_player::<P>(context, PlayerMessage::PlayPause)
            }
        }

        PlayerMessage::VideoDoubleClicked => {
            update_player::<P>(context, PlayerMessage::ToggleFullscreen)
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
            if let Some(video) = state.video_opt.as_mut()
                && let Err(error) = video.apply_command(
                    crate::contract::PlaybackCommand::SetContentFit(
                        crate::video::playback_content_fit(fit),
                    ),
                )
            {
                warn!("Could not change playback content fit: {error}");
            }
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

        PlayerMessage::ChapterSelected(chapter_id) => {
            if let Err(error) = state.select_chapter(chapter_id) {
                error!("{error}");
            }
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::EditionSelected(edition_id) => {
            if let Err(error) = state.select_edition(edition_id) {
                error!("{error}");
            }
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

        PlayerMessage::QualityProfileSelected(profile) => {
            state.current_quality_profile = Some(profile.as_str().to_string());
            state.show_quality_menu = false;
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::ToggleAppsinkBackend => {
            if let Some(video) = state.video_opt.as_mut() {
                let result = if std::env::var("WAYLAND_DISPLAY").is_ok() {
                    video.toggle_diagnostic_backend()
                } else {
                    video.force_appsink()
                };
                if let Err(error) = result {
                    error!(
                        "Failed to switch Subwave diagnostic backend: {error}"
                    );
                } else {
                    info!(
                        "Switched Subwave diagnostic backend (appsink: {})",
                        video.is_appsink()
                    );
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
            // Periodically clear notifications and hide controls if idle.
            // Legacy adapters reuse this bounded UI timer to synchronize their
            // snapshot; event-driven native backends never poll here.
            state.update_track_notification();
            let synchronize_legacy_snapshot = state
                .video_opt
                .as_ref()
                .is_some_and(|video| !video.uses_event_driven_snapshots());
            if state.controls
                && state.controls_time.elapsed() > Duration::from_secs(3)
            {
                state.controls = false;
            }
            if synchronize_legacy_snapshot {
                update_player::<P>(context, PlayerMessage::PlaybackSnapshotTick)
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }

        // New Phase 2 direct command handlers
        PlayerMessage::SeekTo(duration) => {
            let Some(video) = state.video_opt.as_mut() else {
                return DomainUpdateResult::task(Task::none());
            };
            if let Err(error) =
                video.apply_command(PlaybackCommand::SeekAbsolute(duration))
            {
                error!(
                    "Failed to seek video to {:.3}s: {error}",
                    duration.as_secs_f64()
                );
            } else {
                state.dragging = false;
                state.seeking = true;
                state.seek_started_time = Some(Instant::now());
                state.last_valid_position = duration.as_secs_f64();
                state.last_seek_position = None;
                state.pending_seek_position = None;
                state.last_seek_time = None;
                state.update_controls(true);
            }
            DomainUpdateResult::task(Task::none())
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
            let mode = current_playback_start_mode(state);
            let (current_episode_id, mid_opt) =
                if let Some(MediaID::Episode(episode)) = state.current_media_id
                {
                    (episode, state.current_media_id)
                } else {
                    return DomainUpdateResult::task(Task::none());
                };
            let (pos, dur) = current_playback_progress(state);

            // Resolve next strictly by ordering using repo accessor
            let next_opt = context.episodes.next_episode(current_episode_id);

            if let Some(next_ep_id) = next_opt {
                let progress_task = if let Some(mid) = mid_opt {
                    Task::done(P::send_progress_update(mid, pos, dur))
                } else {
                    Task::none()
                };

                let tasks = sequence_tasks([
                    progress_task,
                    Task::done(P::play_media_with_id(
                        MediaID::Episode(next_ep_id),
                        mode,
                    )),
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
            let (position, mut duration) = current_playback_progress(state);
            if let Some(src) = state.source_duration
                && src > 0.0
            {
                duration = src;
            }
            if should_restart_current_episode(position, duration) {
                // Restart current episode from beginning
                if let Some(base) =
                    prepare_restart_current_episode(state, position)
                {
                    // Use immediate relative seek to 0 for internal player
                    update_player::<P>(
                        context,
                        PlayerMessage::SeekRelative(-base),
                    )
                } else {
                    DomainUpdateResult::task(Task::none())
                }
            } else {
                // Less than 5% watched: go to previous episode by ordering
                let mode = current_playback_start_mode(state);
                let mid_opt = state.current_media_id;
                let (p, d) = current_playback_progress(state);

                let prev_opt =
                    context.episodes.previous_episode(current_episode_id);

                if let Some(prev_ep_id) = prev_opt {
                    let progress_task = if let Some(mid) = mid_opt {
                        Task::done(P::send_progress_update(mid, p, d))
                    } else {
                        Task::none()
                    };

                    let tasks = sequence_tasks([
                        progress_task,
                        Task::done(P::play_media_with_id(
                            MediaID::Episode(prev_ep_id),
                            mode,
                        )),
                    ]);
                    DomainUpdateResult::task(tasks)
                } else {
                    // No previous episode -> restart current instead
                    if let Some(base) =
                        prepare_restart_current_episode(state, p)
                    {
                        update_player::<P>(
                            context,
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
                context.watch_progress.take_pending_resume_position();

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

            // Content labeling uses server/decoder metadata only. Native HDR
            // output remains a separate observed diagnostic and is never
            // inferred from a filename or backend name.
            state.is_hdr_content = media
                .media_file_metadata
                .as_ref()
                .is_some_and(media_file_metadata_indicates_hdr);

            // Clear any previous stream before resolving a new authenticated URL.
            // External MPV may be requested before this async task completes, so a
            // stale URL must not be available for handoff.
            state.current_url = None;
            state.current_source = None;
            state.is_resolving_stream_url = true;
            state.stream_url_resolution_failed = false;

            let server_url = context.server_url.to_string();
            let media_id_string = media.id.to_string();
            let api = Arc::clone(&context.api_service);
            DomainUpdateResult::task(Task::perform(
                resolve_playback_stream_source(
                    api,
                    server_url,
                    media_id_string,
                ),
                |result| match result {
                    Ok(source) => P::playback_message(
                        PlayerMessage::SetStreamSource(source),
                    ),
                    Err(message) => P::playback_message(
                        PlayerMessage::StreamUrlResolutionFailed(message),
                    ),
                },
            ))
        }

        // External MPV player messages
        PlayerMessage::ExternalPlaybackStarted => {
            info!("External MPV playback started");
            state.mark_external_playback_started();
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::ExternalPlaybackUpdate { position, duration } => {
            // Reduce copied IPC values into the same snapshot consumed by
            // progress, episode, subscription, and view policy.
            state.update_external_playback_snapshot(position, duration);
            state.last_valid_position = position;
            state.last_valid_duration = duration;

            if position > 0.0 && duration > 0.0 {
                state.last_progress_sent = position;
            }

            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::ExternalPlaybackEnded => {
            info!("External MPV playback ended");

            // Polling captures the final IPC values into the snapshot before
            // dropping the process handle, so terminal behavior never depends
            // on a native object surviving into this message turn.
            let final_snapshot = state.external_mpv_snapshot.clone();
            let final_fullscreen = final_snapshot
                .as_ref()
                .map(|snapshot| snapshot.fullscreen)
                .unwrap_or(state.is_fullscreen);
            let final_position = final_snapshot
                .as_ref()
                .map(|snapshot| snapshot.position.as_secs_f64())
                .unwrap_or(state.last_valid_position);
            let final_duration = final_snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.duration)
                .map(|duration| duration.as_secs_f64())
                .unwrap_or(state.last_valid_duration);
            state.last_valid_position = final_position;
            state.last_valid_duration = final_duration;
            state.is_fullscreen = final_fullscreen;

            let Some(media_id) = state.current_media_id else {
                state.clear_external_playback();
                let tasks = sequence_tasks([
                    Task::done(P::playback_message(
                        PlayerMessage::ResetAfterStop,
                    )),
                    Task::done(P::navigate_back()),
                ]);
                return DomainUpdateResult::with_events(
                    tasks,
                    vec![PlaybackWindowEvent::RestoreWindow(final_fullscreen)],
                );
            };

            let next_episode = match media_id {
                MediaID::Episode(current) => {
                    context.episodes.next_episode(current)
                }
                _ => None,
            };
            state.clear_external_playback();

            if let (MediaID::Episode(current), Some(next)) =
                (media_id, next_episode)
            {
                let tasks = sequence_tasks([
                    Task::done(P::send_progress_update(
                        MediaID::Episode(current),
                        final_position,
                        final_duration,
                    )),
                    Task::done(P::play_media_with_id(
                        MediaID::Episode(next),
                        PlaybackStartMode::External,
                    )),
                ]);
                return DomainUpdateResult::with_events(
                    tasks,
                    vec![PlaybackWindowEvent::RestoreWindow(final_fullscreen)],
                );
            }

            let tasks = sequence_tasks([
                Task::done(P::send_progress_update(
                    media_id,
                    final_position,
                    final_duration,
                )),
                Task::done(P::playback_message(PlayerMessage::ResetAfterStop)),
                Task::done(P::navigate_back()),
            ]);
            DomainUpdateResult::with_events(
                tasks,
                vec![PlaybackWindowEvent::RestoreWindow(final_fullscreen)],
            )
        }

        PlayerMessage::ProgressHeartbeat => {
            // Periodic progress checkpoint from the backend-neutral snapshot.
            // This low-rate synchronization is also the terminal fallback for
            // legacy Subwave playback while the controls timer is inactive.
            let in_process = state.video_opt.is_some();
            let delivery = if state
                .video_opt
                .as_ref()
                .is_some_and(|video| video.uses_event_driven_snapshots())
            {
                SnapshotDelivery::EventDriven
            } else {
                SnapshotDelivery::LegacyPolled
            };
            let snapshot = if let Some(video) = state.video_opt.as_mut() {
                video.synchronize_snapshot();
                Some(video.snapshot().clone())
            } else {
                state.external_mpv_snapshot.clone()
            };
            let Some(snapshot) = snapshot else {
                return DomainUpdateResult::task(Task::none());
            };

            if in_process {
                apply_snapshot_to_domain(state, &snapshot, delivery);
                if let Some(task) = handle_synchronized_terminal::<P>(
                    state, context.ui, &snapshot,
                ) {
                    return DomainUpdateResult::task(task);
                }
            }

            if let Some((media_id, position, duration)) =
                final_snapshot_progress(state, &snapshot)
                && position > 0.0
                && duration > 0.0
            {
                return DomainUpdateResult::task(Task::done(
                    P::send_progress_update(media_id, position, duration),
                ));
            }
            DomainUpdateResult::task(Task::none())
        }

        PlayerMessage::PollExternalMpv => {
            let Some(mut handle) = state.external_mpv_handle.take() else {
                return DomainUpdateResult::task(Task::none());
            };

            // Drain pending IPC observations before checking the process result
            // so the final snapshot survives process-handle teardown.
            let (position, duration) = handle.poll_position();
            let fullscreen = handle.get_final_fullscreen();
            if !handle.is_alive() {
                info!(
                    "External MPV process ended at {:.3}s / {:.3}s",
                    position, duration
                );
                state.finish_external_playback(
                    position,
                    duration,
                    fullscreen,
                    EndReason::Eof,
                );
                state.last_valid_position = position;
                state.last_valid_duration = duration;
                state.is_fullscreen = fullscreen;
                drop(handle);

                DomainUpdateResult::task(Task::done(P::playback_message(
                    PlayerMessage::ExternalPlaybackEnded,
                )))
            } else {
                state.external_mpv_handle = Some(handle);
                state.update_external_playback_snapshot(position, duration);
                if position >= 0.0 {
                    state.last_valid_position = position;
                }
                if duration > 0.0 {
                    state.last_valid_duration = duration;
                }
                if position > 0.0 && duration > 0.0 {
                    state.last_progress_sent = position;
                }
                DomainUpdateResult::task(Task::none())
            }
        }

        PlayerMessage::PlayExternal => {
            start_external_mpv_with_current_url::<P>(context)
        }

        PlayerMessage::StreamUrlResolutionFailed(message) => {
            state.current_url = None;
            state.current_source = None;
            state.is_resolving_stream_url = false;
            state.stream_url_resolution_failed = true;
            state.is_loading_video = false;
            context.ui.set_video_error(message);
            DomainUpdateResult::task(Task::none())
        }

        // Accept a resolved source and kick off playback. The source keeps its
        // playback ticket in an Authorization header rather than the URI.
        PlayerMessage::SetStreamSource(source) => {
            state.is_resolving_stream_url = false;
            state.stream_url_resolution_failed = false;
            let display_url = source.uri().as_str().to_string();
            state.set_playback_source(source);

            // If we're already in the Player view (e.g., next/prev episode while playing),
            // keep the Player view and swap streams seamlessly without showing the loading page.
            // Otherwise (e.g., initial play from library), show the loading view.
            let in_player_already =
                context.ui.is_player_view() || state.video_opt.is_some();

            // Clear any previous error
            context.ui.clear_error();

            if in_player_already {
                // Ensure we stay on the Player view for near-instant transitions
                context.ui.set_player_view();
                // Explicitly close the existing provider so load_video doesn't early-return
                close_video(state);
            } else {
                // The displayed/loading URL contains no playback credential.
                context.ui.set_loading_video_view(display_url);
            }

            DomainUpdateResult::task(load_video::<P>(state, context.ui))
        }
    }
}

fn finish_terminated_playback<P>(
    state: &PlayerDomainState,
    snapshot: &crate::contract::PlaybackSnapshot,
) -> Task<P::AppMessage>
where
    P: PlaybackUpdatePort + 'static,
{
    let progress = final_snapshot_progress(state, snapshot).map_or_else(
        Task::none,
        |(media_id, position, duration)| {
            Task::done(P::send_progress_update(media_id, position, duration))
        },
    );

    // A native-window close is a user exit, not EOF. In particular, do not
    // auto-advance an episode after mpv's top-level window is closed.
    sequence_tasks([
        progress,
        Task::done(P::playback_message(PlayerMessage::ResetAfterStop)),
        Task::done(P::navigate_back()),
    ])
}

/// Read one backend-neutral progress pair. In-process adapters get one bounded
/// synchronization turn; external playback is already reduced by its IPC poll.
fn current_playback_progress(state: &mut PlayerDomainState) -> (f64, f64) {
    if let Some(video) = state.video_opt.as_mut() {
        video.synchronize_snapshot();
    }

    let Some(snapshot) = state.playback_snapshot() else {
        return (state.last_valid_position, state.last_valid_duration);
    };
    let observed_position = snapshot.position.as_secs_f64();
    let position =
        if observed_position > 0.0 || state.last_valid_position <= 0.0 {
            observed_position
        } else {
            state.last_valid_position
        };
    let duration = snapshot
        .duration
        .map(|duration| duration.as_secs_f64())
        .filter(|duration| *duration > 0.0)
        .unwrap_or(state.last_valid_duration);
    (position, duration)
}

/// Chain message-producing tasks in semantic order. Iced's `Task::batch`
/// intentionally runs streams in parallel, which is unsafe for final progress
/// followed by reset/navigation or replacement playback.
fn sequence_tasks<Message>(
    tasks: impl IntoIterator<Item = Task<Message>>,
) -> Task<Message>
where
    Message: Send + 'static,
{
    tasks
        .into_iter()
        .fold(Task::none(), |sequence, task| sequence.chain(task))
}

fn should_restart_current_episode(position: f64, duration: f64) -> bool {
    // Preserve the established player rule: at or beyond five percent,
    // Previous restarts the current episode. With no trustworthy duration,
    // restarting is safer than unexpectedly leaving the current item.
    !position.is_finite()
        || !duration.is_finite()
        || duration <= 0.0
        || position.max(0.0) / duration >= 0.05
}

fn progress_checkpoint_task<P>(
    state: &mut PlayerDomainState,
) -> Task<P::AppMessage>
where
    P: PlaybackUpdatePort + 'static,
{
    let progress = current_playback_progress(state);
    state.current_media_id.map_or_else(Task::none, |media_id| {
        Task::done(P::send_progress_update(media_id, progress.0, progress.1))
    })
}

fn final_snapshot_progress(
    state: &PlayerDomainState,
    snapshot: &crate::contract::PlaybackSnapshot,
) -> Option<(MediaID, f64, f64)> {
    let snapshot_position = snapshot.position.as_secs_f64();
    let position =
        if snapshot_position > 0.0 || state.last_valid_position <= 0.0 {
            snapshot_position
        } else {
            state.last_valid_position
        };
    Some((
        state.current_media_id?,
        position,
        snapshot
            .duration
            .map(|duration| duration.as_secs_f64())
            .filter(|duration| *duration > 0.0)
            .unwrap_or(state.last_valid_duration),
    ))
}

fn current_playback_start_mode(state: &PlayerDomainState) -> PlaybackStartMode {
    if state.is_external_playback() {
        PlaybackStartMode::External
    } else if state
        .playback_snapshot()
        .is_some_and(|snapshot| snapshot.target.backend == BackendKind::Mpv)
        || matches!(
            state.backend_request,
            BackendRequest::Exact(target) if target.backend == BackendKind::Mpv
        )
    {
        PlaybackStartMode::MpvNativeWindow
    } else {
        PlaybackStartMode::Internal
    }
}

/// Prepare a restart of the current episode. External-process selection is
/// read from the same backend-neutral snapshot used by episode policy.
fn prepare_restart_current_episode(
    state: &mut PlayerDomainState,
    position: f64,
) -> Option<f64> {
    if state.is_external_playback() {
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
fn start_external_mpv_with_current_url<P>(
    context: &mut PlaybackUpdateContext<'_>,
) -> DomainUpdateResult<Task<P::AppMessage>, PlaybackWindowEvent>
where
    P: PlaybackUpdatePort + 'static,
{
    let state = &mut *context.playback;

    // Resolve window attributes
    let is_fullscreen = state.is_fullscreen;
    let window_size = Some((
        context.window_size.width as u32,
        context.window_size.height as u32,
    ));
    let window_position =
        context.window_position.map(|p| (p.x as i32, p.y as i32));

    // Ensure handoff starts at the current native player position
    let (observed_position, _) = current_playback_progress(state);
    let resume_position = if observed_position > 0.0 {
        Some(observed_position as f32)
    } else {
        state.pending_resume_position
    };

    let url = state
        .current_source
        .as_ref()
        .map(external_mpv_url)
        .or_else(|| {
            state
                .current_url
                .as_ref()
                .map(|url| Zeroizing::new(url.to_string()))
        })
        .unwrap_or_else(|| Zeroizing::new(String::new()));

    if url.is_empty() {
        if state.is_resolving_stream_url {
            // URL not ready yet (e.g., tokenization async); retry shortly
            info!(
                "External MPV requested before stream URL resolved; retrying..."
            );
            return DomainUpdateResult::task(Task::perform(
                async {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100))
                        .await;
                },
                |_| P::playback_message(PlayerMessage::PlayExternal),
            ));
        }

        if !state.stream_url_resolution_failed {
            context.ui.set_video_error(
                "Playback stream is not ready. Retry playback.".to_string(),
            );
        }
        return DomainUpdateResult::task(Task::none());
    }

    // Stop internal playback if running before handoff
    state.stop_native_playback();

    match crate::external_mpv::start_external_playback(
        url.as_str(),
        is_fullscreen,
        window_size,
        window_position,
        resume_position,
    ) {
        Ok(handle) => {
            let Some(generation) = state.playback_generation.next() else {
                context.ui.set_video_error(
                    "Playback session generation exhausted".to_string(),
                );
                return DomainUpdateResult::task(Task::none());
            };
            state.playback_generation = generation;
            state.external_mpv_handle = Some(Box::new(handle));
            state.begin_external_playback(
                generation,
                resume_position.unwrap_or_default() as f64,
                state.last_valid_duration,
                is_fullscreen,
            );
            context.ui.set_player_view();

            DomainUpdateResult::task(Task::done(P::playback_message(
                PlayerMessage::ExternalPlaybackStarted,
            )))
        }
        Err(e) => {
            // Fallback to internal provider
            error!(
                "Failed to start external MPV (falling back to internal): {}",
                e
            );
            state.clear_external_playback();
            DomainUpdateResult::task(load_video::<P>(state, context.ui))
        }
    }
}

/// Convert a header-authenticated Ferrex source only at the explicit legacy
/// external-process boundary. In-process Subwave/libmpv loads never place the
/// playback ticket in their URL. The returned buffer is zeroized after spawn.
fn external_mpv_url(source: &PlaybackSource) -> Zeroizing<String> {
    let already_ticketed = source
        .uri()
        .query_pairs()
        .any(|(name, _)| name == "access_token");
    let bearer_ticket = (!already_ticketed)
        .then(|| {
            source.headers().iter().find_map(|header| {
                if !header.name.eq_ignore_ascii_case("authorization") {
                    return None;
                }
                header
                    .value
                    .expose_secret()
                    .strip_prefix("Bearer ")
                    .filter(|ticket| !ticket.is_empty())
            })
        })
        .flatten();

    let Some(ticket) = bearer_ticket else {
        return Zeroizing::new(source.uri().to_string());
    };

    // Serialize directly into the buffer that will be zeroized. Avoid putting
    // the header credential into a temporary `Url` allocation.
    let mut url = source.uri().clone();
    let fragment = url.fragment().map(str::to_owned);
    url.set_fragment(None);
    let mut output = url.to_string();
    output.push(if url.query().is_some() { '&' } else { '?' });
    let query_start = output.len();
    let mut serializer =
        url::form_urlencoded::Serializer::for_suffix(output, query_start);
    serializer.append_pair("access_token", ticket);
    let mut output = serializer.finish();
    if let Some(fragment) = fragment {
        output.push('#');
        output.push_str(&fragment);
    }
    Zeroizing::new(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrex_player_api::testing::TestApiService;
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Clone, PartialEq)]
    enum RecordedMessage {
        Playback(String),
        Progress(MediaID, f64, f64),
        Back,
        Home,
        Play(MediaID, PlaybackStartMode),
    }

    static RECORDS: Mutex<Vec<RecordedMessage>> = Mutex::new(Vec::new());
    static RECORDING_TEST: Mutex<()> = Mutex::new(());

    struct RecordingPort;

    impl PlaybackUpdatePort for RecordingPort {
        type AppMessage = RecordedMessage;

        fn playback_message(message: PlayerMessage) -> Self::AppMessage {
            let message = RecordedMessage::Playback(format!("{message:?}"));
            RECORDS.lock().unwrap().push(message.clone());
            message
        }

        fn send_progress_update(
            media_id: MediaID,
            position: f64,
            duration: f64,
        ) -> Self::AppMessage {
            let message =
                RecordedMessage::Progress(media_id, position, duration);
            RECORDS.lock().unwrap().push(message.clone());
            message
        }

        fn navigate_back() -> Self::AppMessage {
            RECORDS.lock().unwrap().push(RecordedMessage::Back);
            RecordedMessage::Back
        }

        fn navigate_home() -> Self::AppMessage {
            RECORDS.lock().unwrap().push(RecordedMessage::Home);
            RecordedMessage::Home
        }

        fn play_media_with_id(
            media_id: MediaID,
            mode: PlaybackStartMode,
        ) -> Self::AppMessage {
            let message = RecordedMessage::Play(media_id, mode);
            RECORDS.lock().unwrap().push(message.clone());
            message
        }
    }

    #[derive(Default)]
    struct TestUi {
        player: bool,
        error: Option<String>,
    }

    impl PlaybackUiShell for TestUi {
        fn is_player_view(&self) -> bool {
            self.player
        }

        fn set_player_view(&mut self) {
            self.player = true;
        }

        fn set_loading_video_view(&mut self, _url: String) {
            self.player = false;
        }

        fn set_video_error(&mut self, message: String) {
            self.error = Some(message);
        }

        fn clear_error(&mut self) {
            self.error = None;
        }
    }

    struct NoopWatchProgress;

    impl PlaybackWatchProgressPort for NoopWatchProgress {
        fn take_pending_resume_position(&mut self) -> Option<f32> {
            None
        }
    }

    struct NoopEpisodes;

    impl PlaybackEpisodeNavigator for NoopEpisodes {
        fn next_episode(&self, _current: EpisodeID) -> Option<EpisodeID> {
            None
        }

        fn previous_episode(&self, _current: EpisodeID) -> Option<EpisodeID> {
            None
        }
    }

    struct TestEpisodes {
        next: EpisodeID,
        previous: EpisodeID,
    }

    impl PlaybackEpisodeNavigator for TestEpisodes {
        fn next_episode(&self, _current: EpisodeID) -> Option<EpisodeID> {
            Some(self.next)
        }

        fn previous_episode(&self, _current: EpisodeID) -> Option<EpisodeID> {
            Some(self.previous)
        }
    }

    fn record_terminal_update(message: PlayerMessage) -> Vec<RecordedMessage> {
        RECORDS.lock().unwrap().clear();
        let media_id = MediaID::Movie(MovieID::new_uuid());
        let mut playback = PlayerDomainState {
            current_media_id: Some(media_id),
            last_valid_position: 42.5,
            last_valid_duration: 100.0,
            ..PlayerDomainState::default()
        };
        let mut watch_progress = NoopWatchProgress;
        let mut ui = TestUi {
            player: true,
            ..TestUi::default()
        };
        let episodes = NoopEpisodes;
        let api_service: Arc<dyn ApiService> =
            Arc::new(TestApiService::new("https://ferrex.example"));
        let mut context = PlaybackUpdateContext {
            playback: &mut playback,
            watch_progress: &mut watch_progress,
            ui: &mut ui,
            episodes: &episodes,
            api_service,
            server_url: "https://ferrex.example",
            window_size: iced::Size::new(1280.0, 720.0),
            window_position: None,
        };

        drop(update_player::<RecordingPort>(&mut context, message));
        std::mem::take(&mut *RECORDS.lock().unwrap())
    }

    #[test]
    fn stop_and_eof_persist_final_progress_before_exit() {
        let _serial = RECORDING_TEST.lock().unwrap();
        for message in [PlayerMessage::Stop, PlayerMessage::EndOfStream] {
            let records = record_terminal_update(message);
            assert!(matches!(
                records.first(),
                Some(RecordedMessage::Progress(_, 42.5, 100.0))
            ));
            assert!(records.contains(&RecordedMessage::Playback(
                "ResetAfterStop".to_string()
            )));
            assert!(records.contains(&RecordedMessage::Back));
        }
    }

    #[test]
    fn back_and_home_navigation_checkpoint_then_exit_playback() {
        let _serial = RECORDING_TEST.lock().unwrap();
        for (message, destination) in [
            (PlayerMessage::NavigateBack, RecordedMessage::Back),
            (PlayerMessage::NavigateHome, RecordedMessage::Home),
        ] {
            let records = record_terminal_update(message);
            assert!(matches!(
                records.first(),
                Some(RecordedMessage::Progress(_, 42.5, 100.0))
            ));
            assert!(records.contains(&RecordedMessage::Playback(
                "ResetAfterStop".to_string()
            )));
            assert!(records.contains(&destination));
        }
    }

    #[test]
    fn reset_after_stop_emits_one_backend_neutral_host_restore() {
        let _serial = RECORDING_TEST.lock().unwrap();
        RECORDS.lock().unwrap().clear();
        let mut playback = PlayerDomainState {
            current_media_id: Some(MediaID::Movie(MovieID::new_uuid())),
            last_valid_position: 42.5,
            ..PlayerDomainState::default()
        };
        let mut watch_progress = NoopWatchProgress;
        let mut ui = TestUi::default();
        let episodes = NoopEpisodes;
        let api_service: Arc<dyn ApiService> =
            Arc::new(TestApiService::new("https://ferrex.example"));
        let mut context = PlaybackUpdateContext {
            playback: &mut playback,
            watch_progress: &mut watch_progress,
            ui: &mut ui,
            episodes: &episodes,
            api_service,
            server_url: "https://ferrex.example",
            window_size: iced::Size::new(1280.0, 720.0),
            window_position: None,
        };

        let result = update_player::<RecordingPort>(
            &mut context,
            PlayerMessage::ResetAfterStop,
        );

        assert!(playback.current_media_id.is_none());
        assert!(matches!(
            result.events.as_slice(),
            [PlaybackWindowEvent::PlaybackExited]
        ));
    }

    fn state_with_presenter_snapshot(
        generation: crate::contract::SessionGeneration,
        target: crate::contract::PlaybackTarget,
        presenter: crate::contract::PresenterState,
    ) -> PlayerDomainState {
        let mut snapshot = crate::contract::PlaybackSnapshot::new(
            generation,
            target,
            crate::contract::PlaybackCapabilities::default(),
        );
        snapshot.presenter = presenter;
        PlayerDomainState {
            external_mpv_snapshot: Some(snapshot),
            ..PlayerDomainState::default()
        }
    }

    #[test]
    fn native_presenter_handoff_events_are_once_per_outcome_and_generation() {
        let first_generation = crate::contract::SessionGeneration::new(7);
        let mut state = state_with_presenter_snapshot(
            first_generation,
            crate::contract::PlaybackTarget::MPV_INTEGRATED,
            crate::contract::PresenterState::Attached,
        );

        assert!(matches!(
            take_native_presenter_window_event(&mut state),
            Some(PlaybackWindowEvent::NativePresenterAttached)
        ));

        for presenter in [
            crate::contract::PresenterState::Hidden,
            crate::contract::PresenterState::Suspended,
            crate::contract::PresenterState::Attached,
        ] {
            state
                .external_mpv_snapshot
                .as_mut()
                .expect("presenter snapshot")
                .presenter = presenter;
            assert!(take_native_presenter_window_event(&mut state).is_none());
        }

        state
            .external_mpv_snapshot
            .as_mut()
            .expect("presenter snapshot")
            .presenter = crate::contract::PresenterState::Failed;
        assert!(matches!(
            take_native_presenter_window_event(&mut state),
            Some(PlaybackWindowEvent::NativePresenterUnavailable)
        ));
        assert!(take_native_presenter_window_event(&mut state).is_none());

        let next_generation = first_generation.next().expect("next generation");
        let snapshot = state
            .external_mpv_snapshot
            .as_mut()
            .expect("presenter snapshot");
        snapshot.generation = next_generation;
        snapshot.presenter = crate::contract::PresenterState::Attached;
        assert!(matches!(
            take_native_presenter_window_event(&mut state),
            Some(PlaybackWindowEvent::NativePresenterAttached)
        ));
        state
            .external_mpv_snapshot
            .as_mut()
            .expect("presenter snapshot")
            .presenter = crate::contract::PresenterState::Failed;
        assert!(matches!(
            take_native_presenter_window_event(&mut state),
            Some(PlaybackWindowEvent::NativePresenterUnavailable)
        ));
    }

    #[test]
    fn native_presenter_fallback_unavailable_event_is_not_repeated() {
        let generation = crate::contract::SessionGeneration::new(11);
        let mut state = state_with_presenter_snapshot(
            generation,
            crate::contract::PlaybackTarget::MPV_NATIVE_WINDOW,
            crate::contract::PresenterState::Detached,
        );
        state
            .external_mpv_snapshot
            .as_mut()
            .expect("fallback snapshot")
            .fallback_chain
            .push(crate::contract::FallbackReason {
                code: crate::contract::FallbackReasonCode::PresenterFailed,
                from: Some(crate::contract::PlaybackTarget::MPV_INTEGRATED),
                to: crate::contract::PlaybackTarget::MPV_NATIVE_WINDOW,
                detail: "presenter unavailable".to_string(),
            });

        assert!(matches!(
            take_native_presenter_window_event(&mut state),
            Some(PlaybackWindowEvent::NativePresenterUnavailable)
        ));
        assert!(take_native_presenter_window_event(&mut state).is_none());

        state
            .external_mpv_snapshot
            .as_mut()
            .expect("fallback snapshot")
            .generation = generation.next().expect("next generation");
        assert!(matches!(
            take_native_presenter_window_event(&mut state),
            Some(PlaybackWindowEvent::NativePresenterUnavailable)
        ));
    }

    #[test]
    fn previous_episode_restart_policy_has_a_precise_five_percent_boundary() {
        assert!(!should_restart_current_episode(4.999, 100.0));
        assert!(should_restart_current_episode(5.0, 100.0));
        assert!(should_restart_current_episode(80.0, 100.0));
        assert!(should_restart_current_episode(0.0, 0.0));
        assert!(should_restart_current_episode(f64::NAN, 100.0));
        assert!(should_restart_current_episode(1.0, f64::INFINITY));
    }

    #[test]
    fn backend_error_progress_uses_last_owned_snapshot_values() {
        let media_id = MediaID::Movie(MovieID::new_uuid());
        let state = PlayerDomainState {
            current_media_id: Some(media_id),
            last_valid_position: 8.0,
            last_valid_duration: 90.0,
            ..PlayerDomainState::default()
        };
        let mut snapshot = crate::contract::PlaybackSnapshot::new(
            crate::contract::SessionGeneration::INITIAL,
            crate::contract::PlaybackTarget::MPV_NATIVE_WINDOW,
            crate::contract::PlaybackCapabilities::default(),
        );
        snapshot.position = Duration::from_millis(12_500);
        snapshot.duration = Some(Duration::from_secs(100));
        snapshot.state = crate::contract::PlaybackState::Failed;

        assert_eq!(
            final_snapshot_progress(&state, &snapshot),
            Some((media_id, 12.5, 100.0))
        );

        snapshot.position = Duration::ZERO;
        snapshot.duration = None;
        assert_eq!(
            final_snapshot_progress(&state, &snapshot),
            Some((media_id, 8.0, 90.0))
        );
    }

    #[test]
    fn native_window_close_persists_progress_and_does_not_advance_episode() {
        let _serial = RECORDING_TEST.lock().unwrap();
        let media_id = MediaID::Episode(EpisodeID::new());
        let state = PlayerDomainState {
            current_media_id: Some(media_id),
            ..PlayerDomainState::default()
        };

        for reason in [
            crate::contract::EndReason::Closed,
            crate::contract::EndReason::BackendTerminated,
        ] {
            RECORDS.lock().unwrap().clear();
            let mut snapshot = crate::contract::PlaybackSnapshot::new(
                crate::contract::SessionGeneration::INITIAL,
                crate::contract::PlaybackTarget::MPV_NATIVE_WINDOW,
                crate::contract::PlaybackCapabilities::default(),
            );
            snapshot.position = Duration::from_millis(42_500);
            snapshot.duration = Some(Duration::from_secs(100));
            snapshot.state = crate::contract::PlaybackState::Ended;
            snapshot.end_reason = Some(reason);

            drop(finish_terminated_playback::<RecordingPort>(
                &state, &snapshot,
            ));
            let records = std::mem::take(&mut *RECORDS.lock().unwrap());

            assert!(
                records.contains(&RecordedMessage::Progress(
                    media_id, 42.5, 100.0,
                ))
            );
            assert!(records.contains(&RecordedMessage::Playback(
                "ResetAfterStop".to_string()
            )));
            assert!(records.contains(&RecordedMessage::Back));
            assert!(
                !records
                    .iter()
                    .any(|record| matches!(record, RecordedMessage::Play(..)))
            );
        }
    }

    #[test]
    fn episode_transitions_persist_progress_and_preserve_backend_mode() {
        let _serial = RECORDING_TEST.lock().unwrap();
        let cases = [
            (BackendRequest::Auto, false, PlaybackStartMode::Internal),
            (
                BackendRequest::Exact(
                    crate::contract::PlaybackTarget::MPV_NATIVE_WINDOW,
                ),
                false,
                PlaybackStartMode::MpvNativeWindow,
            ),
            (BackendRequest::Auto, true, PlaybackStartMode::External),
        ];

        for (request, external, expected_mode) in cases {
            let current = EpisodeID::new();
            let next = EpisodeID::new();
            let previous = EpisodeID::new();
            let episodes = TestEpisodes { next, previous };

            for (message, expected_episode) in [
                (PlayerMessage::NextEpisode, next),
                (PlayerMessage::PreviousEpisode, previous),
                (PlayerMessage::EndOfStream, next),
            ] {
                RECORDS.lock().unwrap().clear();
                let external_mpv_snapshot = external.then(|| {
                    let mut snapshot = PlaybackSnapshot::new(
                        crate::contract::SessionGeneration::new(7),
                        crate::contract::PlaybackTarget::EXTERNAL_MPV,
                        crate::contract::PlaybackCapabilities::default(),
                    );
                    snapshot.state = PlaybackState::Playing;
                    snapshot.position = Duration::from_secs(2);
                    snapshot.duration = Some(Duration::from_secs(100));
                    snapshot
                });
                let mut playback = PlayerDomainState {
                    current_media_id: Some(MediaID::Episode(current)),
                    backend_request: request,
                    external_mpv_snapshot,
                    last_valid_position: 2.0,
                    last_valid_duration: 100.0,
                    ..PlayerDomainState::default()
                };
                let mut watch_progress = NoopWatchProgress;
                let mut ui = TestUi {
                    player: true,
                    ..TestUi::default()
                };
                let api_service: Arc<dyn ApiService> =
                    Arc::new(TestApiService::new("https://ferrex.example"));
                let mut context = PlaybackUpdateContext {
                    playback: &mut playback,
                    watch_progress: &mut watch_progress,
                    ui: &mut ui,
                    episodes: &episodes,
                    api_service,
                    server_url: "https://ferrex.example",
                    window_size: iced::Size::new(1280.0, 720.0),
                    window_position: None,
                };

                drop(update_player::<RecordingPort>(&mut context, message));
                let records = std::mem::take(&mut *RECORDS.lock().unwrap());

                assert!(records.contains(&RecordedMessage::Progress(
                    MediaID::Episode(current),
                    2.0,
                    100.0,
                )));
                assert!(records.contains(&RecordedMessage::Play(
                    MediaID::Episode(expected_episode),
                    expected_mode,
                )));
            }
        }
    }

    #[cfg(feature = "mpv")]
    #[test]
    #[ignore = "requires generated fixtures, linked libmpv, and a working desktop VO"]
    fn linked_native_window_eof_reloads_next_episode_with_same_backend() {
        let _serial = RECORDING_TEST.lock().unwrap();
        RECORDS.lock().unwrap().clear();

        let fixture = std::env::var_os("FERREX_MPV_SMOKE_MEDIA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
                    "../../target/native-playback-fixtures/h264-sdr-8bit.mkv",
                )
            });
        let fixture = std::fs::canonicalize(fixture).expect(
            "native playback fixture is missing; run native_playback_fixtures.py generate",
        );
        let fixture_uri = url::Url::from_file_path(&fixture)
            .expect("fixture path should convert to a file URL");

        let current = EpisodeID::new();
        let next = EpisodeID::new();
        let episodes = TestEpisodes {
            next,
            previous: EpisodeID::new(),
        };
        let mut playback = PlayerDomainState {
            current_media_id: Some(MediaID::Episode(current)),
            backend_request: BackendRequest::Exact(
                crate::contract::PlaybackTarget::MPV_NATIVE_WINDOW,
            ),
            ..PlayerDomainState::default()
        };
        let mut watch_progress = NoopWatchProgress;
        let mut ui = TestUi {
            player: true,
            ..TestUi::default()
        };
        let api_service: Arc<dyn ApiService> =
            Arc::new(TestApiService::new("https://ferrex.example"));
        let mut context = PlaybackUpdateContext {
            playback: &mut playback,
            watch_progress: &mut watch_progress,
            ui: &mut ui,
            episodes: &episodes,
            api_service,
            server_url: "https://ferrex.example",
            window_size: iced::Size::new(1280.0, 720.0),
            window_position: None,
        };

        drop(update_player::<RecordingPort>(
            &mut context,
            PlayerMessage::SetStreamSource(
                PlaybackSource::new(fixture_uri.clone())
                    .with_title("Synthetic episode one"),
            ),
        ));
        let first_generation = context
            .playback
            .playback_snapshot()
            .expect("first episode session")
            .generation;
        assert_eq!(
            context
                .playback
                .playback_snapshot()
                .map(|snapshot| snapshot.target),
            Some(crate::contract::PlaybackTarget::MPV_NATIVE_WINDOW)
        );

        let eof_deadline = Instant::now() + Duration::from_secs(10);
        loop {
            drop(update_player::<RecordingPort>(
                &mut context,
                PlayerMessage::PlaybackEventsReady,
            ));
            if RECORDS
                .lock()
                .unwrap()
                .contains(&RecordedMessage::Playback("EndOfStream".to_string()))
            {
                break;
            }
            let snapshot = context
                .playback
                .playback_snapshot()
                .expect("first episode snapshot");
            assert_ne!(
                snapshot.state,
                PlaybackState::Failed,
                "first episode failed: {:?}",
                snapshot.last_error
            );
            assert!(
                Instant::now() < eof_deadline,
                "first episode did not reach EOF: {snapshot:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let eof = context
            .playback
            .playback_snapshot()
            .expect("terminal first episode snapshot");
        assert_eq!(eof.state, PlaybackState::Ended);
        assert_eq!(eof.end_reason, Some(EndReason::Eof));

        RECORDS.lock().unwrap().clear();
        drop(update_player::<RecordingPort>(
            &mut context,
            PlayerMessage::EndOfStream,
        ));
        let transition = std::mem::take(&mut *RECORDS.lock().unwrap());
        assert!(transition.iter().any(|message| matches!(
            message,
            RecordedMessage::Progress(MediaID::Episode(id), position, duration)
                if *id == current && *position > 0.0 && *duration > 0.0
        )));
        assert!(transition.contains(&RecordedMessage::Play(
            MediaID::Episode(next),
            PlaybackStartMode::MpvNativeWindow,
        )));

        // Simulate the app shell resolving the emitted next-episode request.
        // SetStreamSource owns the real close/reopen path and must preserve the
        // exact mpv backend while advancing the session generation.
        context.playback.current_media_id = Some(MediaID::Episode(next));
        context.playback.pending_resume_position = None;
        drop(update_player::<RecordingPort>(
            &mut context,
            PlayerMessage::SetStreamSource(
                PlaybackSource::new(fixture_uri)
                    .with_title("Synthetic episode two"),
            ),
        ));

        let second_deadline = Instant::now() + Duration::from_secs(8);
        let second_generation = loop {
            drop(update_player::<RecordingPort>(
                &mut context,
                PlayerMessage::PlaybackEventsReady,
            ));
            let snapshot = context
                .playback
                .playback_snapshot()
                .expect("second episode snapshot");
            assert_ne!(
                snapshot.state,
                PlaybackState::Failed,
                "second episode failed: {:?}",
                snapshot.last_error
            );
            if matches!(
                snapshot.state,
                PlaybackState::Playing | PlaybackState::Paused
            ) && snapshot.duration.is_some()
            {
                assert!(snapshot.generation > first_generation);
                assert_eq!(
                    snapshot.target,
                    crate::contract::PlaybackTarget::MPV_NATIVE_WINDOW
                );
                break snapshot.generation;
            }
            assert!(
                Instant::now() < second_deadline,
                "second episode did not start: {snapshot:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        };

        eprintln!(
            "native-mpv episode replacement passed: generation={} -> {}",
            first_generation.get(),
            second_generation.get()
        );
        close_video(context.playback);
    }

    #[test]
    fn external_terminal_snapshot_survives_handle_drop_and_advances_episode() {
        let _serial = RECORDING_TEST.lock().unwrap();
        RECORDS.lock().unwrap().clear();
        let current = EpisodeID::new();
        let next = EpisodeID::new();
        let previous = EpisodeID::new();
        let episodes = TestEpisodes { next, previous };
        let mut snapshot = PlaybackSnapshot::new(
            crate::contract::SessionGeneration::new(13),
            crate::contract::PlaybackTarget::EXTERNAL_MPV,
            crate::contract::PlaybackCapabilities::default(),
        );
        snapshot.state = PlaybackState::Ended;
        snapshot.end_reason = Some(EndReason::Eof);
        snapshot.position = Duration::from_millis(42_500);
        snapshot.duration = Some(Duration::from_secs(100));

        let mut playback = PlayerDomainState {
            current_media_id: Some(MediaID::Episode(current)),
            external_mpv_snapshot: Some(snapshot),
            ..PlayerDomainState::default()
        };
        let mut watch_progress = NoopWatchProgress;
        let mut ui = TestUi {
            player: true,
            ..TestUi::default()
        };
        let api_service: Arc<dyn ApiService> =
            Arc::new(TestApiService::new("https://ferrex.example"));
        let mut context = PlaybackUpdateContext {
            playback: &mut playback,
            watch_progress: &mut watch_progress,
            ui: &mut ui,
            episodes: &episodes,
            api_service,
            server_url: "https://ferrex.example",
            window_size: iced::Size::new(1280.0, 720.0),
            window_position: None,
        };

        drop(update_player::<RecordingPort>(
            &mut context,
            PlayerMessage::ExternalPlaybackEnded,
        ));
        let records = std::mem::take(&mut *RECORDS.lock().unwrap());

        assert!(records.contains(&RecordedMessage::Progress(
            MediaID::Episode(current),
            42.5,
            100.0,
        )));
        assert!(records.contains(&RecordedMessage::Play(
            MediaID::Episode(next),
            PlaybackStartMode::External,
        )));
        assert!(!records.contains(&RecordedMessage::Back));
        assert!(playback.external_mpv_snapshot.is_none());
    }

    #[test]
    fn external_snapshot_participates_in_progress_heartbeat() {
        let _serial = RECORDING_TEST.lock().unwrap();
        RECORDS.lock().unwrap().clear();
        let media_id = MediaID::Movie(MovieID::new_uuid());
        let mut snapshot = PlaybackSnapshot::new(
            crate::contract::SessionGeneration::new(14),
            crate::contract::PlaybackTarget::EXTERNAL_MPV,
            crate::contract::PlaybackCapabilities::default(),
        );
        snapshot.state = PlaybackState::Playing;
        snapshot.position = Duration::from_secs(17);
        snapshot.duration = Some(Duration::from_secs(90));

        let mut playback = PlayerDomainState {
            current_media_id: Some(media_id),
            external_mpv_snapshot: Some(snapshot),
            ..PlayerDomainState::default()
        };
        let mut watch_progress = NoopWatchProgress;
        let mut ui = TestUi {
            player: true,
            ..TestUi::default()
        };
        let episodes = NoopEpisodes;
        let api_service: Arc<dyn ApiService> =
            Arc::new(TestApiService::new("https://ferrex.example"));
        let mut context = PlaybackUpdateContext {
            playback: &mut playback,
            watch_progress: &mut watch_progress,
            ui: &mut ui,
            episodes: &episodes,
            api_service,
            server_url: "https://ferrex.example",
            window_size: iced::Size::new(1280.0, 720.0),
            window_position: None,
        };

        drop(update_player::<RecordingPort>(
            &mut context,
            PlayerMessage::ProgressHeartbeat,
        ));

        assert!(
            RECORDS
                .lock()
                .unwrap()
                .contains(&RecordedMessage::Progress(media_id, 17.0, 90.0))
        );
    }

    #[test]
    fn event_snapshot_projection_waits_for_confirmed_seek_completion() {
        let mut state = PlayerDomainState {
            last_valid_position: 30.0,
            seeking: true,
            seek_started_time: Some(Instant::now()),
            ..PlayerDomainState::default()
        };
        let mut snapshot = PlaybackSnapshot::new(
            crate::contract::SessionGeneration::INITIAL,
            crate::contract::PlaybackTarget::MPV_NATIVE_WINDOW,
            crate::contract::PlaybackCapabilities::default(),
        );
        snapshot.state = PlaybackState::Seeking;
        snapshot.position = Duration::from_secs(12);
        snapshot.duration = Some(Duration::from_secs(100));
        snapshot.volume = 0.4;
        snapshot.muted = true;
        snapshot.speed = 1.25;
        snapshot.content_fit = PlaybackContentFit::Cover;

        apply_snapshot_to_domain(
            &mut state,
            &snapshot,
            SnapshotDelivery::EventDriven,
        );

        assert!(state.seeking);
        assert_eq!(state.last_valid_position, 30.0);
        assert_eq!(state.last_valid_duration, 100.0);

        snapshot.state = PlaybackState::Playing;
        snapshot.position = Duration::from_millis(42_500);
        apply_snapshot_to_domain(
            &mut state,
            &snapshot,
            SnapshotDelivery::EventDriven,
        );

        assert!(!state.seeking);
        assert!(state.seek_started_time.is_none());
        assert_eq!(state.last_valid_position, 42.5);
        assert_eq!(state.volume, 0.4);
        assert!(state.is_muted);
        assert_eq!(state.playback_speed, 1.25);
        assert_eq!(state.content_fit, iced::ContentFit::Cover);
    }

    #[test]
    fn established_snapshot_track_changes_emit_one_backend_neutral_notice() {
        let english_audio = crate::contract::AudioTrack {
            id: crate::contract::TrackId::new("audio:eng"),
            title: None,
            language: Some("eng".to_string()),
            codec: Some("aac".to_string()),
            channels: Some(2),
            sample_rate: None,
            is_default: true,
            is_forced: false,
        };
        let japanese_audio = crate::contract::AudioTrack {
            id: crate::contract::TrackId::new("audio:jpn"),
            title: None,
            language: Some("jpn".to_string()),
            codec: Some("aac".to_string()),
            channels: Some(2),
            sample_rate: None,
            is_default: false,
            is_forced: false,
        };
        let english_subtitle = crate::contract::SubtitleTrack {
            id: crate::contract::TrackId::new("subtitle:eng"),
            title: None,
            language: Some("eng".to_string()),
            codec: Some("ass".to_string()),
            kind: crate::contract::SubtitleKind::Text,
            is_default: true,
            is_forced: false,
            is_external: false,
        };
        let mut state = PlayerDomainState {
            track_catalog_generation: Some(
                crate::contract::SessionGeneration::INITIAL,
            ),
            available_audio_tracks: vec![english_audio.clone()],
            current_audio_track: Some(english_audio.id.clone()),
            available_subtitle_tracks: vec![english_subtitle.clone()],
            current_subtitle_track: Some(english_subtitle.id.clone()),
            subtitles_enabled: true,
            ..PlayerDomainState::default()
        };
        let mut snapshot = PlaybackSnapshot::new(
            crate::contract::SessionGeneration::INITIAL,
            crate::contract::PlaybackTarget::MPV_NATIVE_WINDOW,
            crate::contract::PlaybackCapabilities::default(),
        );
        snapshot.state = PlaybackState::Playing;
        snapshot.tracks.audio =
            vec![english_audio.clone(), japanese_audio.clone()];
        snapshot.tracks.selected_audio = Some(japanese_audio.id.clone());
        snapshot.tracks.subtitles = vec![english_subtitle];
        snapshot.tracks.selected_subtitle = None;

        apply_snapshot_to_domain(
            &mut state,
            &snapshot,
            SnapshotDelivery::EventDriven,
        );

        let notice = state
            .track_notification
            .as_ref()
            .expect("selection changes are visible")
            .message
            .clone();
        assert!(notice.contains("Audio: Japanese"));
        assert!(notice.contains("Subtitles: Disabled"));

        state.track_notification = None;
        apply_snapshot_to_domain(
            &mut state,
            &snapshot,
            SnapshotDelivery::EventDriven,
        );
        assert!(state.track_notification.is_none());

        let mut initial = PlayerDomainState::default();
        apply_snapshot_to_domain(
            &mut initial,
            &snapshot,
            SnapshotDelivery::EventDriven,
        );
        assert!(initial.track_notification.is_none());

        let mut replacement = snapshot.clone();
        replacement.generation = crate::contract::SessionGeneration::new(2);
        replacement.tracks.selected_audio = Some(english_audio.id);
        apply_snapshot_to_domain(
            &mut initial,
            &replacement,
            SnapshotDelivery::EventDriven,
        );
        assert!(initial.track_notification.is_none());
        assert_eq!(
            initial.track_catalog_generation,
            Some(crate::contract::SessionGeneration::new(2))
        );
    }

    #[test]
    fn loading_snapshot_does_not_erase_a_pending_resume_hint() {
        let mut state = PlayerDomainState {
            last_valid_position: 45.0,
            pending_resume_position: Some(45.0),
            ..PlayerDomainState::default()
        };
        let mut snapshot = PlaybackSnapshot::new(
            crate::contract::SessionGeneration::INITIAL,
            crate::contract::PlaybackTarget::MPV_NATIVE_WINDOW,
            crate::contract::PlaybackCapabilities::default(),
        );
        snapshot.state = PlaybackState::Loading;

        apply_snapshot_to_domain(
            &mut state,
            &snapshot,
            SnapshotDelivery::EventDriven,
        );

        assert_eq!(state.last_valid_position, 45.0);
        assert!(state.is_loading_video);
    }

    #[test]
    fn synchronized_eof_and_backend_error_have_one_terminal_dispatch() {
        let _serial = RECORDING_TEST.lock().unwrap();
        let media_id = MediaID::Movie(MovieID::new_uuid());
        let mut ui = TestUi {
            player: true,
            ..TestUi::default()
        };

        RECORDS.lock().unwrap().clear();
        let mut eof_state = PlayerDomainState {
            current_media_id: Some(media_id),
            ..PlayerDomainState::default()
        };
        let mut eof = PlaybackSnapshot::new(
            crate::contract::SessionGeneration::new(11),
            crate::contract::PlaybackTarget::GSTREAMER_EMBEDDED,
            crate::contract::PlaybackCapabilities::default(),
        );
        eof.state = PlaybackState::Ended;
        eof.end_reason = Some(EndReason::Eof);
        assert!(
            handle_synchronized_terminal::<RecordingPort>(
                &mut eof_state,
                &mut ui,
                &eof,
            )
            .is_some()
        );
        assert!(
            handle_synchronized_terminal::<RecordingPort>(
                &mut eof_state,
                &mut ui,
                &eof,
            )
            .is_some()
        );
        assert_eq!(
            RECORDS
                .lock()
                .unwrap()
                .iter()
                .filter(|message| matches!(
                    message,
                    RecordedMessage::Playback(name) if name == "EndOfStream"
                ))
                .count(),
            1
        );

        RECORDS.lock().unwrap().clear();
        let mut failed_state = PlayerDomainState {
            current_media_id: Some(media_id),
            last_valid_duration: 100.0,
            ..PlayerDomainState::default()
        };
        let mut failed = PlaybackSnapshot::new(
            crate::contract::SessionGeneration::new(12),
            crate::contract::PlaybackTarget::GSTREAMER_EMBEDDED,
            crate::contract::PlaybackCapabilities::default(),
        );
        failed.state = PlaybackState::Failed;
        failed.position = Duration::from_millis(12_500);
        failed.duration = Some(Duration::from_secs(100));
        failed.last_error = Some(crate::contract::PlaybackError::new(
            crate::contract::PlaybackErrorKind::Unknown,
            "backend failed safely",
        ));

        assert!(
            handle_synchronized_terminal::<RecordingPort>(
                &mut failed_state,
                &mut ui,
                &failed,
            )
            .is_some()
        );
        assert!(
            RECORDS
                .lock()
                .unwrap()
                .contains(&RecordedMessage::Progress(media_id, 12.5, 100.0))
        );
        assert_eq!(ui.error.as_deref(), Some("Unknown: backend failed safely"));
    }

    #[test]
    fn drag_seek_preview_uses_a_deterministic_dispatch_interval() {
        let first = Instant::now();

        assert!(drag_seek_is_due(None, first));
        assert!(!drag_seek_is_due(
            Some(first),
            first + crate::constants::seeking::SEEK_DRAG_THROTTLE
                - Duration::from_millis(1),
        ));
        assert!(drag_seek_is_due(
            Some(first),
            first + crate::constants::seeking::SEEK_DRAG_THROTTLE,
        ));
    }

    #[tokio::test]
    async fn resolve_playback_stream_source_uses_a_bearer_header() {
        let api = TestApiService::new("https://ferrex.example");
        api.set_playback_ticket("ticket secret/with symbols");

        let source = resolve_playback_stream_source(
            Arc::new(api),
            "https://ferrex.example/".to_string(),
            "media file".to_string(),
        )
        .await
        .expect("ticket resolution succeeds");

        assert_eq!(
            source.uri().as_str(),
            "https://ferrex.example/api/v1/stream/media%20file"
        );
        assert!(source.uri().query().is_none());
        assert_eq!(source.headers().len(), 1);
        assert_eq!(source.headers()[0].name, "Authorization");
        assert_eq!(
            source.headers()[0].value.expose_secret(),
            "Bearer ticket secret/with symbols"
        );
        assert!(!format!("{source:?}").contains("ticket secret"));

        let external_url = external_mpv_url(&source);
        let external_url = url::Url::parse(external_url.as_str()).unwrap();
        assert_eq!(
            external_url
                .query_pairs()
                .find(|(name, _)| name == "access_token")
                .map(|(_, value)| value.into_owned())
                .as_deref(),
            Some("ticket secret/with symbols")
        );
        assert!(source.uri().query().is_none());
    }

    #[tokio::test]
    async fn resolve_playback_stream_source_fails_closed_on_ticket_error() {
        let api = TestApiService::new("https://ferrex.example");
        api.set_playback_ticket_error("Unauthorized - please login again");

        let error = resolve_playback_stream_source(
            Arc::new(api),
            "https://ferrex.example".to_string(),
            "media-file".to_string(),
        )
        .await
        .expect_err("ticket failure must not return a bare stream source");

        assert!(error.contains("Sign in again"));
        assert!(!error.contains("/api/v1/stream/media-file"));
    }

    #[tokio::test]
    async fn resolve_playback_stream_source_rejects_header_injection() {
        let api = TestApiService::new("https://ferrex.example");
        api.set_playback_ticket("ticket\r\nX-Injected: secret");

        let error = resolve_playback_stream_source(
            Arc::new(api),
            "https://ferrex.example".to_string(),
            "media-file".to_string(),
        )
        .await
        .expect_err("control characters must not enter a playback header");

        assert!(error.contains("Could not authorize playback"));
        assert!(!error.contains("ticket"));
        assert!(!error.contains("X-Injected"));
        assert!(!error.contains("secret"));
    }
}
