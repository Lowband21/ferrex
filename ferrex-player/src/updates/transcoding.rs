use crate::{
    messages::{streaming::Message, CrossDomainEvent},
    state::{State, ViewState},
};
use iced::Task;

/// Handle transcoding started event
pub fn handle_transcoding_started(
    state: &mut State,
    result: Result<String, String>,
) -> Task<Message> {
    match result {
        Ok(job_id) => {
            log::info!("Transcoding started successfully with job ID: {}", job_id);

            // Check if this is a cached response
            if job_id.starts_with("cached_") {
                log::info!("Media is already cached, marking as ready immediately");
                state.player.transcoding_job_id = None; // No job to track
                state.player.transcoding_status =
                    Some(crate::player::state::TranscodingStatus::Completed);

                // Emit cross-domain event to load video
                if state.player.video_opt.is_none() && state.player.using_hls {
                    return Task::done(Message::_EmitCrossDomainEvent(
                        CrossDomainEvent::VideoReadyToPlay,
                    ));
                } else {
                    return Task::none();
                }
            }

            // Normal transcoding job
            state.player.transcoding_job_id = Some(job_id);
            state.player.transcoding_status =
                Some(crate::player::state::TranscodingStatus::Processing { progress: 0.0 });
            state.player.transcoding_check_count = 0; // Reset check count

            // Start checking status immediately
            Task::perform(async {}, |_| Message::CheckTranscodingStatus)
        }
        Err(e) => {
            log::error!("Failed to start transcoding: {}", e);
            state.player.transcoding_status =
                Some(crate::player::state::TranscodingStatus::Failed { error: e.clone() });

            // Show error to user
            state.error_message = Some(format!("Transcoding failed: {}", e));

            Task::none()
        }
    }
}

/// Check transcoding status periodically
pub fn handle_check_transcoding_status(state: &State) -> Task<Message> {
    if let Some(job_id) = &state.player.transcoding_job_id {
        if let Some(client) = &state.player.hls_client {
            let client_clone = client.clone();
            let job_id_clone = job_id.clone();

            Task::perform(
                async move {
                    match client_clone.check_transcoding_status(&job_id_clone).await {
                        Ok(job) => {
                            // Status is already deserialized from the shared enum
                            let status = job.status.clone();

                            // Use duration from job if available
                            let duration = job.duration;

                            // Log job details for debugging
                            log::info!(
                                "Transcoding job details: id={}, media_id={}, playlist_path={:?}",
                                job.id,
                                job.media_id,
                                job.playlist_path
                            );

                            // Log progress details if processing
                            let playlist_path = match &status {
                                ferrex_core::TranscodingStatus::Processing { progress } => {
                                    if let Some(details) = &job.progress_details {
                                        log::info!(
                                            "Transcoding progress: {:.1}%, FPS: {:.0}, ETA: {:.0}s",
                                            details.percentage,
                                            details.current_fps.unwrap_or(0.0),
                                            details.estimated_time_remaining.unwrap_or(0.0)
                                        );
                                    } else {
                                        log::info!(
                                            "Transcoding progress: {:.5}%",
                                            progress * 100.0
                                        );
                                        log::info!("Raw transcoding progress: {}%", progress);
                                    }
                                    None
                                }
                                ferrex_core::TranscodingStatus::Failed { error } => {
                                    log::error!("Transcoding failed: {}", error);
                                    None
                                }
                                ferrex_core::TranscodingStatus::Pending => {
                                    log::info!("Transcoding is pending");
                                    None
                                }
                                ferrex_core::TranscodingStatus::Queued => {
                                    log::info!("Transcoding is queued");
                                    None
                                }
                                ferrex_core::TranscodingStatus::Cancelled => {
                                    log::warn!("Transcoding was cancelled");
                                    None
                                }
                                ferrex_core::TranscodingStatus::Completed => {
                                    job.playlist_path.clone()
                                }
                            };

                            let status_converted = match status {
                                ferrex_core::TranscodingStatus::Pending => {
                                    crate::player::state::TranscodingStatus::Pending
                                }
                                ferrex_core::TranscodingStatus::Queued => {
                                    crate::player::state::TranscodingStatus::Queued
                                }
                                ferrex_core::TranscodingStatus::Processing { progress } => {
                                    crate::player::state::TranscodingStatus::Processing { progress }
                                }
                                ferrex_core::TranscodingStatus::Completed => {
                                    crate::player::state::TranscodingStatus::Completed
                                }
                                ferrex_core::TranscodingStatus::Failed { error } => {
                                    crate::player::state::TranscodingStatus::Failed { error }
                                }
                                ferrex_core::TranscodingStatus::Cancelled => {
                                    crate::player::state::TranscodingStatus::Cancelled
                                }
                            };
                            Ok((status_converted, duration, playlist_path))
                        }
                        Err(e) => Err(e),
                    }
                },
                Message::TranscodingStatusUpdate,
            )
        } else {
            Task::none()
        }
    } else {
        Task::none()
    }
}

