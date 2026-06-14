//! Episode selection helpers using explicit repository/watch-state inputs.

use ferrex_core::player_prelude::{
    EpisodeID, SeasonID, SeriesID, UserWatchState,
};
use ferrex_model::{EpisodeReference, Media};
use ferrex_player_library::repository::{Accessor, ReadOnly};

pub fn select_next_episode_for_series_with_repo(
    accessor: &Accessor<ReadOnly>,
    watch_state: Option<&UserWatchState>,
    series_id: SeriesID,
) -> Option<EpisodeID> {
    let episodes = ordered_series_episodes(accessor, &series_id);
    select_next_from_ordered_episodes(watch_state, &episodes)
}

pub fn select_first_episode_for_series_with_repo(
    accessor: &Accessor<ReadOnly>,
    series_id: SeriesID,
) -> Option<EpisodeID> {
    ordered_series_episodes(accessor, &series_id)
        .first()
        .map(|episode| episode.id)
}

pub fn select_first_episode_for_season_with_repo(
    accessor: &Accessor<ReadOnly>,
    season_id: SeasonID,
) -> Option<EpisodeID> {
    let mut episodes =
        accessor.get_season_episodes(&season_id).unwrap_or_default();
    episodes.sort_by_key(|episode| episode.episode_number.value());
    episodes.first().map(|episode| episode.id)
}

pub fn select_next_episode_for_season_with_repo(
    accessor: &Accessor<ReadOnly>,
    watch_state: Option<&UserWatchState>,
    season_id: SeasonID,
) -> Option<EpisodeID> {
    let mut episodes =
        accessor.get_season_episodes(&season_id).unwrap_or_default();
    episodes.sort_by_key(|episode| episode.episode_number.value());
    select_next_from_ordered_episodes(watch_state, &episodes)
}

fn select_next_from_ordered_episodes(
    watch_state: Option<&UserWatchState>,
    episodes: &[ferrex_core::player_prelude::EpisodeReference],
) -> Option<EpisodeID> {
    if episodes.is_empty() {
        return None;
    }

    if let Some(watch_state) = watch_state {
        if let Some(in_progress) = episodes.iter().find(|episode| {
            watch_state.in_progress.contains_key(&episode.id.to_uuid())
        }) {
            return Some(in_progress.id);
        }

        if let Some(unwatched) = episodes.iter().find(|episode| {
            let id = episode.id.to_uuid();
            !watch_state.in_progress.contains_key(&id)
                && !watch_state.completed.contains(&id)
        }) {
            return Some(unwatched.id);
        }
    }

    Some(episodes[0].id)
}

fn resolve_episode_using_accessor(
    accessor: &Accessor<ReadOnly>,
    episode_id: &EpisodeID,
) -> Option<Box<EpisodeReference>> {
    accessor.get(episode_id).ok().and_then(|media| match media {
        Media::Episode(episode) => Some(episode),
        _ => None,
    })
}

fn ordered_series_episodes(
    accessor: &Accessor<ReadOnly>,
    series_id: &SeriesID,
) -> Vec<ferrex_core::player_prelude::EpisodeReference> {
    let seasons = accessor.get_series_seasons(series_id).unwrap_or_default();
    let mut episodes = Vec::new();
    for season in &seasons {
        let mut eps =
            accessor.get_season_episodes(&season.id).unwrap_or_default();
        eps.sort_by_key(|episode| episode.episode_number.value());
        episodes.extend(eps);
    }
    episodes.sort_by_key(|episode| {
        (
            episode.season_number.value(),
            episode.episode_number.value(),
        )
    });
    episodes
}

pub fn next_episode_by_order_with_repo(
    accessor: &Accessor<ReadOnly>,
    current_episode_id: EpisodeID,
) -> Option<EpisodeID> {
    let current =
        resolve_episode_using_accessor(accessor, &current_episode_id)?;
    let episodes = ordered_series_episodes(accessor, &current.series_id);

    if let Some(idx) =
        episodes.iter().position(|episode| episode.id == current.id)
        && idx + 1 < episodes.len()
    {
        return Some(episodes[idx + 1].id);
    }
    None
}

pub fn previous_episode_by_order_with_repo(
    accessor: &Accessor<ReadOnly>,
    current_episode_id: EpisodeID,
) -> Option<EpisodeID> {
    let current =
        resolve_episode_using_accessor(accessor, &current_episode_id)?;
    let episodes = ordered_series_episodes(accessor, &current.series_id);

    if let Some(idx) =
        episodes.iter().position(|episode| episode.id == current.id)
        && idx > 0
    {
        return Some(episodes[idx - 1].id);
    }
    None
}
