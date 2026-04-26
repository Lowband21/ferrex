use crate::{
    common::messages::{DomainMessage, DomainUpdateResult},
    domains::{
        auth::messages::AuthMessage,
        media::selectors,
        ui::{
            feedback_ui::ToastNotification,
            menu::{
                MenuButton, PendingWatchToggleConfirmation, PosterMenuMessage,
                PosterMenuState, WatchToggleAction,
            },
            playback_ui::PlaybackMessage,
            shell_ui::UiShellMessage,
        },
    },
    infra::{
        constants::menu::MENU_KEEPALIVE_MS,
        shader_widgets::poster::{
            PosterFace, PosterInstanceKey, WatchButtonMode,
        },
    },
    state::State,
};

use ferrex_core::player_prelude::{
    EpisodeID, Media, MediaID, MediaIDLike, MovieID, SeasonID, SeriesID,
};
use iced::Task;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone)]
enum PosterMenuTarget {
    Movie(MovieID),
    Series { id: SeriesID, tmdb_id: u64 },
    Season { id: SeasonID, series_id: SeriesID },
    Episode(EpisodeID),
}

fn resolve_menu_target(
    state: &State,
    media_uuid: Uuid,
) -> Option<PosterMenuTarget> {
    let accessor = &state.domains.ui.state.repo_accessor;

    accessor
        .get(&MediaID::Movie(MovieID(media_uuid)))
        .ok()
        .and_then(|media| match media {
            Media::Movie(movie) => Some(PosterMenuTarget::Movie(movie.id)),
            _ => None,
        })
        .or_else(|| {
            accessor
                .get(&MediaID::Series(SeriesID(media_uuid)))
                .ok()
                .and_then(|media| match media {
                    Media::Series(series) => Some(PosterMenuTarget::Series {
                        id: series.id,
                        tmdb_id: series.tmdb_id,
                    }),
                    _ => None,
                })
        })
        .or_else(|| {
            accessor
                .get(&MediaID::Season(SeasonID(media_uuid)))
                .ok()
                .and_then(|media| match media {
                    Media::Season(season) => Some(PosterMenuTarget::Season {
                        id: season.id,
                        series_id: season.series_id,
                    }),
                    _ => None,
                })
        })
        .or_else(|| {
            accessor
                .get(&MediaID::Episode(EpisodeID(media_uuid)))
                .ok()
                .and_then(|media| match media {
                    Media::Episode(episode) => {
                        Some(PosterMenuTarget::Episode(episode.id))
                    }
                    _ => None,
                })
        })
}

fn season_episode_ids(state: &State, season_id: SeasonID) -> Vec<EpisodeID> {
    state
        .domains
        .ui
        .state
        .repo_accessor
        .get_season_episodes(&season_id)
        .unwrap_or_default()
        .into_iter()
        .map(|episode| episode.id)
        .collect()
}

fn all_episodes_watched(
    state: &ferrex_core::player_prelude::UserWatchState,
    episode_ids: impl IntoIterator<Item = EpisodeID>,
) -> bool {
    let mut saw_episode = false;

    for episode_id in episode_ids {
        let episode_uuid = episode_id.to_uuid();
        saw_episode = true;

        if state.in_progress.contains_key(&episode_uuid)
            || !state.completed.contains(&episode_uuid)
        {
            return false;
        }
    }

    saw_episode
}

fn is_series_watched(state: &State, series_id: SeriesID) -> bool {
    let Some(watch_state) =
        state.domains.media.state.get_watch_state().as_ref()
    else {
        return false;
    };

    let season_ids: Vec<SeasonID> = state
        .domains
        .ui
        .state
        .repo_accessor
        .get_series_seasons(&series_id)
        .unwrap_or_default()
        .into_iter()
        .map(|season| season.id)
        .collect();

    if season_ids.is_empty() {
        return false;
    }

    let episode_ids = season_ids
        .into_iter()
        .flat_map(|season_id| season_episode_ids(state, season_id));

    all_episodes_watched(watch_state, episode_ids)
}

fn is_season_watched(state: &State, season_id: SeasonID) -> bool {
    let Some(watch_state) =
        state.domains.media.state.get_watch_state().as_ref()
    else {
        return false;
    };

    all_episodes_watched(watch_state, season_episode_ids(state, season_id))
}

