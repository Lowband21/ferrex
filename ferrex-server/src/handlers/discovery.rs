use std::collections::HashMap;

use axum::{Extension, Json, extract::Query, extract::State};
use ferrex_core::{
    api::{ApiResponse, types::*},
    application::unit_of_work::AppUnitOfWork,
    domain::{users::user::User, watch::ItemWatchStatus},
    player_prelude::{
        MediaFilters, MediaQuery, MediaTypeFilter, MediaWithStatus, Pagination,
        SortBy, SortCriteria, SortOrder,
    },
};
use ferrex_model::{MediaID, MovieID, MovieReference};
use serde::Deserialize;
use uuid::Uuid;

use crate::{infra::app_state::AppState, infra::errors::AppError};

const DEFAULT_DISCOVERY_SECTION_LIMIT: usize = 20;
const MAX_DISCOVERY_SECTION_LIMIT: usize = 40;
const MAX_EXPLORE_FETCH_LIMIT: usize = 100;

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct ExploreQueryParams {
    /// Optional per-section item limit. Values are clamped to a small shelf size.
    pub limit: Option<usize>,
}

impl ExploreQueryParams {
    fn section_limit(self) -> usize {
        discovery_section_limit(self.limit)
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct ResumeQueryParams {
    /// Optional resume item limit. Values are clamped like Explore shelves.
    pub limit: Option<usize>,
}

impl ResumeQueryParams {
    fn section_limit(self) -> usize {
        discovery_section_limit(self.limit)
    }
}

fn discovery_section_limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(DEFAULT_DISCOVERY_SECTION_LIMIT)
        .clamp(1, MAX_DISCOVERY_SECTION_LIMIT)
}

/// Deterministic global Explore shelves.
pub async fn get_explore_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Query(params): Query<ExploreQueryParams>,
) -> Result<Json<ApiResponse<DiscoveryResponse>>, AppError> {
    let limit = params.section_limit();
    let uow = state.unit_of_work();

    let recently_added = query_movie_shelf(
        uow.as_ref(),
        user.id,
        SortBy::DateAdded,
        limit,
        recently_added_reason,
    )
    .await?;

    let recently_released = query_movie_shelf(
        uow.as_ref(),
        user.id,
        SortBy::ReleaseDate,
        limit,
        recently_released_reason,
    )
    .await?;

    let audience_rating_picks = query_movie_shelf(
        uow.as_ref(),
        user.id,
        SortBy::Rating,
        limit,
        audience_rating_reason,
    )
    .await?;

    let mut sections = Vec::new();
    push_section_if_not_empty(
        &mut sections,
        DiscoverySection::poster_row(
            DISCOVERY_SECTION_RECENTLY_ADDED,
            "Recently added",
            Some("New in your library".to_string()),
            recently_added,
            limit,
        ),
    );
    push_section_if_not_empty(
        &mut sections,
        DiscoverySection::poster_row(
            DISCOVERY_SECTION_RECENTLY_RELEASED,
            "Recently released",
            Some("Recent releases available in your library".to_string()),
            recently_released,
            limit,
        ),
    );
    push_section_if_not_empty(
        &mut sections,
        DiscoverySection::poster_row(
            DISCOVERY_SECTION_AUDIENCE_RATING_PICKS,
            "Audience rating picks",
            Some("Highly rated titles with metadata".to_string()),
            audience_rating_picks,
            limit,
        ),
    );

    Ok(Json(ApiResponse::success(DiscoveryResponse::new(sections))))
}

/// Deterministic Resume shelf backed by the existing continue-watching read path.
pub async fn get_resume_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Query(params): Query<ResumeQueryParams>,
) -> Result<Json<ApiResponse<DiscoveryResponse>>, AppError> {
    let limit = params.section_limit();
    let uow = state.unit_of_work();
    let continue_items = uow
        .watch_status
        .get_continue_watching(user.id, limit)
        .await?;
    let items = continue_items
        .iter()
        .map(discovery_item_from_continue_watching)
        .collect();

    let section = DiscoverySection::continue_row(
        DISCOVERY_SECTION_RESUME,
        "Resume",
        Some("Pick up where you left off".to_string()),
        items,
        limit,
    );

    Ok(Json(ApiResponse::success(DiscoveryResponse::new(vec![
        section,
    ]))))
}

async fn query_movie_shelf<F>(
    uow: &AppUnitOfWork,
    user_id: Uuid,
    sort_by: SortBy,
    limit: usize,
    reason_for: F,
) -> Result<Vec<DiscoveryItem>, AppError>
where
    F: Fn(&MovieReference) -> Option<String>,
{
    let query = movie_query(user_id, sort_by, expanded_fetch_limit(limit));
    let hits = uow.query.query_media(&query).await?;
    let movie_ids = movie_ids_from_hits(&hits);
    let movies = load_movie_references(uow, &movie_ids).await?;

    Ok(build_movie_discovery_items(hits, movies, limit, reason_for))
}

fn movie_query(user_id: Uuid, sort_by: SortBy, limit: usize) -> MediaQuery {
    MediaQuery {
        filters: MediaFilters {
            media_type: Some(MediaTypeFilter::Movie),
            ..Default::default()
        },
        sort: SortCriteria {
            primary: sort_by,
            order: SortOrder::Descending,
            secondary: Some(SortBy::Title),
        },
        search: None,
        pagination: Pagination { offset: 0, limit },
        user_context: Some(user_id),
    }
}

fn expanded_fetch_limit(limit: usize) -> usize {
    limit
        .saturating_mul(4)
        .max(limit)
        .min(MAX_EXPLORE_FETCH_LIMIT)
}

