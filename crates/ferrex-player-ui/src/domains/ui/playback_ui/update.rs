use crate::{
    common::messages::{CrossDomainEvent, DomainMessage, DomainUpdateResult},
    domains::ui::{
        messages::UiMessage, playback_ui::PlaybackMessage,
        shell_ui::UiShellMessage,
    },
    state::State,
};
use ferrex_core::player_prelude::{
    EpisodeID, EpisodeLike, Media, MediaID, MovieLike,
};
use ferrex_player_playback::contract::{BackendRequest, PlaybackTarget};
use iced::Task;

fn play_media_with_position(
    state: &mut State,
    media_id: MediaID,
    position: f32,
) -> DomainUpdateResult {
    state.domains.player.state.backend_request = BackendRequest::Auto;
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

            let duration_hint = media_file
                .media_file_metadata
                .as_ref()
                .and_then(|meta| meta.duration)
                .filter(|duration| *duration > 0.0)
                .unwrap_or(0.0);

            if !state.domains.player.state.has_observable_playback_root() {
                state.domains.player.state.last_valid_position =
                    position as f64;
                state.domains.player.state.last_valid_duration = duration_hint;
            }
            state.domains.media.state.pending_resume_position = Some(position);
            state.domains.player.state.pending_resume_position = Some(position);

            DomainUpdateResult::task(Task::done(DomainMessage::Player(
                crate::domains::player::messages::PlayerMessage::PlayMediaWithId(
                    media_file, media_id,
                ),
            )))
        }
        Err(_) => {
            log::error!("Failed to get media with id {}", media_id);
            DomainUpdateResult::task(Task::none())
        }
    }
}

fn play_media_with_mpv_mode(
    state: &mut State,
    media_id: MediaID,
    external_process: bool,
) -> DomainUpdateResult {
    // Backend-disabled builds retain the historical external-process action.
    let external_process = external_process || !cfg!(feature = "mpv");
    state.domains.player.state.backend_request = if external_process {
        BackendRequest::Auto
    } else {
        BackendRequest::Exact(in_process_mpv_target())
    };

    let media_file = match state.domains.ui.state.repo_accessor.get(&media_id) {
        Ok(Media::Movie(movie)) => movie.file(),
        Ok(Media::Episode(episode)) => episode.file(),
        Ok(_) => {
            log::error!("Media not playable type {media_id}");
            return DomainUpdateResult::task(Task::none());
        }
        Err(_) => {
            log::error!("Failed to get media with id {media_id}");
            return DomainUpdateResult::task(Task::none());
        }
    };

    let mut resume_opt = None;
    let mut watch_duration_hint = None;
    if let Some(watch_state) = &state.domains.media.state.user_watch_state
        && let Some(item) = watch_state.get_by_media_id(media_id.as_uuid())
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
        .and_then(|metadata| metadata.duration)
        .filter(|duration| *duration > 0.0);
    let duration_hint = watch_duration_hint.or(metadata_duration_hint);

    if !state.domains.player.state.has_observable_playback_root() {
        state.domains.player.state.last_valid_position =
            resume_opt.map(|position| position as f64).unwrap_or(0.0);
        state.domains.player.state.last_valid_duration =
            duration_hint.unwrap_or(0.0);
    }
    state.domains.media.state.pending_resume_position = resume_opt;
    state.domains.player.state.pending_resume_position = resume_opt;

    if external_process {
        DomainUpdateResult::task(Task::done(DomainMessage::Player(
            crate::domains::player::messages::PlayerMessage::PlayMediaWithIdExternally(
                media_file, media_id,
            ),
        )))
    } else if in_process_mpv_target() == PlaybackTarget::MPV_INTEGRATED {
        let play = Task::done(DomainMessage::Player(
            crate::domains::player::messages::PlayerMessage::PlayMediaWithId(
                media_file, media_id,
            ),
        ));
        // Allocate the transparent controls host before playback attachment.
        // It remains invisible until the presenter reports `Attached`.
        DomainUpdateResult::task(
            Task::done(DomainMessage::Ui(
                UiShellMessage::OpenPlayerOverlay.into(),
            ))
            .chain(play),
        )
    } else {
        DomainUpdateResult::task(Task::done(DomainMessage::Player(
            crate::domains::player::messages::PlayerMessage::PlayMediaWithId(
                media_file, media_id,
            ),
        )))
    }
}

const fn in_process_mpv_target() -> PlaybackTarget {
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        PlaybackTarget::MPV_INTEGRATED
    } else {
        PlaybackTarget::MPV_NATIVE_WINDOW
    }
}

pub fn update_playback_ui(
    state: &mut State,
    message: PlaybackMessage,
) -> DomainUpdateResult {
    match message {
        PlaybackMessage::PlayMediaWithId(media_id) => {
            state.domains.player.state.backend_request = BackendRequest::Auto;
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
        PlaybackMessage::PlayMediaWithIdFromStart(media_id) => {
            play_media_with_position(state, media_id, 0.0)
        }
        PlaybackMessage::PlayMediaWithIdInMpv(media_id) => {
            // Explicit opt-in to in-process libmpv native-window mode. Auto
            // remains Subwave, and initialization failures fall back there.
            play_media_with_mpv_mode(state, media_id, false)
        }
        PlaybackMessage::PlayMediaWithIdExternally(media_id) => {
            play_media_with_mpv_mode(state, media_id, true)
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
