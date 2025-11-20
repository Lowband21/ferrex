use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
};
use ferrex_core::{SeasonID, SeriesID};
use serde_json::{json, Value};
use tracing::{info, warn};
use uuid::Uuid;

// TV Show handlers
pub async fn list_shows_handler(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    info!("Listing all TV shows");

    // Get all series references from the database
    match state.db.backend().get_series_references().await {
        Ok(series_list) => {
            info!("Found {} TV series", series_list.len());

            Ok(Json(json!({
                "status": "success",
                "series": series_list,
                "count": series_list.len()
            })))
        }
        Err(e) => {
            warn!("Failed to retrieve TV shows: {}", e);
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}

pub async fn show_details_handler(
    State(state): State<AppState>,
    Path(series_id): Path<Uuid>,
) -> Result<Json<Value>, StatusCode> {
    info!("Getting details for series ID: {}", series_id);

    // Parse the series ID
    let series_id = SeriesID(series_id);

    // Get the series reference
    match state.db.backend().get_series_reference(&series_id).await {
        Ok(series) => {
            // Get seasons for this series
            let seasons = match state.db.backend().get_series_seasons(&series_id).await {
                Ok(seasons) => seasons,
                Err(e) => {
                    warn!("Failed to get seasons for series: {}", e);
                    vec![]
                }
            };

            Ok(Json(json!({
                "status": "success",
                "series": series,
                "seasons": seasons
            })))
        }
        Err(e) => {
            warn!("Failed to retrieve series details: {}", e);
            if e.to_string().contains("not found") {
                Err(StatusCode::NOT_FOUND)
            } else {
                Ok(Json(json!({
                    "status": "error",
                    "error": e.to_string()
                })))
            }
        }
    }
}

pub async fn season_details_handler(
    State(state): State<AppState>,
    Path(season_id): Path<Uuid>,
) -> Result<Json<Value>, StatusCode> {
    info!("Getting details for season ID: {}", season_id);

    // Parse the season ID
    let season_id = SeasonID(season_id);

    // Get the season reference
    match state.db.backend().get_season_reference(&season_id).await {
        Ok(season) => {
            // Get episodes for this season
            let episodes = match state.db.backend().get_season_episodes(&season_id).await {
                Ok(episodes) => episodes,
                Err(e) => {
                    warn!("Failed to get episodes for season: {}", e);
                    vec![]
                }
            };

            Ok(Json(json!({
                "status": "success",
                "season": season,
                "episodes": episodes
            })))
        }
        Err(e) => {
            warn!("Failed to retrieve season details: {}", e);
            if e.to_string().contains("not found") {
                Err(StatusCode::NOT_FOUND)
            } else {
                Ok(Json(json!({
                    "status": "error",
                    "error": e.to_string()
                })))
            }
        }
    }
}

// Get all episodes for a series
pub async fn show_episodes_handler(
    State(state): State<AppState>,
    Path(series_id): Path<Uuid>,
) -> Result<Json<Value>, StatusCode> {
    info!("Getting episodes for series ID: {}", series_id);

    // Parse the series ID
    let series_id = SeriesID(series_id);

    // Get all seasons for the series
    match state.db.backend().get_series_seasons(&series_id).await {
        Ok(seasons) => {
            let mut all_episodes = Vec::new();

            // Get episodes for each season
            for season in seasons {
                match state.db.backend().get_season_episodes(&season.id).await {
                    Ok(episodes) => all_episodes.extend(episodes),
                    Err(e) => warn!("Failed to get episodes for season: {}", e),
                }
            }

            Ok(Json(json!({
                "status": "success",
                "series_id": format!("{:?}", series_id),
                "count": all_episodes.len(),
                "episodes": all_episodes
            })))
        }
        Err(e) => {
            warn!("Failed to retrieve seasons for series: {}", e);
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}
