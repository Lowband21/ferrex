use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
};
use ferrex_core::{
    api_types::{ApiResponse, BatchMediaRequest, BatchMediaResponse, MediaId},
    media::{
        EpisodeID, MediaDetailsOption, MediaReference, MovieID, SeasonID, SeriesID,
        TmdbDetails,
    },
    MediaError,
};
use tracing::{error, info, warn};
use uuid::Uuid;

/// Single endpoint to fetch any media reference by ID
/// Returns the appropriate MediaReference variant based on the ID type
pub async fn get_media_reference_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<MediaReference>>, StatusCode> {
    info!("Fetching media reference for ID: {}", id);

    // Try to parse as UUID first (for backward compatibility)
    let id_str = if let Ok(uuid) = Uuid::parse_str(&id) {
        uuid.to_string()
    } else {
        id.clone()
    };

    // Try each media type in order
    if let Ok(media_ref) = fetch_movie_reference(&state, &id_str).await {
        return Ok(Json(ApiResponse {
            status: "success".to_string(),
            data: Some(media_ref),
            error: None,
            message: None,
        }));
    }

    if let Ok(media_ref) = fetch_series_reference(&state, &id_str).await {
        return Ok(Json(ApiResponse {
            status: "success".to_string(),
            data: Some(media_ref),
            error: None,
            message: None,
        }));
    }

    if let Ok(media_ref) = fetch_season_reference(&state, &id_str).await {
        return Ok(Json(ApiResponse {
            status: "success".to_string(),
            data: Some(media_ref),
            error: None,
            message: None,
        }));
    }

    if let Ok(media_ref) = fetch_episode_reference(&state, &id_str).await {
        return Ok(Json(ApiResponse {
            status: "success".to_string(),
            data: Some(media_ref),
            error: None,
            message: None,
        }));
    }

    // If we get here, the ID doesn't match any media
    warn!("Media not found for ID: {}", id);
    Ok(Json(ApiResponse {
        status: "error".to_string(),
        data: None,
        error: Some(format!("Media not found for ID: {}", id)),
        message: None,
    }))
}

/// Helper function to fetch a movie reference
async fn fetch_movie_reference(
    state: &AppState,
    id: &str,
) -> Result<MediaReference, MediaError> {
    let movie_id = MovieID::new(id.to_string())?;
    
    match state.db.backend().get_movie_reference(&movie_id).await {
        Ok(movie_ref) => {
            info!("Found movie: {}", movie_ref.title.as_str());
            
            // Ensure we have full metadata
            match &movie_ref.details {
                MediaDetailsOption::Details(TmdbDetails::Movie(details)) => {
                    info!(
                        "Movie has full TMDB metadata - {} genres, {} cast members, {} images",
                        details.genres.len(),
                        details.cast.len(),
                        details.images.posters.len() + details.images.backdrops.len() + details.images.logos.len()
                    );
                }
                MediaDetailsOption::Endpoint(_) => {
                    warn!("Movie {} only has endpoint URL, not full metadata", id);
                }
                _ => {
                    error!("Movie {} has wrong metadata type", id);
                }
            }
            
            Ok(MediaReference::Movie(movie_ref))
        }
        Err(e) => Err(e),
    }
}

/// Helper function to fetch a series reference
async fn fetch_series_reference(
    state: &AppState,
    id: &str,
) -> Result<MediaReference, MediaError> {
    let series_id = SeriesID::new(id.to_string())?;
    
    match state.db.backend().get_series_reference(&series_id).await {
        Ok(series_ref) => {
            info!("Found series: {}", series_ref.title.as_str());
            
            // Ensure we have full metadata
            match &series_ref.details {
                MediaDetailsOption::Details(TmdbDetails::Series(details)) => {
                    info!(
                        "Series has full TMDB metadata - {} seasons, {} episodes, {} images",
                        details.number_of_seasons.unwrap_or(0),
                        details.number_of_episodes.unwrap_or(0),
                        details.images.posters.len() + details.images.backdrops.len() + details.images.logos.len()
                    );
                }
                MediaDetailsOption::Endpoint(_) => {
                    warn!("Series {} only has endpoint URL, not full metadata", id);
                }
                _ => {
                    error!("Series {} has wrong metadata type", id);
                }
            }
            
            Ok(MediaReference::Series(series_ref))
        }
        Err(e) => Err(e),
    }
}

/// Helper function to fetch a season reference
async fn fetch_season_reference(
    state: &AppState,
    id: &str,
) -> Result<MediaReference, MediaError> {
    let season_id = SeasonID::new(id.to_string())?;
    
    match state.db.backend().get_season_reference(&season_id).await {
        Ok(season_ref) => {
            info!(
                "Found season {} of series {}",
                season_ref.season_number.value(),
                season_ref.series_id.as_str()
            );
            
            // Ensure we have full metadata
            match &season_ref.details {
                MediaDetailsOption::Details(TmdbDetails::Season(details)) => {
                    info!(
                        "Season has full TMDB metadata - {} episodes",
                        details.episode_count
                    );
                }
                MediaDetailsOption::Endpoint(_) => {
                    warn!("Season {} only has endpoint URL, not full metadata", id);
                }
                _ => {
                    error!("Season {} has wrong metadata type", id);
                }
            }
            
            Ok(MediaReference::Season(season_ref))
        }
        Err(e) => Err(e),
    }
}

