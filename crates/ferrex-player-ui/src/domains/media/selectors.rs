//! State-aware episode selector wrappers for the extracted media data domain.

use crate::state::State;
use ferrex_core::player_prelude::{EpisodeID, SeasonID, SeriesID};

pub use ferrex_player_media::selectors::{
    next_episode_by_order_with_repo, previous_episode_by_order_with_repo,
    select_first_episode_for_season_with_repo,
    select_first_episode_for_series_with_repo,
    select_next_episode_for_season_with_repo,
    select_next_episode_for_series_with_repo,
};

/// For a series: choose the first in-progress episode, else the first
/// unwatched episode. If all are completed (or no watch state), fallback to
/// the very first episode (S01E01 in sorted order) if available.
pub fn select_next_episode_for_series(
    state: &State,
    series_id: SeriesID,
) -> Option<EpisodeID> {
    select_next_episode_for_series_with_repo(
        &state.domains.ui.state.repo_accessor,
        state.domains.media.state.get_watch_state().as_ref(),
        series_id,
    )
}

/// Select the first playable episode in canonical series order.
pub fn select_first_episode_for_series(
    state: &State,
    series_id: SeriesID,
) -> Option<EpisodeID> {
    select_first_episode_for_series_with_repo(
        &state.domains.ui.state.repo_accessor,
        series_id,
    )
}

/// Select the first playable episode in a season by episode number.
pub fn select_first_episode_for_season(
    state: &State,
    season_id: SeasonID,
) -> Option<EpisodeID> {
    select_first_episode_for_season_with_repo(
        &state.domains.ui.state.repo_accessor,
        season_id,
    )
}

/// For a season: choose the first in-progress episode, else the first
/// unwatched episode. If all are completed (or no watch state), fallback to
/// the first episode in the season if available.
pub fn select_next_episode_for_season(
    state: &State,
    season_id: SeasonID,
) -> Option<EpisodeID> {
    select_next_episode_for_season_with_repo(
        &state.domains.ui.state.repo_accessor,
        state.domains.media.state.get_watch_state().as_ref(),
        season_id,
    )
}

/// Find the next episode strictly by ordering from the current episode (season, episode).
/// Returns None if the current episode is the last in the series or cannot be resolved.
pub fn next_episode_by_order(
    state: &State,
    current_episode_id: EpisodeID,
) -> Option<EpisodeID> {
    next_episode_by_order_with_repo(
        &state.domains.ui.state.repo_accessor,
        current_episode_id,
    )
}

/// Find the previous episode strictly by ordering from the current episode (season, episode).
/// Returns None if the current episode is the first in the series or cannot be resolved.
pub fn previous_episode_by_order(
    state: &State,
    current_episode_id: EpisodeID,
) -> Option<EpisodeID> {
    previous_episode_by_order_with_repo(
        &state.domains.ui.state.repo_accessor,
        current_episode_id,
    )
}
