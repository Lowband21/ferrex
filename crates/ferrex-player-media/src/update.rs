//! UI-agnostic media/watch-state reducer logic.
//!
//! Reducers mutate `MediaDomainState` and emit domain tasks while leaving
//! concrete UI effects to the app shell.

use crate::{MediaDomainState, messages::MediaMessage};
use ferrex_core::player_prelude::{
    EpisodeKey, MediaID, MediaIDLike, UpdateProgressRequest, UserWatchState,
};
use ferrex_player_foundation::domain::{DomainTask, DomainUpdateResult};

/// App-shell message factory for media update side effects.
pub trait MediaUpdatePort {
    type AppMessage: Send + 'static;

    fn media_message(message: MediaMessage) -> Self::AppMessage;
    fn refresh_view_model_filters() -> Self::AppMessage;
}

pub fn update_media<P>(
    state: &mut MediaDomainState,
    message: MediaMessage,
) -> DomainUpdateResult<DomainTask<P::AppMessage>, ()>
where
    P: MediaUpdatePort + 'static,
{
    match message {
        MediaMessage::Noop | MediaMessage::WatchProgressFetched(_, _) => {
            DomainUpdateResult::task(DomainTask::none())
        }
        MediaMessage::ProgressUpdateSent(media_id, position, duration) => {
            state.last_progress_sent = position;
            state.last_progress_update = Some(std::time::Instant::now());

            let should_refresh_ui = if duration <= 0.0 {
                log::warn!(
                    "Skipping watch state update - invalid duration {:.1}s for {:?}",
                    duration,
                    media_id
                );
                false
            } else {
                if state.user_watch_state.is_none() {
                    state.user_watch_state = Some(UserWatchState::new());
                }

                if let Some(watch_state) = &mut state.user_watch_state {
                    let media_uuid = media_id.to_uuid();
                    let progress_ratio = (position / duration).clamp(0.0, 1.0);
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

                    let is_completed =
                        watch_state.completed.contains(media_id.as_uuid());
                    let is_in_progress = watch_state
                        .in_progress
                        .contains_key(media_id.as_uuid());

                    let bypass_debounce = (was_completed != is_completed)
                        || (was_in_progress != is_in_progress)
                        || reached_completion;
                    let allow_debounce_refresh =
                        state.last_ui_refresh_for_progress.is_none_or(|last| {
                            last.elapsed() > std::time::Duration::from_secs(2)
                        });

                    if bypass_debounce || allow_debounce_refresh {
                        state.last_ui_refresh_for_progress =
                            Some(std::time::Instant::now());
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            };

            if should_refresh_ui {
                DomainUpdateResult::task(DomainTask::done(
                    P::refresh_view_model_filters(),
                ))
            } else {
                DomainUpdateResult::task(DomainTask::none())
            }
        }
        MediaMessage::ProgressUpdateFailed => {
            log::debug!("Progress update failed, will retry on next interval");
            DomainUpdateResult::task(DomainTask::none())
        }
        MediaMessage::SendProgressUpdateWithData(
            media_id,
            position,
            duration,
        ) => {
            if let Some(api_service) = &state.api_service
                && position > 0.0
                && duration > 0.0
            {
                let api_service = api_service.clone();

                let episode_key_opt: Option<EpisodeKey> = match media_id {
                    MediaID::Episode(ep_id) => match state
                        .repo_accessor
                        .get(&MediaID::Episode(ep_id))
                    {
                        Ok(ferrex_player_api::api_types::Media::Episode(
                            ep,
                        )) => Some(EpisodeKey {
                            tmdb_series_id: ep.tmdb_series_id,
                            season_number: ep.season_number.value(),
                            episode_number: ep.episode_number.value(),
                        }),
                        _ => None,
                    },
                    _ => None,
                };

                DomainUpdateResult::task(DomainTask::perform(
                    async move {
                        let request = UpdateProgressRequest {
                            media_id: media_id.to_uuid(),
                            media_type: media_id.media_type(),
                            position: position as f32,
                            duration: duration as f32,
                            episode: episode_key_opt,
                            last_media_uuid: Some(media_id.to_uuid()),
                        };
                        api_service
                            .update_progress(&request)
                            .await
                            .map(|_| position)
                    },
                    move |result| match result {
                        Ok(pos) => {
                            P::media_message(MediaMessage::ProgressUpdateSent(
                                media_id, pos, duration,
                            ))
                        }
                        Err(err) => {
                            log::warn!(
                                "Failed to send progress update: {}",
                                err
                            );
                            P::media_message(MediaMessage::ProgressUpdateFailed)
                        }
                    },
                ))
            } else {
                if position <= 0.0 || duration <= 0.0 {
                    log::warn!(
                        "Skipping progress update - invalid data: position={:.1}s, duration={:.1}s",
                        position,
                        duration
                    );
                }
                DomainUpdateResult::task(DomainTask::none())
            }
        }
    }
}