/// Helper function to fetch an episode reference
async fn fetch_episode_reference(
    state: &AppState,
    id: &str,
) -> Result<MediaReference, MediaError> {
    let episode_id = EpisodeID::new(id.to_string())?;
    
    match state.db.backend().get_episode_reference(&episode_id).await {
        Ok(episode_ref) => {
            info!(
                "Found episode S{:02}E{:02} of series {}",
                episode_ref.season_number.value(),
                episode_ref.episode_number.value(),
                episode_ref.series_id.as_str()
            );
            
            // Ensure we have full metadata
            match &episode_ref.details {
                MediaDetailsOption::Details(TmdbDetails::Episode(details)) => {
                    info!(
                        "Episode has full TMDB metadata - runtime: {} minutes",
                        details.runtime.unwrap_or(0)
                    );
                }
                MediaDetailsOption::Endpoint(_) => {
                    warn!("Episode {} only has endpoint URL, not full metadata", id);
                }
                _ => {
                    error!("Episode {} has wrong metadata type", id);
                }
            }
            
            Ok(MediaReference::Episode(episode_ref))
        }
        Err(e) => Err(e),
    }
}

/// Batch endpoint to fetch multiple media references at once
pub async fn get_media_batch_handler(
    State(state): State<AppState>,
    Json(request): Json<BatchMediaRequest>,
) -> Result<Json<ApiResponse<BatchMediaResponse>>, StatusCode> {
    info!(
        "Fetching batch of {} media items for library {}",
        request.media_ids.len(),
        request.library_id
    );

    // Group media IDs by type for bulk fetching
    let mut movie_ids = Vec::new();
    let mut series_ids = Vec::new();
    let mut season_ids = Vec::new();
    let mut episode_ids = Vec::new();
    let mut person_ids = Vec::new();
    
    for media_id in &request.media_ids {
        match media_id {
            MediaId::Movie(id) => movie_ids.push(id),
            MediaId::Series(id) => series_ids.push(id),
            MediaId::Season(id) => season_ids.push(id),
            MediaId::Episode(id) => episode_ids.push(id),
            MediaId::Person(id) => person_ids.push(id),
        }
    }
    
    // Execute bulk queries in parallel using tokio::join!
    let (movies_result, series_result, seasons_result, episodes_result) = tokio::join!(
        state.db.backend().get_movie_references_bulk(&movie_ids),
        state.db.backend().get_series_references_bulk(&series_ids),
        state.db.backend().get_season_references_bulk(&season_ids),
        state.db.backend().get_episode_references_bulk(&episode_ids)
    );
    
    let mut items = Vec::new();
    let mut errors = Vec::new();
    
    // Process movie results
    match movies_result {
        Ok(movies) => {
            for movie in movies {
                items.push(MediaReference::Movie(movie));
            }
        }
        Err(e) => {
            error!("Failed to fetch movies in bulk: {}", e);
            for id in movie_ids {
                errors.push((MediaId::Movie(id.clone()), e.to_string()));
            }
        }
    }
    
    // Process series results
    match series_result {
        Ok(series_list) => {
            for series in series_list {
                items.push(MediaReference::Series(series));
            }
        }
        Err(e) => {
            error!("Failed to fetch series in bulk: {}", e);
            for id in series_ids {
                errors.push((MediaId::Series(id.clone()), e.to_string()));
            }
        }
    }
    
    // Process season results
    match seasons_result {
        Ok(seasons) => {
            for season in seasons {
                items.push(MediaReference::Season(season));
            }
        }
        Err(e) => {
            error!("Failed to fetch seasons in bulk: {}", e);
            for id in season_ids {
                errors.push((MediaId::Season(id.clone()), e.to_string()));
            }
        }
    }
    
    // Process episode results
    match episodes_result {
        Ok(episodes) => {
            for episode in episodes {
                items.push(MediaReference::Episode(episode));
            }
        }
        Err(e) => {
            error!("Failed to fetch episodes in bulk: {}", e);
            for id in episode_ids {
                errors.push((MediaId::Episode(id.clone()), e.to_string()));
            }
        }
    }
    
    // Handle person IDs (not yet implemented)
    for id in person_ids {
        errors.push((
            MediaId::Person(id.clone()),
            "Person references not yet implemented".to_string()
        ));
    }

    info!(
        "Batch fetch complete: {} successful, {} errors. Bulk queries reduced {} sequential queries to 4 parallel queries",
        items.len(),
        errors.len(),
        request.media_ids.len()
    );

    Ok(Json(ApiResponse::success(BatchMediaResponse {
        items,
        errors,
    })))
}