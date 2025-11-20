use crate::{
    common::messages::{CrossDomainEvent, DomainMessage, DomainUpdateResult},
    domains::streaming::messages::Message,
    domains::ui::types::ViewState,
    state_refactored::State,
};
use ferrex_core::player_prelude::TranscodingStatus;
use iced::Task;

/// Handle transcoding started event
pub fn handle_transcoding_started(
    state: &mut State,
    result: Result<String, String>,
) -> DomainUpdateResult {
    match result {
        Ok(job_id) => {
            log::info!(
                "Transcoding started successfully with job ID: {}",
                job_id
            );

            // Check if this is a cached response
            if job_id.starts_with("cached_") {
                log::info!(
                    "Media is already cached, marking as ready immediately"
                );
                state.domains.streaming.state.transcoding_job_id = None; // No job to track
                state.domains.streaming.state.transcoding_status =
                    Some(TranscodingStatus::Completed);

                // Send direct message to Player domain to load video
                if state.domains.player.state.video_opt.is_none()
                    && state.domains.streaming.state.using_hls
                {
                    return DomainUpdateResult::task(Task::done(
                        crate::common::messages::DomainMessage::Player(
                            crate::domains::player::messages::Message::VideoReadyToPlay,
                        ),
                    ));
                } else {
                    return DomainUpdateResult::task(Task::none());
                }
            }

            // Normal transcoding job
            state.domains.streaming.state.transcoding_job_id = Some(job_id);
            state.domains.streaming.state.transcoding_status =
                Some(TranscodingStatus::Processing { progress: 0.0 });
            state.domains.streaming.state.transcoding_check_count = 0; // Reset check count

            // Start checking status immediately
            DomainUpdateResult::task(Task::perform(async {}, |_| {
                DomainMessage::Streaming(Message::CheckTranscodingStatus)
            }))
        }
        Err(e) => {
            log::error!("Failed to start transcoding: {}", e);
            state.domains.streaming.state.transcoding_status =
                Some(TranscodingStatus::Failed { error: e.clone() });

            // Show error to user
            state.domains.ui.state.error_message =
                Some(format!("Transcoding failed: {}", e));

            DomainUpdateResult::task(Task::none())
        }
    }
}

/// Check transcoding status periodically
pub fn handle_check_transcoding_status(state: &State) -> DomainUpdateResult {
    if let Some(job_id) = &state.domains.streaming.state.transcoding_job_id {
        // Use trait-based streaming service (RUS-136: removed dual-path fallback)
        let service = state.domains.streaming.state.streaming_service.clone();
        let job_id_clone = job_id.clone();
        DomainUpdateResult::task(Task::perform(
            async move {
                // Map service result into legacy tuple expected downstream
                match service.check_transcoding_status(&job_id_clone).await {
                    Ok(status) => {
                        let converted = match status.state.as_str() {
                            "pending" => TranscodingStatus::Pending,
                            "queued" => TranscodingStatus::Queued,
                            "completed" => TranscodingStatus::Completed,
                            "failed" => TranscodingStatus::Failed {
                                error: status.message.unwrap_or_else(|| {
                                    "Unknown error".to_string()
                                }),
                            },
                            _ => TranscodingStatus::Processing {
                                progress: status.progress.unwrap_or(0.0),
                            },
                        };
                        Ok((converted, None, None))
                    }
                    Err(e) => Err(e.to_string()),
                }
            },
            |result| {
                DomainMessage::Streaming(Message::TranscodingStatusUpdate(
                    result,
                ))
            },
        ))
    } else {
        DomainUpdateResult::task(Task::none())
    }
}

