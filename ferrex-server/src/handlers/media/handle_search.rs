use axum::{Extension, Json, extract::State};
use ferrex_core::{
    api::ApiResponse,
    domain::watch::ItemWatchStatus,
    player_prelude::{MediaQuery, MediaWithStatus, User},
};

use crate::infra::{
    app_state::AppState,
    content_negotiation::{self as cn, AcceptFormat, NegotiatedResponse},
    errors::AppError,
};

const DEFAULT_SEARCH_LIMIT: usize = 50;
const MAX_SEARCH_LIMIT: usize = 100;

/// Execute a media query
pub async fn query_media_handler(
    State(state): State<AppState>,
    accept: AcceptFormat,
    Extension(user): Extension<User>,
    cn::FlexJson(mut query): cn::FlexJson<MediaQuery>,
) -> Result<NegotiatedResponse, AppError> {
    // Add user context to the query
    query.user_context = Some(user.id);

    clamp_query_limit(&mut query);

    // Execute the query
    let results = state.unit_of_work().query.query_media(&query).await?;

    Ok(respond_media_query(accept, &results))
}

/// Execute a media query without authentication (public)
pub async fn query_media_public_handler(
    State(state): State<AppState>,
    accept: AcceptFormat,
    cn::FlexJson(mut query): cn::FlexJson<MediaQuery>,
) -> Result<NegotiatedResponse, AppError> {
    // Execute the query without user context
    clamp_query_limit(&mut query);
    clamp_query_limit(&mut query);
    let results = state.unit_of_work().query.query_media(&query).await?;

    Ok(respond_media_query(accept, &results))
}

fn respond_media_query(accept: AcceptFormat, results: &[MediaWithStatus]) -> NegotiatedResponse {
    cn::respond(
        accept,
        &ApiResponse::success(results),
        || {
            use ferrex_flatbuffers::conversions::media_query::MediaQueryHit;

            let hits: Vec<MediaQueryHit> = results
                .iter()
                .map(|item| {
                    let media_uuid = *item.id.as_uuid();
                    let (position, duration, completed, last_watched) =
                        match &item.watch_status {
                            Some(ItemWatchStatus::InProgress(ip)) => {
                                (ip.position as f64, ip.duration as f64, false, ip.last_watched)
                            }
                            Some(ItemWatchStatus::Completed(_)) => {
                                (0.0, 0.0, true, 0)
                            }
                            None => (0.0, 0.0, false, 0),
                        };
                    MediaQueryHit {
                        media_uuid,
                        position,
                        duration,
                        completed,
                        last_watched_secs: last_watched,
                    }
                })
                .collect();

            ferrex_flatbuffers::conversions::media_query::serialize_media_query_results(&hits)
        },
    )
}

fn clamp_query_limit(query: &mut MediaQuery) {
    if query.pagination.limit == 0 {
        query.pagination.limit = DEFAULT_SEARCH_LIMIT;
    } else if query.pagination.limit > MAX_SEARCH_LIMIT {
        query.pagination.limit = MAX_SEARCH_LIMIT;
    }
}
