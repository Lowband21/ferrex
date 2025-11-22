use crate::{
    common::messages::{CrossDomainEvent, DomainMessage, DomainUpdateResult},
    domains::ui::{messages::UiMessage, playback_ui::PlaybackMessage},
    state::State,
};
use ferrex_core::player_prelude::{
    EpisodeID, EpisodeLike, Media, MediaID, MovieLike,
};
use iced::Task;

pub fn update_playback_ui(
    state: &mut State,
    message: PlaybackMessage,
) -> DomainUpdateResult {
    match message {
        PlaybackMessage::PlayMediaWithId(media_id) => {
            match state.domains.ui.state.repo_accessor.get(&media_id) {
                Ok(media) => match media {
                    Media::Movie(movie) => DomainUpdateResult::with_events(
                        Task::none(),
                        vec![CrossDomainEvent::MediaPlayWithId(
                            movie.file(),
                            media_id,
                        )],
                    ),
                    Media::Episode(episode) => DomainUpdateResult::with_events(
                        Task::none(),
                        vec![CrossDomainEvent::MediaPlayWithId(
                            episode.file(),
                            media_id,
                        )],
                    ),
                    _ => {
                        log::error!("Media not playable type {}", media_id);
                        DomainUpdateResult::task(Task::none())
                    }
                },
                Err(_) => {
                    log::error!("Failed to get media with id {}", media_id);
                    DomainUpdateResult::task(Task::none())
                }
            }
        }
        PlaybackMessage::PlayMediaWithIdInMpv(media_id) => {
            match state.domains.ui.state.repo_accessor.get(&media_id) {
                Ok(media) => {
                    // Extract the concrete media file for playback
                    let media_file = match media {
                        Media::Movie(movie) => movie.file(),
                        Media::Episode(episode) => episode.file(),
                        _ => {
                            log::error!("Media not playable type {}", media_id);
                            return DomainUpdateResult::task(Task::none());
                        }
                    };

                    // Seed resume/duration hints similarly to CrossDomainEvent::MediaPlayWithId
                    let mut resume_opt: Option<f32> = None;
                    let mut watch_duration_hint: Option<f64> = None;
                    if let Some(watch_state) =
                        &state.domains.media.state.user_watch_state
                        && let Some(item) =
                            watch_state.get_by_media_id(media_id.as_uuid())
                    {
                        if item.position > 0.0 && item.duration > 0.0 {
                            resume_opt = Some(item.position);
                        }
                        if item.duration > 0.0 {
                            watch_duration_hint = Some(item.duration as f64);
                        }
                    }

                    let metadata_duration_hint = media_file
                        .media_file_metadata
                        .as_ref()
                        .and_then(|meta| meta.duration)
                        .filter(|d| *d > 0.0);

                    let duration_hint =
                        watch_duration_hint.or(metadata_duration_hint);

                    state.domains.player.state.last_valid_position =
                        resume_opt.map(|pos| pos as f64).unwrap_or(0.0);
                    state.domains.player.state.last_valid_duration =
                        duration_hint.unwrap_or(0.0);
                    state.domains.media.state.pending_resume_position =
                        resume_opt;
                    state.domains.player.state.pending_resume_position =
                        resume_opt;

                    // First seed the player with PlayMediaWithId, then switch to external player
                    let tasks = Task::batch(vec![
                        Task::done(DomainMessage::Player(
                            crate::domains::player::messages::PlayerMessage::PlayMediaWithId(
                                media_file,
                                media_id,
                            ),
                        )),
                        Task::done(DomainMessage::Player(
                            crate::domains::player::messages::PlayerMessage::PlayExternal,
                        )),
                    ]);

                    DomainUpdateResult::task(tasks)
                }
                Err(_) => {
                    log::error!("Failed to get media with id {}", media_id);
                    DomainUpdateResult::task(Task::none())
                }
            }
        }
        PlaybackMessage::PlaySeriesNextEpisode(series_id) => {
            // Prefer identity-based next-episode from server, fall back to local selection
            let fallback_next =
                crate::domains::media::selectors::select_next_episode_for_series(
                    state, series_id,
                );

            // Resolve TMDB series id from repository (SeriesReference)
            let tmdb_series_id = match state
                .domains
                .ui
                .state
                .repo_accessor
                .get(&MediaID::Series(series_id))
            {
                Ok(Media::Series(series)) => Some(series.tmdb_id),
                _ => None,
            };

            // If we have an API service and tmdb id, defer to server
            if let (Some(api), Some(tmdb_id)) = (
                state.domains.media.state.api_service.clone(),
                tmdb_series_id,
            ) {
                let task = Task::perform(
                    async move { api.get_series_next_episode(tmdb_id).await },
                    move |result| match result {
                        Ok(Some(next)) => {
                            if let Some(playable) = next.playable_media_id {
                                DomainMessage::Ui(
                                    PlaybackMessage::PlayMediaWithId(
                                        MediaID::Episode(EpisodeID(playable)),
                                    )
                                    .into(),
                                )
                            } else if let Some(fid) = fallback_next {
                                DomainMessage::Ui(
                                    PlaybackMessage::PlayMediaWithId(
                                        MediaID::Episode(fid),
                                    )
                                    .into(),
                                )
                            } else {
                                DomainMessage::Ui(UiMessage::NoOp)
                            }
                        }
                        _ => {
                            if let Some(fid) = fallback_next {
                                DomainMessage::Ui(
                                    PlaybackMessage::PlayMediaWithId(
                                        MediaID::Episode(fid),
                                    )
                                    .into(),
                                )
                            } else {
                                DomainMessage::Ui(UiMessage::NoOp)
                            }
                        }
                    },
                );
                DomainUpdateResult::task(task)
            } else {
                // No API or TMDB id -> use local selection immediately
                if let Some(fid) = fallback_next {
                    DomainUpdateResult::task(Task::done(DomainMessage::Ui(
                        PlaybackMessage::PlayMediaWithId(MediaID::Episode(fid))
                            .into(),
                    )))
                } else {
                    DomainUpdateResult::task(Task::none())
                }
            }
        }
    }
}