/// Handle transcoding status update
pub fn handle_transcoding_status_update(
    state: &mut State,
    result: Result<(TranscodingStatus, Option<f64>, Option<String>), String>,
) -> DomainUpdateResult {
    match result {
        Ok((status, duration, playlist_path)) => {
            let should_continue_checking = match &status {
                TranscodingStatus::Pending | TranscodingStatus::Queued => true,
                TranscodingStatus::Processing { progress } => {
                    // For HLS, we can start playback once we have enough segments
                    // Continue checking if video not loaded yet or progress < 100%
                    state.domains.player.state.video_opt.is_none()
                        || *progress < 1.0
                }
                _ => false,
            };

            state.domains.streaming.state.transcoding_status =
                Some(status.clone());

            // Store duration from transcoding job if available and valid
            if let Some(dur) = duration {
                if dur > 0.0 && dur.is_finite() {
                    state.domains.streaming.state.transcoding_duration =
                        Some(dur);

                    // Store source duration separately - this is the full media duration
                    if state.domains.player.state.source_duration.is_none() {
                        state.domains.player.state.source_duration = Some(dur);
                        log::info!(
                            "Stored source duration: {} seconds ({:.1} minutes)",
                            dur,
                            dur / 60.0
                        );
                    }

                    // Update player duration if video is already loaded but had no duration
                    if state.domains.player.state.last_valid_duration <= 0.0
                        && state.domains.player.state.video_opt.is_some()
                    {
                        state.domains.player.state.last_valid_duration = dur;
                        log::info!(
                            "Updated player duration from transcoding job"
                        );
                    }
                } else {
                    log::warn!(
                        "Invalid duration from transcoding job: {:?}",
                        dur
                    );
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
                    state.domains.player.state.current_url = Some(url);
                }
            }

            // Increment check count
            state.domains.streaming.state.transcoding_check_count += 1;

            // If we've checked too many times (30 checks = ~1 minute), give up and load video
            if state.domains.streaming.state.transcoding_check_count > 30 {
                log::warn!(
                    "Transcoding status checks exceeded limit - loading video anyway"
                );
                state.domains.streaming.state.transcoding_status =
                    Some(TranscodingStatus::Completed);
                state.domains.streaming.state.transcoding_job_id = None;

                if state.domains.player.state.video_opt.is_none()
                    && state.domains.streaming.state.using_hls
                {
                    return DomainUpdateResult::task(Task::done(
                        crate::common::messages::DomainMessage::Player(
                            crate::domains::player::messages::Message::VideoReadyToPlay,
                        ),
                    ));
                } else {
                    return DomainUpdateResult::task(Task::none());
                }
            }

            // For HLS streaming, try to start playback during processing if we have segments
            let should_try_playback = match &status {
                TranscodingStatus::Processing { progress } => {
                    // Start playback when we have at least 1% transcoded (ensures initial segments exist)
                    // With 4-second segments, 2 segments = 8 seconds, which is <1% of most videos
                    *progress >= 0.01
                        && state.domains.player.state.video_opt.is_none()
                        && state.domains.streaming.state.using_hls
                }
                TranscodingStatus::Completed => {
                    // Also try when completed if not already playing
                    state.domains.player.state.video_opt.is_none()
                        && state.domains.streaming.state.using_hls
                }
                _ => false,
            };

            let mut tasks: Vec<Task<DomainMessage>> = Vec::new();
            let events: Vec<CrossDomainEvent> = Vec::new();

            if should_continue_checking {
                // Continue checking every 2 seconds
                tasks.push(Task::perform(
                    async {
                        tokio::time::sleep(tokio::time::Duration::from_secs(2))
                            .await;
                    },
                    |_| {
                        DomainMessage::Streaming(
                            Message::CheckTranscodingStatus,
                        )
                    },
                ));
            }

            if should_try_playback {
                log::info!(
                    "Attempting to start HLS playback (status: {:?})...",
                    status
                );
                // Load video now that we have segments ready
                if state.domains.player.state.video_opt.is_none()
                    && state.domains.streaming.state.using_hls
                {
                    // First check if master playlist exists before trying to load
                    let check_playlist_task = if let Some(media) =
                        &state.domains.player.state.current_media
                    {
                        if let Some(client) =
                            &state.domains.streaming.state.hls_client
                        {
                            let client_clone = client.clone();
                            let media_id = media.id;

                            Some(Task::perform(
                                async move {
                                    // Small delay to ensure playlist files are written
                                    tokio::time::sleep(
                                        tokio::time::Duration::from_millis(100),
                                    )
                                    .await;

                                    match client_clone
                                        .fetch_master_playlist(&media_id)
                                        .await
                                    {
                                        Ok(playlist) => {
                                            log::info!(
                                                "Master playlist fetched with {} variants",
                                                playlist.variants.len()
                                            );
                                            Some(playlist)
                                        }
                                        Err(e) => {
                                            log::error!(
                                                "Failed to fetch master playlist: {}",
                                                e
                                            );
                                            None
                                        }
                                    }
                                },
                                |playlist| {
                                    DomainMessage::Streaming(
                                        Message::MasterPlaylistReady(playlist),
                                    )
                                },
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
                        // Send direct message to Player domain
                        tasks.push(Task::done(crate::common::messages::DomainMessage::Player(
                            crate::domains::player::messages::Message::VideoReadyToPlay,
                        )));
                    }
                }
            }

            // Handle transcoding failures
            match &status {
                TranscodingStatus::Failed { error } => {
                    log::error!("Transcoding failed: {}", error);
                    state.domains.ui.state.error_message =
                        Some(format!("Transcoding failed: {}", error));
                    state.domains.ui.state.view = ViewState::VideoError {
                        message: format!("Transcoding failed: {}", error),
                    };
                }
                TranscodingStatus::Cancelled => {
                    log::warn!("Transcoding was cancelled");
                    state.domains.ui.state.error_message =
                        Some("Transcoding was cancelled".to_string());
                }
                _ => {}
            }

            // Return appropriate task and events
            let task = if tasks.is_empty() {
                Task::none()
            } else if tasks.len() == 1 {
                tasks.into_iter().next().unwrap()
            } else {
                Task::batch(tasks)
            };

            if events.is_empty() {
                DomainUpdateResult::task(task)
            } else {
                DomainUpdateResult::with_events(task, events)
            }
        }
        Err(e) => {
            log::warn!("Failed to check transcoding status: {}", e);

            // Special handling for "Job not found" - the job might have completed or expired
            if e.contains("Job not found") || e.contains("not found") {
                log::info!(
                    "Transcoding job not found - this could mean the job completed or the master playlist is ready"
                );

                // For adaptive streaming, check if the master playlist exists
                if state.domains.streaming.state.using_hls
                    && state.domains.player.state.video_opt.is_none()
                {
                    if let Some(ref media) =
                        state.domains.player.state.current_media
                    {
                        log::info!(
                            "Checking if master playlist is available for media {}",
                            media.id
                        );

                        // Try to load the video directly - if the playlist exists, it will work
                        state.domains.streaming.state.transcoding_status =
                            Some(TranscodingStatus::Completed);
                        state.domains.streaming.state.transcoding_job_id = None;

                        // Load video and fetch master playlist
                        let fetch_playlist_task = if let Some(client) =
                            &state.domains.streaming.state.hls_client
                        {
                            let client_clone = client.clone();
                            let media_id = media.id;

                            Some(Task::perform(
                                async move {
                                    match client_clone
                                        .fetch_master_playlist(&media_id)
                                        .await
                                    {
                                        Ok(playlist) => {
                                            log::info!(
                                                "Master playlist fetched with {} variants",
                                                playlist.variants.len()
                                            );
                                            Some(playlist)
                                        }
                                        Err(e) => {
                                            log::error!(
                                                "Failed to fetch master playlist: {}",
                                                e
                                            );
                                            None
                                        }
                                    }
                                },
                                |playlist| {
                                    DomainMessage::Streaming(
                                        Message::MasterPlaylistLoaded(playlist),
                                    )
                                },
                            ))
                        } else {
                            None
                        };

                        if let Some(playlist_task) = fetch_playlist_task {
                            DomainUpdateResult::task(Task::batch(vec![
                                playlist_task,
                                Task::done(crate::common::messages::DomainMessage::Player(
                                    crate::domains::player::messages::Message::VideoReadyToPlay,
                                )),
                            ]))
                        } else {
                            DomainUpdateResult::task(Task::done(
                                crate::common::messages::DomainMessage::Player(
                                    crate::domains::player::messages::Message::VideoReadyToPlay,
                                ),
                            ))
                        }
                    } else {
                        DomainUpdateResult::task(Task::none())
                    }
                } else {
                    DomainUpdateResult::task(Task::none())
                }
            } else {
                // Other errors - retry after 5 seconds
                DomainUpdateResult::task(Task::perform(
                    async {
                        tokio::time::sleep(tokio::time::Duration::from_secs(5))
                            .await;
                    },
                    |_| {
                        DomainMessage::Streaming(
                            Message::CheckTranscodingStatus,
                        )
                    },
                ))
            }
        }
    }
}