fn movie_ids_from_hits(hits: &[MediaWithStatus]) -> Vec<MovieID> {
    hits.iter()
        .filter_map(|hit| match hit.id {
            MediaID::Movie(id) => Some(id),
            _ => None,
        })
        .collect()
}

async fn load_movie_references(
    uow: &AppUnitOfWork,
    movie_ids: &[MovieID],
) -> Result<Vec<MovieReference>, AppError> {
    let refs: Vec<&MovieID> = movie_ids.iter().collect();
    Ok(uow.media_refs.get_movie_references_bulk(&refs).await?)
}

fn build_movie_discovery_items<F>(
    hits: Vec<MediaWithStatus>,
    movies: Vec<MovieReference>,
    limit: usize,
    reason_for: F,
) -> Vec<DiscoveryItem>
where
    F: Fn(&MovieReference) -> Option<String>,
{
    let by_id: HashMap<MovieID, MovieReference> =
        movies.into_iter().map(|movie| (movie.id, movie)).collect();
    let mut items = Vec::with_capacity(limit.min(hits.len()));

    for hit in hits {
        if is_completed(hit.watch_status.as_ref()) {
            continue;
        }

        let MediaID::Movie(movie_id) = hit.id else {
            continue;
        };
        let Some(movie) = by_id.get(&movie_id) else {
            continue;
        };
        let Some(reason) = reason_for(movie) else {
            continue;
        };

        items.push(movie_to_discovery_item(
            movie,
            hit.watch_status.as_ref(),
            reason,
        ));

        if items.len() >= limit {
            break;
        }
    }

    items
}

fn movie_to_discovery_item(
    movie: &MovieReference,
    watch_status: Option<&ItemWatchStatus>,
    reason: String,
) -> DiscoveryItem {
    let media_id = MediaID::Movie(movie.id);
    let mut item =
        DiscoveryItem::new(media_id, movie.title.as_ref().to_string());
    item.poster_iid = movie.details.primary_poster_iid;
    item.backdrop_iid = movie.details.primary_backdrop_iid;
    item.release_date = movie.details.release_date.clone();
    item.release_year = item
        .release_date
        .as_deref()
        .and_then(release_year_from_date);
    item.runtime_minutes = movie.details.runtime;
    item.ratings = DiscoveryRatingSummary {
        audience: movie.details.vote_average,
        critic: None,
    };
    item.watch = watch_status.map(DiscoveryWatchSummary::from_item_status);
    item.playback = Some(DiscoveryPlaybackAction {
        target_media_id: movie.id.to_uuid(),
        target_media_type: DiscoveryMediaType::Movie,
        hint: DiscoveryPlaybackHint::Play,
    });
    item.reason = Some(reason);
    item
}

fn recently_added_reason(_movie: &MovieReference) -> Option<String> {
    Some("Recently added to your library".to_string())
}

fn recently_released_reason(movie: &MovieReference) -> Option<String> {
    let release_date = movie.details.release_date.as_deref()?;
    let reason = release_year_from_date(release_date).map_or_else(
        || "Recently released".to_string(),
        |year| format!("Released in {year}"),
    );

    Some(reason)
}

fn audience_rating_reason(movie: &MovieReference) -> Option<String> {
    let rating = movie
        .details
        .vote_average
        .filter(|rating| rating.is_finite())?;
    Some(format!("{rating:.1}/10 audience rating"))
}

fn is_completed(status: Option<&ItemWatchStatus>) -> bool {
    matches!(status, Some(ItemWatchStatus::Completed(_)))
}

fn push_section_if_not_empty(
    sections: &mut Vec<DiscoverySection>,
    section: DiscoverySection,
) {
    if !section.is_empty() {
        sections.push(section);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrex_core::domain::watch::{CompletedItem, InProgressItem};

    #[test]
    fn discovery_section_limit_clamps_query_param() {
        assert_eq!(discovery_section_limit(None), 20);
        assert_eq!(discovery_section_limit(Some(0)), 1);
        assert_eq!(discovery_section_limit(Some(1)), 1);
        assert_eq!(discovery_section_limit(Some(40)), 40);
        assert_eq!(discovery_section_limit(Some(41)), 40);
    }

    #[test]
    fn expanded_fetch_limit_is_bounded() {
        assert_eq!(expanded_fetch_limit(1), 4);
        assert_eq!(expanded_fetch_limit(20), 80);
        assert_eq!(expanded_fetch_limit(40), 100);
        assert_eq!(expanded_fetch_limit(usize::MAX), 100);
    }

    #[test]
    fn movie_query_uses_user_context_and_bounded_limit() {
        let user_id = Uuid::from_u128(99);
        let query = movie_query(user_id, SortBy::Rating, 16);

        assert_eq!(query.user_context, Some(user_id));
        assert_eq!(query.pagination.limit, 16);
        assert_eq!(query.pagination.offset, 0);
        assert_eq!(query.filters.media_type, Some(MediaTypeFilter::Movie));
        assert_eq!(query.sort.primary, SortBy::Rating);
        assert_eq!(query.sort.order, SortOrder::Descending);
        assert_eq!(query.sort.secondary, Some(SortBy::Title));
    }

    #[test]
    fn completed_watch_status_is_filtered() {
        let movie_id = MovieID(Uuid::from_u128(100));
        let completed = ItemWatchStatus::Completed(CompletedItem {
            media_id: MediaID::Movie(movie_id),
            last_watched: 1_700_000_500,
        });
        let in_progress = ItemWatchStatus::InProgress(InProgressItem {
            media_id: movie_id.to_uuid(),
            position: 12.0,
            duration: 120.0,
            last_watched: 1_700_000_600,
        });

        assert!(is_completed(Some(&completed)));
        assert!(!is_completed(Some(&in_progress)));
        assert!(!is_completed(None));
    }
}