const WATCH_TOGGLE_CONFIRMATION_WINDOW: Duration = Duration::from_secs(3);

fn watch_state_has_progress(
    state: &ferrex_core::player_prelude::UserWatchState,
    media_id: Uuid,
) -> bool {
    state.in_progress.contains_key(&media_id)
        || state.completed.contains(&media_id)
}

fn series_has_existing_watch_progress(
    state: &State,
    series_id: SeriesID,
) -> bool {
    let Some(watch_state) =
        state.domains.media.state.get_watch_state().as_ref()
    else {
        return false;
    };

    state
        .domains
        .ui
        .state
        .repo_accessor
        .get_series_seasons(&series_id)
        .unwrap_or_default()
        .into_iter()
        .map(|season| season.id)
        .flat_map(|season_id| season_episode_ids(state, season_id))
        .any(|episode_id| {
            watch_state_has_progress(watch_state, episode_id.to_uuid())
        })
}

fn season_has_existing_watch_progress(
    state: &State,
    season_id: SeasonID,
) -> bool {
    let Some(watch_state) =
        state.domains.media.state.get_watch_state().as_ref()
    else {
        return false;
    };

    season_episode_ids(state, season_id)
        .into_iter()
        .any(|episode_id| {
            watch_state_has_progress(watch_state, episode_id.to_uuid())
        })
}

fn target_watch_toggle_action(
    state: &State,
    target: &PosterMenuTarget,
) -> Option<WatchToggleAction> {
    match target {
        PosterMenuTarget::Movie(movie_id) => Some(
            if state
                .domains
                .media
                .state
                .is_watched(&MediaID::Movie(movie_id.clone()))
            {
                WatchToggleAction::MarkUnwatched
            } else {
                WatchToggleAction::MarkWatched
            },
        ),
        PosterMenuTarget::Series { id, .. } => {
            Some(if is_series_watched(state, id.clone()) {
                WatchToggleAction::MarkUnwatched
            } else {
                WatchToggleAction::MarkWatched
            })
        }
        PosterMenuTarget::Season { id, .. } => {
            if season_episode_ids(state, id.clone()).is_empty() {
                None
            } else if is_season_watched(state, id.clone()) {
                Some(WatchToggleAction::MarkUnwatched)
            } else {
                Some(WatchToggleAction::MarkWatched)
            }
        }
        PosterMenuTarget::Episode(episode_id) => Some(
            if state
                .domains
                .media
                .state
                .is_watched(&MediaID::Episode(episode_id.clone()))
            {
                WatchToggleAction::MarkUnwatched
            } else {
                WatchToggleAction::MarkWatched
            },
        ),
    }
}

fn target_has_existing_watch_progress(
    state: &State,
    target: &PosterMenuTarget,
) -> bool {
    let Some(watch_state) =
        state.domains.media.state.get_watch_state().as_ref()
    else {
        return false;
    };

    match target {
        PosterMenuTarget::Movie(movie_id) => {
            watch_state_has_progress(watch_state, movie_id.to_uuid())
        }
        PosterMenuTarget::Series { id, .. } => {
            series_has_existing_watch_progress(state, id.clone())
        }
        PosterMenuTarget::Season { id, .. } => {
            season_has_existing_watch_progress(state, id.clone())
        }
        PosterMenuTarget::Episode(episode_id) => {
            watch_state_has_progress(watch_state, episode_id.to_uuid())
        }
    }
}

fn watch_toggle_action_button_label(action: WatchToggleAction) -> &'static str {
    match action {
        WatchToggleAction::MarkWatched => "Watched",
        WatchToggleAction::MarkUnwatched => "Unwatch",
    }
}

fn watch_toggle_confirmation_message(
    target: &PosterMenuTarget,
    action: WatchToggleAction,
) -> String {
    let scope = match target {
        PosterMenuTarget::Movie(_) => "movie",
        PosterMenuTarget::Series { .. } => "series",
        PosterMenuTarget::Season { .. } => "season",
        PosterMenuTarget::Episode(_) => "episode",
    };
    let action_text = match action {
        WatchToggleAction::MarkWatched => "marking it watched",
        WatchToggleAction::MarkUnwatched => "marking it unwatched",
    };

    let button_label = watch_toggle_action_button_label(action);

    format!(
        "This {} already has watch progress. Click the {} button again within 3 seconds to confirm {}.",
        scope, button_label, action_text
    )
}

