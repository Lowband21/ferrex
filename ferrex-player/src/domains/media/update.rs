use super::messages::MediaMessage;
use crate::{
    common::messages::{DomainMessage, DomainUpdateResult},
    state::State,
};
use ferrex_core::player_prelude::{
    EpisodeKey, MediaID, MediaIDLike, UpdateProgressRequest, UserWatchState,
};
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
pub fn update_media(
    state: &mut State,
    message: MediaMessage,
) -> DomainUpdateResult {
    match message {
        MediaMessage::Noop => {
            // No-op message, used for task chaining
            DomainUpdateResult::task(Task::none())
        }

        MediaMessage::WatchProgressFetched(_media_id, _resume_position) => {
            // This message is for future use when we fetch watch progress asynchronously
            // Currently handled synchronously in PlayMediaWithId
            DomainUpdateResult::task(Task::none())
        }

        /* TODO: Reimplement this in player domain
        Message::PlayNextEpisode => {
            // Check if current media is an episode and play the next one
            if let Some(current_media_id) = &state.domains.media.state.current_media_id {
                if let MediaID::Episode(episode_id) = current_media_id {
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
                        let media_id = MediaID::Episode(next_episode.id.clone());

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
            }
            DomainUpdateResult::task(Task::none())
        }*/
        // Handle watch progress tracking
        MediaMessage::ProgressUpdateSent(media_id, position, duration) => {
            // Update the last sent position
            state.domains.player.state.last_progress_sent = position;
            state.domains.player.state.last_progress_update =
                Some(std::time::Instant::now());

            // Update local watch state to reflect in UI immediately
            let should_refresh_ui = {
                if duration <= 0.0 {
                    log::warn!(
                        "Skipping watch state update - invalid duration {:.1}s for {:?}",
                        duration,
                        media_id
                    );
                    false
                } else {
                    // Ensure we have a local watch state cache to update
                    if state.domains.media.state.user_watch_state.is_none() {
                        state.domains.media.state.user_watch_state =
                            Some(UserWatchState::new());
                    }

                    if let Some(watch_state) =
                        &mut state.domains.media.state.user_watch_state
                    {
                        let media_uuid = media_id.to_uuid();
                        let progress_ratio =
                            (position / duration).clamp(0.0, 1.0);
                        let reached_completion = progress_ratio >= 0.95;

                        let was_completed =
                            watch_state.completed.contains(media_id.as_uuid());
                        let was_in_progress = watch_state
                            .in_progress
                            .contains_key(media_id.as_uuid());
                        watch_state.update_progress(
                            media_uuid,
                            position as f32,
                            duration as f32,
                        );
                        log::info!(
                            "Updated local watch state for {:?}: {:.1}s/{:.1}s ({:.1}%)",
                            media_id,
                            position,
                            duration,
                            progress_ratio * 100.0
                        );

                        let is_completed =
                            watch_state.completed.contains(media_id.as_uuid());
                        let is_in_progress = watch_state
                            .in_progress
                            .contains_key(media_id.as_uuid());

                        // Debug: Check what's actually in the watch state now
                        if let Some(item) =
                            watch_state.in_progress.get(media_id.as_uuid())
                        {
                            log::debug!(
                                "Watch state verification - MediaID {:?} has position: {:.1}s, duration: {:.1}s",
                                media_id,
                                item.position,
                                item.duration
                            );
                        } else if watch_state
                            .completed
                            .contains(media_id.as_uuid())
                        {
                            log::debug!(
                                "Watch state verification - MediaID {:?} is marked as completed",
                                media_id
                            );
                        } else {
                            log::debug!(
                                "Watch state verification - MediaID {:?} currently unwatched",
                                media_id
                            );
                        }

                        // Refresh immediately when status categories change (eg. becomes completed or unwatched)
                        let bypass_debounce = (was_completed != is_completed)
                            || (was_in_progress != is_in_progress)
                            || reached_completion;

                        // Otherwise fall back to debounce window (max one refresh every 2 seconds)
                        let allow_debounce_refresh = if let Some(last_refresh) =
                            state
                                .domains
                                .media
                                .state
                                .last_ui_refresh_for_progress
                        {
                            last_refresh.elapsed()
                                > std::time::Duration::from_secs(2)
                        } else {
                            true
                        };

                        if bypass_debounce || allow_debounce_refresh {
                            state
                                .domains
                                .media
                                .state
                                .last_ui_refresh_for_progress =
                                Some(std::time::Instant::now());
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
            };

            // If watch state was updated and debounce allows, trigger a UI refresh
            if should_refresh_ui {
                log::debug!("Triggering UI refresh for watch progress update");
                // Use UpdateViewModelFilters for a lightweight refresh
                DomainUpdateResult::task(Task::done(DomainMessage::Ui(
                    crate::domains::ui::messages::UiMessage::UpdateViewModelFilters,
                )))
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }

        MediaMessage::ProgressUpdateFailed => {
            // Log was already done in subscription, just track the failure
            log::debug!("Progress update failed, will retry on next interval");
            DomainUpdateResult::task(Task::none())
        }

        MediaMessage::SendProgressUpdateWithData(
            media_id,
            position,
            duration,
        ) => {
            log::debug!(
                "SendProgressUpdateWithData: Starting progress update with captured data"
            );

            // Send an immediate progress update using the captured data
            if let Some(api_service) = &state.domains.media.state.api_service {
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
                            Ok(pos) => DomainMessage::Media(
                                MediaMessage::ProgressUpdateSent(
                                    media_id, pos, duration,
                                ),
                            ),
                            Err(e) => {
                                log::warn!(
                                    "Failed to send progress update: {}",
                                    e
                                );
                                DomainMessage::Media(
                                    MediaMessage::ProgressUpdateFailed,
                                )
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
            }
        }
    }
}
