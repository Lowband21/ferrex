use crate::{
    common::messages::{DomainMessage, DomainUpdateResult},
    domains::{
        player::messages::PlayerMessage,
        ui::{messages::UiMessage, playback_ui::PlaybackMessage},
    },
    state::State,
};
use ferrex_core::player_prelude::{
    EpisodeID, EpisodeLike, Media, MediaFile, MediaID, MovieLike,
};
use iced::Task;

fn seed_playback_hints(
    state: &mut State,
    media_file: &MediaFile,
    media_id: MediaID,
    forced_start: Option<f32>,
) {
    let mut resume_from_watch: Option<f32> = None;
    let mut watch_duration_hint: Option<f64> = None;

    if let Some(watch_state) = &state.domains.media.state.user_watch_state
        && let Some(item) = watch_state.get_by_media_id(media_id.as_uuid())
    {
        if item.position > 0.0 && item.duration > 0.0 {
            resume_from_watch = Some(item.position);
        }
        if item.duration > 0.0 {
            watch_duration_hint = Some(item.duration as f64);
        }
    }

    let resume_opt = forced_start.or(resume_from_watch);
    let metadata_duration_hint = media_file
        .media_file_metadata
        .as_ref()
        .and_then(|meta| meta.duration)
        .filter(|d| *d > 0.0);
    let duration_hint = metadata_duration_hint.or(watch_duration_hint);

    state.domains.player.state.last_valid_position =
        resume_opt.map(|pos| pos as f64).unwrap_or(0.0);
    state.domains.player.state.last_valid_duration =
        duration_hint.unwrap_or(0.0);
    state.domains.media.state.pending_resume_position = resume_opt;
    state.domains.player.state.pending_resume_position = resume_opt;
}

fn play_media_with_start(
    state: &mut State,
    media_id: MediaID,
    forced_start: Option<f32>,
    external: bool,
) -> DomainUpdateResult {
    match state.domains.ui.state.repo_accessor.get(&media_id) {
        Ok(media) => {
            let media_file = match media {
                Media::Movie(movie) => movie.file(),
                Media::Episode(episode) => episode.file(),
                _ => {
                    log::error!("Media not playable type {}", media_id);
                    return DomainUpdateResult::task(Task::none());
                }
            };

            seed_playback_hints(state, &media_file, media_id, forced_start);

            let play_task = Task::done(DomainMessage::Player(
                PlayerMessage::PlayMediaWithId(media_file, media_id),
            ));

            if external {
                DomainUpdateResult::task(Task::batch(vec![
                    play_task,
                    Task::done(DomainMessage::Player(
                        PlayerMessage::PlayExternal,
                    )),
                ]))
            } else {
                DomainUpdateResult::task(play_task)
            }
        }
        Err(_) => {
            log::error!("Failed to get media with id {}", media_id);
            DomainUpdateResult::task(Task::none())
        }
    }
}

pub fn update_playback_ui(
    state: &mut State,
    message: PlaybackMessage,
) -> DomainUpdateResult {
    match message {
        PlaybackMessage::PlayMediaWithId(media_id) => {
            play_media_with_start(state, media_id, None, false)
        }
        PlaybackMessage::PlayMediaWithIdFromStart(media_id) => {
            play_media_with_start(state, media_id, Some(0.0), false)
        }
        PlaybackMessage::PlayMediaWithIdInMpv(media_id) => {
            play_media_with_start(state, media_id, None, true)
        }
        PlaybackMessage::PlayMediaWithIdInMpvFromStart(media_id) => {
            play_media_with_start(state, media_id, Some(0.0), true)
        }
        PlaybackMessage::PlaySeriesNextEpisode(series_id) => {
            // Prefer identity-based next-episode from server, fall back to local selection.
            let fallback_next =
                crate::domains::media::selectors::select_next_episode_for_series(
                    state, series_id,
                );

            // Resolve TMDB series id from repository (SeriesReference).
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
            } else if let Some(fid) = fallback_next {
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