pub(crate) fn watched_button_mode_for_media_uuid(
    state: &State,
    media_uuid: Uuid,
) -> WatchButtonMode {
    resolve_menu_target(state, media_uuid)
        .and_then(|target| target_watch_toggle_action(state, &target))
        .map(|action| match action {
            WatchToggleAction::MarkWatched => WatchButtonMode::MarkWatched,
            WatchToggleAction::MarkUnwatched => WatchButtonMode::MarkUnwatched,
        })
        .unwrap_or(WatchButtonMode::StaticWatched)
}

fn close_menu_for_instance(
    state: &mut State,
    instance_key: &PosterInstanceKey,
    now: Instant,
) {
    let ui_state = &mut state.domains.ui.state;

    if ui_state.poster_menu_open.as_ref() == Some(instance_key) {
        ui_state.poster_menu_open = None;
    }

    let entry = ui_state
        .poster_menu_states
        .entry(instance_key.clone())
        .or_insert_with(|| PosterMenuState::new(now));
    entry.force_to(now, PosterFace::Front);
}

fn handle_watch_toggle_click(
    state: &mut State,
    media_id: Uuid,
    target: PosterMenuTarget,
    now: Instant,
) -> (DomainUpdateResult, bool) {
    let Some(action) = target_watch_toggle_action(state, &target) else {
        log::warn!(
            "[Menu] Explicit watched/unwatched is not yet supported for target {:?}",
            target
        );
        state.domains.ui.state.pending_watch_toggle_confirmation = None;
        return (DomainUpdateResult::task(Task::none()), true);
    };

    if target_has_existing_watch_progress(state, &target) {
        let already_confirmed = {
            let ui_state = &mut state.domains.ui.state;
            if ui_state
                .pending_watch_toggle_confirmation
                .as_ref()
                .is_some_and(|pending| pending.is_expired(now))
            {
                ui_state.pending_watch_toggle_confirmation = None;
            }

            ui_state
                .pending_watch_toggle_confirmation
                .as_ref()
                .is_some_and(|pending| pending.matches(media_id, action, now))
        };

        if !already_confirmed {
            let prompt = watch_toggle_confirmation_message(&target, action);
            state.domains.ui.state.pending_watch_toggle_confirmation =
                Some(PendingWatchToggleConfirmation {
                    media_id,
                    action,
                    expires_at: now + WATCH_TOGGLE_CONFIRMATION_WINDOW,
                });
            state.domains.ui.state.toast_manager.push(
                ToastNotification::warning(prompt),
                WATCH_TOGGLE_CONFIRMATION_WINDOW,
            );
            return (DomainUpdateResult::task(Task::none()), false);
        }
    }

    state.domains.ui.state.pending_watch_toggle_confirmation = None;

    match watch_toggle_task_for_target(state, target) {
        Some(task) => (DomainUpdateResult::task(task), true),
        None => {
            if state.domains.media.state.api_service.is_none() {
                log::warn!(
                    "[Menu] Cannot toggle watched state without API service"
                );
            }
            (DomainUpdateResult::task(Task::none()), true)
        }
    }
}

fn watch_status_loaded_message(
    result: crate::infra::repository::RepositoryResult<
        ferrex_core::player_prelude::UserWatchState,
    >,
) -> DomainMessage {
    DomainMessage::Auth(AuthMessage::WatchStatusLoaded(
        result.map_err(|e| e.to_string()),
    ))
}

