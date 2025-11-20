use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
};
use ferrex_core::{database::traits::MediaFilters, ParsedMediaInfo};
use serde_json::{json, Value};
use tracing::{info, warn};
use uuid::Uuid;

pub async fn list_movies_handler(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    info!("Listing all movies");

    // Get all movie files
    let filters = MediaFilters {
        media_type: Some("movie".to_string()), // Database stores lowercase "movie"
        ..Default::default()
    };

    match state.db.backend().list_media(filters).await {
        Ok(movies) => {
            // Transform movies into a cleaner format
            let movie_list: Vec<serde_json::Value> = movies
                .into_iter()
                .map(|movie| {
                    let metadata = movie.media_file_metadata.as_ref();
                    let parsed = metadata.and_then(|m| m.parsed_info.as_ref());

                    json!({
                        "id": movie.id,
                        "title": parsed.and_then(|p| match p {
                            ParsedMediaInfo::Movie(info) => Some(info.title.clone()),
                            _ => None
                        }).unwrap_or_else(|| movie.filename.clone()),
                        "year": parsed.and_then(|p| match p {
                            ParsedMediaInfo::Movie(info) => info.year,
                            _ => None
                        }),
                        // TMDB metadata should come from reference types, not MediaFile
                        "tmdb_id": null,
                        "description": null,
                        "poster_url": null,
                        "backdrop_url": null,
                        "rating": null,
                        "release_date": null,
                        "genres": Vec::<String>::new(),
                        "duration": metadata.and_then(|m| m.duration),
                        "file_path": movie.path.to_string_lossy(),
                        "file_size": movie.size,
                        "video_codec": metadata.and_then(|m| m.video_codec.clone()),
                        "audio_codec": metadata.and_then(|m| m.audio_codec.clone()),
                        "width": metadata.and_then(|m| m.width),
                        "height": metadata.and_then(|m| m.height),
                        // Poster paths should come from reference types
                        "poster_path": null
                    })
                })
                .collect();

            info!("Found {} movies", movie_list.len());
            Ok(Json(json!({
                "status": "success",
                "movies": movie_list,
                "count": movie_list.len()
            })))
        }
        Err(e) => {
            warn!("Failed to retrieve movies: {}", e);
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}

pub async fn movie_details_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, StatusCode> {
    info!("Getting details for movie ID: {}", id);

    match state.db.backend().get_media(&id).await {
        Ok(Some(movie)) => {
            // Verify it's a movie
            let is_movie = movie
                .media_file_metadata
                .as_ref()
                .and_then(|m| m.parsed_info.as_ref())
                .map(|p| matches!(p, ParsedMediaInfo::Movie(_)))
                .unwrap_or(false);

            if !is_movie {
                return Err(StatusCode::NOT_FOUND);
            }

            let metadata = movie.media_file_metadata.as_ref();
            // External info no longer exists in MediaFileMetadata
            let parsed = metadata.and_then(|m| m.parsed_info.as_ref());

            let movie_details = json!({
                "id": movie.id,
                "title": parsed.and_then(|p| match p {
                    ParsedMediaInfo::Movie(info) => Some(info.title.clone()),
                    _ => None
                }).unwrap_or_else(|| movie.filename.clone()),
                "year": parsed.and_then(|p| match p {
                    ParsedMediaInfo::Movie(info) => info.year,
                    _ => None
                }),
                // TMDB metadata should come from reference types, not MediaFile
                "tmdb_id": null,
                "imdb_id": null,
                "description": null,
                "poster_url": null,
                "backdrop_url": null,
                "rating": null,
                "release_date": null,
                "genres": Vec::<String>::new(),
                "duration": metadata.and_then(|m| m.duration),
                "file_path": movie.path.to_string_lossy(),
                "file_size": movie.size,
                "created_at": movie.created_at,
                "library_id": movie.library_id,
                "video_info": {
                    "codec": metadata.and_then(|m| m.video_codec.clone()),
                    "width": metadata.and_then(|m| m.width),
                    "height": metadata.and_then(|m| m.height),
                    "bitrate": metadata.and_then(|m| m.bitrate),
                    "framerate": metadata.and_then(|m| m.framerate),
                    "bit_depth": metadata.and_then(|m| m.bit_depth),
                    "color_transfer": metadata.and_then(|m| m.color_transfer.clone()),
                    "color_space": metadata.and_then(|m| m.color_space.clone()),
                    "color_primaries": metadata.and_then(|m| m.color_primaries.clone()),
                },
                "audio_info": {
                    "codec": metadata.and_then(|m| m.audio_codec.clone()),
                },
                // Poster paths should come from reference types
                "poster_path": null,
                "stream_url": format!("/stream/{}", movie.id),
                "transcode_url": format!("/stream/{}/transcode", movie.id)
            });

            Ok(Json(json!({
                "status": "success",
                "movie": movie_details
            })))
        }
        Ok(None) => {
            warn!("Movie not found: {}", id);
            Err(StatusCode::NOT_FOUND)
        }
        Err(e) => {
            warn!("Failed to retrieve movie details: {}", e);
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}
