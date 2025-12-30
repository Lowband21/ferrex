use crate::state::State;
use uuid::Uuid;

use crate::infra::api_types::LibraryType;
use crate::infra::repository::MaybeYoked;
use ferrex_core::player_prelude::{Media, MediaID, MovieID, SeriesID};
use rkyv::option::ArchivedOption;

/// Extend the UI keep-alive window used to keep animations/rendering active
/// after user-driven scrolls or carousel motions. This prevents visible stalls
/// while atlas uploads complete. Duration is controlled by RuntimeConfig.
pub fn bump_keep_alive(state: &mut State) {
    use std::time::{Duration, Instant};
    let keep_alive_ms = state.runtime_config.keep_alive_ms();
    let until = Instant::now() + Duration::from_millis(keep_alive_ms);
    let ui_until = &mut state.domains.ui.state.poster_anim_active_until;
    *ui_until = Some(ui_until.map(|u| u.max(until)).unwrap_or(until));
}

pub fn primary_poster_iid_for_library_media(
    state: &State,
    library_type: LibraryType,
    media_uuid: Uuid,
) -> Option<Uuid> {
    match library_type {
        LibraryType::Movies => state
            .domains
            .ui
            .state
            .repo_accessor
            .get(&MediaID::Movie(MovieID(media_uuid)))
            .ok()
            .and_then(|m| match m {
                Media::Movie(mr) => mr.details.primary_poster_iid,
                _ => None,
            }),
        LibraryType::Series => state
            .domains
            .ui
            .state
            .repo_accessor
            .get(&MediaID::Series(SeriesID(media_uuid)))
            .ok()
            .and_then(|m| match m {
                Media::Series(sr) => sr.details.primary_poster_iid,
                _ => None,
            }),
    }
}

pub fn primary_poster_iid_for_library_media_cached(
    state: &State,
    library_type: LibraryType,
    media_uuid: Uuid,
) -> Option<Uuid> {
    match library_type {
        LibraryType::Movies => {
            if let Some(yoke) = state
                .domains
                .ui
                .state
                .movie_yoke_cache
                .peek_ref(&media_uuid)
            {
                let m = yoke.get();
                match &m.details.primary_poster_iid {
                    ArchivedOption::Some(iid) => Some(*iid),
                    ArchivedOption::None => None,
                }
            } else {
                primary_poster_iid_for_library_media(
                    state,
                    library_type,
                    media_uuid,
                )
            }
        }
        LibraryType::Series => {
            if let Some(yoke) = state
                .domains
                .ui
                .state
                .series_yoke_cache
                .peek_ref(&media_uuid)
            {
                let s = yoke.get();
                match &s.details.primary_poster_iid {
                    ArchivedOption::Some(iid) => Some(*iid),
                    ArchivedOption::None => None,
                }
            } else {
                primary_poster_iid_for_library_media(
                    state,
                    library_type,
                    media_uuid,
                )
            }
        }
    }
}

pub fn primary_poster_iid_for_movie_or_series(
    state: &State,
    media_uuid: Uuid,
) -> Option<Uuid> {
    // Try movie first, then series. UUIDs are globally unique in practice.
    state
        .domains
        .ui
        .state
        .repo_accessor
        .get(&MediaID::Movie(MovieID(media_uuid)))
        .ok()
        .and_then(|m| match m {
            Media::Movie(mr) => mr.details.primary_poster_iid,
            _ => None,
        })
        .or_else(|| {
            state
                .domains
                .ui
                .state
                .repo_accessor
                .get(&MediaID::Series(SeriesID(media_uuid)))
                .ok()
                .and_then(|m| match m {
                    Media::Series(sr) => sr.details.primary_poster_iid,
                    _ => None,
                })
        })
}