fn play_task_for_target(
    state: &State,
    target: PosterMenuTarget,
) -> Option<Task<DomainMessage>> {
    match target {
        PosterMenuTarget::Movie(movie_id) => {
            Some(Task::done(DomainMessage::Ui(
                PlaybackMessage::PlayMediaWithId(MediaID::Movie(movie_id))
                    .into(),
            )))
        }
        PosterMenuTarget::Series { id, .. } => {
            Some(Task::done(DomainMessage::Ui(
                PlaybackMessage::PlaySeriesNextEpisode(id).into(),
            )))
        }
        PosterMenuTarget::Season { id, .. } => {
            selectors::select_next_episode_for_season(state, id).map(
                |episode_id| {
                    Task::done(DomainMessage::Ui(
                        PlaybackMessage::PlayMediaWithId(MediaID::Episode(
                            episode_id,
                        ))
                        .into(),
                    ))
                },
            )
        }
        PosterMenuTarget::Episode(episode_id) => {
            Some(Task::done(DomainMessage::Ui(
                PlaybackMessage::PlayMediaWithId(MediaID::Episode(episode_id))
                    .into(),
            )))
        }
    }
}

fn details_task_for_target(target: PosterMenuTarget) -> Task<DomainMessage> {
    match target {
        PosterMenuTarget::Movie(movie_id) => Task::done(DomainMessage::Ui(
            UiShellMessage::ViewMovieDetails(movie_id).into(),
        )),
        PosterMenuTarget::Series { id, .. } => {
            Task::done(DomainMessage::Ui(UiShellMessage::ViewTvShow(id).into()))
        }
        PosterMenuTarget::Season { id, series_id } => Task::done(
            DomainMessage::Ui(UiShellMessage::ViewSeason(series_id, id).into()),
        ),
        PosterMenuTarget::Episode(episode_id) => Task::done(DomainMessage::Ui(
            UiShellMessage::ViewEpisode(episode_id).into(),
        )),
    }
}

fn watch_toggle_task_for_target(
    state: &State,
    target: PosterMenuTarget,
) -> Option<Task<DomainMessage>> {
    let api = state.domains.media.state.api_service.clone()?;

    match target {
        PosterMenuTarget::Movie(movie_id) => {
            let is_watched = state
                .domains
                .media
                .state
                .is_watched(&MediaID::Movie(movie_id));
            Some(Task::perform(
                async move {
                    if is_watched {
                        api.mark_movie_unwatched(movie_id.to_uuid()).await?;
                    } else {
                        api.mark_movie_watched(movie_id.to_uuid()).await?;
                    }
                    api.get_watch_state().await
                },
                watch_status_loaded_message,
            ))
        }
        PosterMenuTarget::Series { id, tmdb_id } => {
            let is_watched = is_series_watched(state, id);
            Some(Task::perform(
                async move {
                    if is_watched {
                        api.mark_series_unwatched(tmdb_id).await?;
                    } else {
                        api.mark_series_watched(tmdb_id).await?;
                    }
                    api.get_watch_state().await
                },
                watch_status_loaded_message,
            ))
        }
        PosterMenuTarget::Season { id, .. } => {
            let episode_ids = season_episode_ids(state, id.clone());
            if episode_ids.is_empty() {
                log::warn!(
                    "[Menu] Cannot toggle watched state for empty season {:?}",
                    id
                );
                return None;
            }

            let should_mark_unwatched = is_season_watched(state, id);
            Some(Task::perform(
                async move {
                    for episode_id in episode_ids {
                        if should_mark_unwatched {
                            api.mark_episode_unwatched(episode_id.to_uuid())
                                .await?;
                        } else {
                            api.mark_episode_watched(episode_id.to_uuid())
                                .await?;
                        }
                    }
                    api.get_watch_state().await
                },
                watch_status_loaded_message,
            ))
        }
        PosterMenuTarget::Episode(episode_id) => {
            let is_watched = state
                .domains
                .media
                .state
                .is_watched(&MediaID::Episode(episode_id));
            Some(Task::perform(
                async move {
                    if is_watched {
                        api.mark_episode_unwatched(episode_id.to_uuid())
                            .await?;
                    } else {
                        api.mark_episode_watched(episode_id.to_uuid()).await?;
                    }
                    api.get_watch_state().await
                },
                watch_status_loaded_message,
            ))
        }
    }
}