/// Handle transcoding status update
pub fn handle_transcoding_status_update(
    state: &mut State,
    result: Result<
        (
            crate::player::state::TranscodingStatus,
            Option<f64>,
            Option<String>,
        ),
        String,
    >,
) -> Task<Message> {
    match result {
        Ok((status, duration, playlist_path)) => {
            let should_continue_checking = match &status {
                crate::player::state::TranscodingStatus::Pending
                | crate::player::state::TranscodingStatus::Queued => true,
                crate::player::state::TranscodingStatus::Processing { progress } => {
                    // For HLS, we can start playback once we have enough segments
                    // Continue checking if video not loaded yet or progress < 100%
                    state.player.video_opt.is_none() || *progress < 1.0
                }
                _ => false,
            };

            state.player.transcoding_status = Some(status.clone());

            // Store duration from transcoding job if available and valid
            if let Some(dur) = duration {
                if dur > 0.0 && dur.is_finite() {
                    state.player.transcoding_duration = Some(dur);

                    // Store source duration separately - this is the full media duration
                    if state.player.source_duration.is_none() {
                        state.player.source_duration = Some(dur);
                        log::info!(
                            "Stored source duration: {} seconds ({:.1} minutes)",
                            dur,
                            dur / 60.0
                        );
                    }

                    // Update player duration if video is already loaded but had no duration
                    if state.player.duration <= 0.0 && state.player.video_opt.is_some() {
                        state.player.duration = dur;
                        log::info!("Updated player duration from transcoding job");
                    }
                } else {
                    log::warn!("Invalid duration from transcoding job: {:?}", dur);
                }
            }

            // Update playlist URL if provided (when transcoding is ready)
            if let Some(playlist_path) = playlist_path {
                let playlist_url = if playlist_path.starts_with("http") {
                    playlist_path
                } else {
                    format!("{}{}", state.server_url, playlist_path)
                };
                log::info!("Updating playlist URL from job: {}", playlist_url);

                // Update the URL to the actual playlist path
                if let Ok(url) = url::Url::parse(&playlist_url) {
                    state.player.current_url = Some(url);
                }
            }

            // Increment check count
            state.player.transcoding_check_count += 1;

            // If we've checked too many times (30 checks = ~1 minute), give up and load video
            if state.player.transcoding_check_count > 30 {
                log::warn!("Transcoding status checks exceeded limit - loading video anyway");
                state.player.transcoding_status =
                    Some(crate::player::state::TranscodingStatus::Completed);
                state.player.transcoding_job_id = None;

                if state.player.video_opt.is_none() && state.player.using_hls {
                    return Task::done(Message::_EmitCrossDomainEvent(
                        CrossDomainEvent::VideoReadyToPlay,
                    ));
                } else {
                    return Task::none();
                }
            }

            // For HLS streaming, try to start playback during processing if we have segments
            let should_try_playback = match &status {
                crate::player::state::TranscodingStatus::Processing { progress } => {
                    // Start playback when we have at least 1% transcoded (ensures initial segments exist)
                    // With 4-second segments, 2 segments = 8 seconds, which is <1% of most videos
                    *progress >= 0.01 && state.player.video_opt.is_none() && state.player.using_hls
                }
                crate::player::state::TranscodingStatus::Completed => {
                    // Also try when completed if not already playing
                    state.player.video_opt.is_none() && state.player.using_hls
                }
                _ => false,
            };

            let mut tasks = Vec::new();

            if should_continue_checking {
                // Continue checking every 2 seconds
                tasks.push(Task::perform(
                    async {
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    },
                    |_| Message::CheckTranscodingStatus,
                ));
            }

            if should_try_playback {
                log::info!("Attempting to start HLS playback (status: {:?})...", status);
                // Load video now that we have segments ready
                if state.player.video_opt.is_none() && state.player.using_hls {
                    // First check if master playlist exists before trying to load
                    let check_playlist_task = if let Some(media) = &state.player.current_media {
                        if let Some(client) = &state.player.hls_client {
                            let client_clone = client.clone();
                            let media_id = media.id.clone();

                            Some(Task::perform(
                                async move {
                                    // Small delay to ensure playlist files are written
                                    tokio::time::sleep(tokio::time::Duration::from_millis(100))
                                        .await;

                                    match client_clone.fetch_master_playlist(&media_id).await {
                                        Ok(playlist) => {
                                            log::info!(
                                                "Master playlist fetched with {} variants",
                                                playlist.variants.len()
                                            );
                                            Some(playlist)
                                        }
                                        Err(e) => {
                                            log::error!("Failed to fetch master playlist: {}", e);
                                            None
                                        }
                                    }
                                },
                                |playlist| Message::MasterPlaylistReady(playlist),
                            ))
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    // Only check for playlist first, don't load video yet
                    if let Some(playlist_task) = check_playlist_task {
                        tasks.push(playlist_task);
                    } else {
                        // Fallback to emit cross-domain event
                        tasks.push(Task::done(Message::_EmitCrossDomainEvent(
                            CrossDomainEvent::VideoReadyToPlay,
                        )));
                    }
                }
            }

            // Handle transcoding failures
            match &status {
                crate::player::state::TranscodingStatus::Failed { error } => {
                    log::error!("Transcoding failed: {}", error);
                    state.error_message = Some(format!("Transcoding failed: {}", error));
                    state.view = ViewState::VideoError {
                        message: format!("Transcoding failed: {}", error),
                    };
                }
                crate::player::state::TranscodingStatus::Cancelled => {
                    log::warn!("Transcoding was cancelled");
                    state.error_message = Some("Transcoding was cancelled".to_string());
                }
                _ => {}
            }

            // Return appropriate task
            if tasks.is_empty() {
                Task::none()
            } else if tasks.len() == 1 {
                tasks.into_iter().next().unwrap()
            } else {
                Task::batch(tasks)
            }
        }
        Err(e) => {
            log::warn!("Failed to check transcoding status: {}", e);

            // Special handling for "Job not found" - the job might have completed or expired
            if e.contains("Job not found") || e.contains("not found") {
                log::info!("Transcoding job not found - this could mean the job completed or the master playlist is ready");

                // For adaptive streaming, check if the master playlist exists
                if state.player.using_hls && state.player.video_opt.is_none() {
                    if let Some(ref media) = state.player.current_media {
                        log::info!(
                            "Checking if master playlist is available for media {}",
                            media.id
                        );

                        // Try to load the video directly - if the playlist exists, it will work
                        state.player.transcoding_status =
                            Some(crate::player::state::TranscodingStatus::Completed);
                        state.player.transcoding_job_id = None;

                        // Load video and fetch master playlist
                        let fetch_playlist_task = if let Some(client) = &state.player.hls_client {
                            let client_clone = client.clone();
                            let media_id = media.id.clone();

                            Some(Task::perform(
                                async move {
                                    match client_clone.fetch_master_playlist(&media_id).await {
                                        Ok(playlist) => {
                                            log::info!(
                                                "Master playlist fetched with {} variants",
                                                playlist.variants.len()
                                            );
                                            Some(playlist)
                                        }
                                        Err(e) => {
                                            log::error!("Failed to fetch master playlist: {}", e);
                                            None
                                        }
                                    }
                                },
                                |playlist| Message::MasterPlaylistLoaded(playlist),
                            ))
                        } else {
                            None
                        };

                        let video_ready_task = Task::done(Message::_EmitCrossDomainEvent(
                            CrossDomainEvent::VideoReadyToPlay,
                        ));

                        if let Some(playlist_task) = fetch_playlist_task {
                            Task::batch([video_ready_task, playlist_task])
                        } else {
                            video_ready_task
                        }
                    } else {
                        Task::none()
                    }
                } else {
                    Task::none()
                }
            } else {
                // Other errors - retry after 5 seconds
                Task::perform(
                    async {
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    },
                    |_| Message::CheckTranscodingStatus,
                )
            }
        }
    }
}