/// Handle a menu button click - close menu and dispatch action
fn handle_button_click(
    state: &mut State,
    instance_key: PosterInstanceKey,
    button: MenuButton,
    now: Instant,
) -> DomainUpdateResult {
    if state
        .domains
        .ui
        .state
        .pending_watch_toggle_confirmation
        .as_ref()
        .is_some_and(|pending| pending.is_expired(now))
    {
        state.domains.ui.state.pending_watch_toggle_confirmation = None;
    }

    let media_id = instance_key.media_id;
    let target = resolve_menu_target(state, media_id);

    if !matches!(button, MenuButton::Watched) {
        state.domains.ui.state.pending_watch_toggle_confirmation = None;
    }

    let (result, close_menu) = match button {
        MenuButton::Play => {
            log::info!("[Menu] Play clicked for media {:?}", media_id);
            let result = match target
                .and_then(|target| play_task_for_target(state, target))
            {
                Some(task) => DomainUpdateResult::task(task),
                None => {
                    log::warn!(
                        "[Menu] Could not resolve playable target for media {:?}",
                        media_id
                    );
                    DomainUpdateResult::task(Task::none())
                }
            };
            (result, true)
        }
        MenuButton::Details => {
            log::info!("[Menu] Details clicked for media {:?}", media_id);
            let result = match target {
                Some(target) => {
                    DomainUpdateResult::task(details_task_for_target(target))
                }
                None => {
                    log::warn!(
                        "[Menu] Could not resolve details target for media {:?}",
                        media_id
                    );
                    DomainUpdateResult::task(Task::none())
                }
            };
            (result, true)
        }
        MenuButton::Watched => {
            log::info!("[Menu] Toggle watched for media {:?}", media_id);
            match target {
                Some(target) => {
                    handle_watch_toggle_click(state, media_id, target, now)
                }
                None => {
                    log::warn!(
                        "[Menu] Could not resolve watched target for media {:?}",
                        media_id
                    );
                    (DomainUpdateResult::task(Task::none()), true)
                }
            }
        }
        MenuButton::Watchlist | MenuButton::Edit => {
            // These are disabled, shouldn't reach here
            log::warn!("[Menu] Disabled button {:?} clicked", button);
            (DomainUpdateResult::task(Task::none()), true)
        }
    };

    if close_menu {
        close_menu_for_instance(state, &instance_key, now);
    }

    state.domains.ui.state.poster_anim_active_until =
        Some(now + std::time::Duration::from_millis(MENU_KEEPALIVE_MS));
    result
}

pub fn poster_menu_update(
    state: &mut State,
    menu_msg: PosterMenuMessage,
) -> DomainUpdateResult {
    let ui_state = &mut state.domains.ui.state;
    let now = Instant::now();

    match menu_msg {
        PosterMenuMessage::Close(instance_key) => {
            // Force close target poster
            let entry = ui_state
                .poster_menu_states
                .entry(instance_key.clone())
                .or_insert_with(|| PosterMenuState::new(now));
            entry.force_to(now, PosterFace::Front);

            // Clear open menu state
            if ui_state.poster_menu_open.as_ref() == Some(&instance_key) {
                ui_state.poster_menu_open = None;
            }
        }
        PosterMenuMessage::Start(instance_key) => {
            // Close previous open poster if exists
            if let Some(ref open_key) = ui_state.poster_menu_open
                && open_key != &instance_key
            {
                let entry_prev = ui_state
                    .poster_menu_states
                    .entry(open_key.clone())
                    .or_insert_with(|| PosterMenuState::new(now));
                entry_prev.force_to(now, PosterFace::Front);
            }

            // Start hold on target poster
            let entry = ui_state
                .poster_menu_states
                .entry(instance_key.clone())
                .or_insert_with(|| PosterMenuState::new(now));
            entry.mark_begin(now);

            // Always set poster_menu_open to the provided target
            ui_state.poster_menu_open = Some(instance_key);
        }
        PosterMenuMessage::End(instance_key) => {
            if let Some(entry) =
                ui_state.poster_menu_states.get_mut(&instance_key)
            {
                entry.mark_end(now);
            }
        }
        PosterMenuMessage::ButtonClicked(instance_key, button) => {
            return handle_button_click(state, instance_key, button, now);
        }
    }

    ui_state.poster_anim_active_until =
        Some(now + std::time::Duration::from_millis(MENU_KEEPALIVE_MS));
    DomainUpdateResult::task(Task::none())
}
