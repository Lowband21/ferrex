use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
};
use ferrex_core::{
    database::traits::MediaFilters, media::{EnhancedMovieDetails, EnhancedSeriesDetails},
    ApiResponse, CreateLibraryRequest, FetchMediaRequest, Library, LibraryMediaResponse,
    LibraryReference, ManualMatchRequest, MediaDetailsOption, MediaEvent, MediaId, MediaReference,
    ParsedMediaInfo, TmdbDetails, UpdateLibraryRequest,
};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info, warn};
use uuid::Uuid;

/// Get all references for a library (lightweight, no TMDB metadata)
pub async fn get_library_media_handler(
    State(state): State<AppState>,
    Path(library_id): Path<Uuid>,
) -> Result<Json<ApiResponse<LibraryMediaResponse>>, StatusCode> {
    info!("Getting media references for library: {}", library_id);

    // Get library reference
    let library = match state.db.backend().get_library_reference(library_id).await {
        Ok(lib) => lib,
        Err(e) => {
            error!("Failed to get library reference: {}", e);
            return Ok(Json(ApiResponse::error(e.to_string())));
        }
    };

    // Get all media references for this library
    let mut media = Vec::new();

    // Get movies
    match state.db.backend().get_library_movies(library_id).await {
        Ok(movies) => {
            for movie in movies {
                media.push(MediaReference::Movie(movie));
            }
        }
        Err(e) => {
            warn!("Failed to get library movies: {}", e);
        }
    }

    // Get series with their seasons and episodes
    match state.db.backend().get_library_series(library_id).await {
        Ok(series_list) => {
            for series in series_list {
                let series_id = series.id.clone();
                media.push(MediaReference::Series(series.clone()));

                // Get seasons for this series
                match state.db.backend().get_series_seasons(&series_id).await {
                    Ok(seasons) => {
                        info!("Found {} seasons for series {} ({})", 
                              seasons.len(), 
                              series.title.as_str(),
                              series_id.as_str());
                        for season in seasons {
                            let season_id = season.id.clone();
                            info!("Adding season {} (S{}) for series {} to media references", 
                                  season_id.as_str(),
                                  season.season_number.value(),
                                  series.title.as_str());
                            
                            // DEBUG: Verify series_id matches
                            if season.series_id != series.id {
                                error!(
                                    "SERIES_ID MISMATCH! Season {} has series_id {} but belongs to series {}",
                                    season_id.as_str(),
                                    season.series_id.as_str(),
                                    series.id.as_str()
                                );
                            } else {
                                info!(
                                    "Season {} correctly has series_id {} matching series",
                                    season_id.as_str(),
                                    season.series_id.as_str()
                                );
                            }
                            
                            media.push(MediaReference::Season(season.clone()));

                            // Get episodes for this season
                            match state.db.backend().get_season_episodes(&season_id).await {
                                Ok(episodes) => {
                                    for episode in episodes {
                                        media.push(MediaReference::Episode(episode));
                                    }
                                }
                                Err(e) => {
                                    warn!("Failed to get season episodes: {}", e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to get series seasons: {}", e);
                    }
                }
            }
        }
        Err(e) => {
            warn!("Failed to get library series: {}", e);
        }
    }

    info!(
        "Found {} media items for library {}",
        media.len(),
        library_id
    );

    Ok(Json(ApiResponse::success(LibraryMediaResponse {
        library,
        media,
    })))
}

/// Fetch a specific media item with full metadata from database
/// If metadata is missing (MediaDetailsOption::Endpoint), fetches from TMDB on-demand
pub async fn fetch_media_handler(
    State(state): State<AppState>,
    Json(request): Json<FetchMediaRequest>,
) -> Result<Json<ApiResponse<MediaReference>>, StatusCode> {
    info!(
        "Fetching media: {:?} from library {}",
        request.media_id, request.library_id
    );

    match request.media_id {
        MediaId::Movie(id) => {
            match state.db.backend().get_movie_reference(&id).await {
                Ok(mut movie) => {
                    // Check if we need to fetch metadata from TMDB
                    if matches!(movie.details, MediaDetailsOption::Endpoint(_)) {
                        //info!("Movie {} has endpoint URL, fetching TMDB metadata", id);

                        // Get the associated media file to extract TMDB ID
                        if let Ok(Some(media_file)) =
                            state.db.backend().get_media(id.as_str()).await
                        {
                            if let Some(metadata) = &media_file.media_file_metadata {
                                if let Some(parsed) = &metadata.parsed_info {
                                    if let ParsedMediaInfo::Movie(movie_info) = parsed {
                                        // Search TMDB for the movie
                                        if let Some(tmdb_provider) = &state.metadata_service.tmdb_provider {
                                            match tmdb_provider
                                                .search_movies(&movie_info.title, movie_info.year.map(|y| y as u16))
                                                .await
                                        {
                                            Ok(results) if !results.is_empty() => {
                                                let tmdb_id = results[0].tmdb_id;

                                                // Get full details from TMDB
                                                match tmdb_provider
                                                    .get_movie(tmdb_id)
                                                    .await
                                                {
                                                    Ok(details) => {
                                                        // Update the movie with full details
                                                        movie.details = MediaDetailsOption::Details(
                                                            TmdbDetails::Movie(EnhancedMovieDetails {
                                                                id: details.inner.id as u64,
                                                                title: details.inner.title.clone(),
                                                                overview: Some(details.inner.overview.clone()),
                                                                release_date: details.inner.release_date.clone().map(|d| d.to_string()),
                                                                runtime: None, // Not available in MovieBase
                                                                vote_average: Some(details.inner.vote_average as f32),
                                                                vote_count: Some(details.inner.vote_count as u32),
                                                                popularity: Some(details.inner.popularity as f32),
                                                                genres: vec![], // Not available in MovieBase
                                                                production_companies: vec![], // Not available in MovieBase
                                                                poster_path: details.inner.poster_path.clone(),
                                                                backdrop_path: details.inner.backdrop_path.clone(),
                                                                logo_path: None,
                                                                images: Default::default(),
                                                                cast: vec![],
                                                                crew: vec![],
                                                                videos: vec![],
                                                                keywords: vec![],
                                                                external_ids: Default::default(),
                                                            }),
                                                        );

                                                        // Update in database
                                                        let _ = state
                                                            .db
                                                            .backend()
                                                            .store_movie_reference(&movie)
                                                            .await;

                                                        info!("Successfully fetched and stored TMDB metadata for movie {}", id.as_str());
                                                    }
                                                    Err(e) => {
                                                        warn!("Failed to get movie details from TMDB: {}", e);
                                                    }
                                                }
                                            }
                                            Ok(_) => {
                                                warn!(
                                                    "No TMDB results found for movie: {}",
                                                    movie_info.title
                                                );
                                            }
                                            Err(e) => {
                                                warn!("Failed to search TMDB for movie: {}", e);
                                            }
                                        }
                                        } else {
                                            warn!("TMDB provider not configured");
                                        }
                                    }
                                }
                            }
                        }
                    }

                    Ok(Json(ApiResponse::success(MediaReference::Movie(movie))))
                }
                Err(e) => {
                    error!("Failed to get movie reference: {}", e);
                    Ok(Json(ApiResponse::error(e.to_string())))
                }
            }
        }
        MediaId::Series(id) => {
            match state.db.backend().get_series_reference(&id).await {
                Ok(mut series) => {
                    // Check if we need to fetch metadata from TMDB
                    if matches!(series.details, MediaDetailsOption::Endpoint(_)) {
                        info!("Series {} has endpoint URL, fetching TMDB metadata", id.as_str());

                        // Get any associated media file to extract show name
                        let filters = MediaFilters {
                            show_name: Some(series.title.as_str().to_string()),
                            media_type: Some("tvepisode".to_string()),
                            limit: Some(1),
                            ..Default::default()
                        };

                        if let Ok(files) = state.db.backend().list_media(filters).await {
                            if let Some(media_file) = files.first() {
                                if let Some(metadata) = &media_file.media_file_metadata {
                                    if let Some(parsed) = &metadata.parsed_info {
                                        if let ParsedMediaInfo::Episode(episode_info) = parsed {
                                            // Search TMDB for the series
                                            if let Some(tmdb_provider) = &state.metadata_service.tmdb_provider {
                                                match tmdb_provider
                                                    .search_series(&episode_info.show_name)
                                                    .await
                                            {
                                                Ok(results) if !results.is_empty() => {
                                                    let tmdb_id = results[0].tmdb_id;

                                                    // Get full details from TMDB
                                                    match tmdb_provider
                                                        .get_series(tmdb_id)
                                                        .await
                                                    {
                                                        Ok(details) => {
                                                            // Update the series with full details
                                                            series.details = MediaDetailsOption::Details(TmdbDetails::Series(EnhancedSeriesDetails {
                                                                id: details.inner.id as u64,
                                                                name: details.inner.name.clone(),
                                                                overview: details.inner.overview.clone(),
                                                                first_air_date: details.inner.first_air_date.clone().map(|d| d.to_string()),
                                                                last_air_date: None, // Not available in TVShowBase
                                                                number_of_seasons: None, // Not available in TVShowBase
                                                                number_of_episodes: None, // Not available in TVShowBase
                                                                vote_average: Some(details.inner.vote_average as f32),
                                                                vote_count: Some(details.inner.vote_count as u32),
                                                                popularity: Some(details.inner.popularity as f32),
                                                                genres: vec![], // Not available in TVShowBase
                                                                networks: vec![], // Not available in TVShowBase
                                                                poster_path: details.inner.poster_path.clone(),
                                                                backdrop_path: details.inner.backdrop_path.clone(),
                                                                logo_path: None,
                                                                images: Default::default(),
                                                                cast: vec![],
                                                                crew: vec![],
                                                                videos: vec![],
                                                                keywords: vec![],
                                                                external_ids: Default::default(),
                                                            }));

                                                            // Update in database
                                                            let _ = state
                                                                .db
                                                                .backend()
                                                                .store_series_reference(&series)
                                                                .await;

                                                            info!("Successfully fetched and stored TMDB metadata for series {}", id.as_str());
                                                        }
                                                        Err(e) => {
                                                            warn!("Failed to get series details from TMDB: {}", e);
                                                        }
                                                    }
                                                }
                                                Ok(_) => {
                                                    warn!(
                                                        "No TMDB results found for series: {}",
                                                        episode_info.show_name
                                                    );
                                                }
                                                Err(e) => {
                                                    warn!(
                                                        "Failed to search TMDB for series: {}",
                                                        e
                                                    );
                                                }
                                            }
                                            } else {
                                                warn!("TMDB provider not configured");
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    Ok(Json(ApiResponse::success(MediaReference::Series(series))))
                }
                Err(e) => {
                    error!("Failed to get series reference: {}", e);
                    Ok(Json(ApiResponse::error(e.to_string())))
                }
            }
        }
        MediaId::Season(id) => {
            match state.db.backend().get_season_reference(&id).await {
                Ok(season) => {
                    // TODO: Implement on-demand season metadata fetching if needed
                    Ok(Json(ApiResponse::success(MediaReference::Season(season))))
                }
                Err(e) => {
                    error!("Failed to get season reference: {}", e);
                    Ok(Json(ApiResponse::error(e.to_string())))
                }
            }
        }
        MediaId::Episode(id) => {
            match state.db.backend().get_episode_reference(&id).await {
                Ok(episode) => {
                    // TODO: Implement on-demand episode metadata fetching if needed
                    Ok(Json(ApiResponse::success(MediaReference::Episode(episode))))
                }
                Err(e) => {
                    error!("Failed to get episode reference: {}", e);
                    Ok(Json(ApiResponse::error(e.to_string())))
                }
            }
        }
        MediaId::Person(_id) => {
            // Person references are not stored as media items
            // They are part of cast/crew data in movie/series metadata
            error!("Person references cannot be fetched as media items");
            Ok(Json(ApiResponse::error("Person references are not stored as separate media items".to_string())))
        }
    }
}

/// Manual TMDB matching for media items
pub async fn manual_match_media_handler(
    State(state): State<AppState>,
    Json(request): Json<ManualMatchRequest>,
) -> Result<Json<ApiResponse<String>>, StatusCode> {
    info!(
        "Manual match request: {:?} to TMDB ID {}",
        request.media_id, request.tmdb_id
    );

    match request.media_id {
        MediaId::Movie(id) => {
            match state
                .db
                .backend()
                .update_movie_tmdb_id(&id, request.tmdb_id)
                .await
            {
                Ok(_) => {
                    // Send update event
                    if let Ok(movie) = state.db.backend().get_movie_reference(&id).await {
                        state
                            .scan_manager
                            .send_media_event(MediaEvent::MovieUpdated { movie })
                            .await;
                    }
                    Ok(Json(ApiResponse::success(
                        "Movie TMDB ID updated".to_string(),
                    )))
                }
                Err(e) => {
                    error!("Failed to update movie TMDB ID: {}", e);
                    Ok(Json(ApiResponse::error(e.to_string())))
                }
            }
        }
        MediaId::Series(id) => {
            match state
                .db
                .backend()
                .update_series_tmdb_id(&id, request.tmdb_id)
                .await
            {
                Ok(_) => {
                    // Update all episodes in this series
                    // TODO: This should cascade to seasons and episodes

                    // Send update event
                    if let Ok(series) = state.db.backend().get_series_reference(&id).await {
                        state
                            .scan_manager
                            .send_media_event(MediaEvent::SeriesUpdated { series })
                            .await;
                    }
                    Ok(Json(ApiResponse::success(
                        "Series TMDB ID updated".to_string(),
                    )))
                }
                Err(e) => {
                    error!("Failed to update series TMDB ID: {}", e);
                    Ok(Json(ApiResponse::error(e.to_string())))
                }
            }
        }
        _ => Ok(Json(ApiResponse::error(
            "Manual matching only supported for movies and series".to_string(),
        ))),
    }
}

/// Get all libraries (without media references)
pub async fn list_libraries_handler(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<LibraryReference>>>, StatusCode> {
    info!("Listing all libraries");

    match state.db.backend().list_library_references().await {
        Ok(libraries) => {
            info!("Found {} libraries", libraries.len());
            Ok(Json(ApiResponse::success(libraries)))
        }
        Err(e) => {
            error!("Failed to list libraries: {}", e);
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

/// Get a specific library (without media references)
pub async fn get_library_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<LibraryReference>>, StatusCode> {
    info!("Getting library: {}", id);

    match state.db.backend().get_library_reference(id).await {
        Ok(library) => Ok(Json(ApiResponse::success(library))),
        Err(e) => {
            error!("Failed to get library: {}", e);
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

/// Create a new library
pub async fn create_library_handler(
    State(state): State<AppState>,
    Json(request): Json<CreateLibraryRequest>,
) -> Result<Json<ApiResponse<String>>, StatusCode> {
    info!("Creating new library: {}", request.name);

    let library_id = Uuid::new_v4();
    info!("Generated library ID: {}", library_id);

    let library = Library {
        id: library_id,
        name: request.name,
        library_type: request.library_type,
        paths: request.paths.into_iter().map(PathBuf::from).collect(),
        scan_interval_minutes: request.scan_interval_minutes,
        enabled: request.enabled,
        last_scan: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        media: None,
        auto_scan: true,
        watch_for_changes: true,
        analyze_on_scan: false,
        max_retry_attempts: 3,
    };

    info!(
        "Storing library with ID: {} and type: {:?}",
        library.id, library.library_type
    );

    match state.db.backend().create_library(library.clone()).await {
        Ok(id) => {
            info!("Library successfully created in database with ID: {}", id);
            
            // Update the FolderMonitor's library list to include the new library
            {
                let mut libraries = state.folder_monitor.libraries.write().await;
                libraries.push(library.clone());
            }
            
            // Trigger immediate folder discovery for the new library
            if let Err(e) = state.folder_monitor.discover_library_folders_immediate(&library.id).await {
                warn!("Failed to trigger immediate folder discovery for library {}: {}", id, e);
                // Continue anyway - folder discovery will happen in the next scheduled cycle
            } else {
                info!("Immediate folder discovery triggered for library {}", id);
                
                // Trigger an immediate scan for the newly created library after folder discovery
                info!("Triggering immediate scan for newly created library {}", id);
                match state.scan_manager.start_library_scan(Arc::new(library.clone()), false).await {
                    Ok(scan_id) => {
                        info!("Immediate scan started for library {} with scan ID: {}", id, scan_id);
                    }
                    Err(e) => {
                        warn!("Failed to trigger immediate scan for library {}: {}", id, e);
                        // Continue anyway - scan can be triggered manually or will happen on schedule
                    }
                }
            }
            
            Ok(Json(ApiResponse::success(id)))
        }
        Err(e) => {
            error!("Failed to create library: {}", e);
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

/// Update an existing library
pub async fn update_library_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<UpdateLibraryRequest>,
) -> Result<Json<ApiResponse<String>>, StatusCode> {
    info!("Updating library: {}", id);

    // Get the existing library
    let mut library = match state.db.backend().get_library(&id).await {
        Ok(Some(lib)) => lib,
        Ok(None) => {
            return Ok(Json(ApiResponse::error("Library not found".to_string())));
        }
        Err(e) => {
            error!("Failed to get library: {}", e);
            return Ok(Json(ApiResponse::error(e.to_string())));
        }
    };

    // Update fields if provided
    if let Some(name) = request.name {
        library.name = name;
    }
    if let Some(paths) = request.paths {
        library.paths = paths.into_iter().map(PathBuf::from).collect();
    }
    if let Some(scan_interval) = request.scan_interval_minutes {
        library.scan_interval_minutes = scan_interval;
    }
    if let Some(enabled) = request.enabled {
        library.enabled = enabled;
    }
    library.updated_at = chrono::Utc::now();

    match state.db.backend().update_library(&id, library).await {
        Ok(_) => {
            info!("Library updated: {}", id);
            Ok(Json(ApiResponse::success("Library updated".to_string())))
        }
        Err(e) => {
            error!("Failed to update library: {}", e);
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

/// Delete a library
pub async fn delete_library_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<String>>, StatusCode> {
    info!("Deleting library: {}", id);

    match state.db.backend().delete_library(&id).await {
        Ok(_) => {
            info!("Library deleted: {}", id);
            Ok(Json(ApiResponse::success("Library deleted".to_string())))
        }
        Err(e) => {
            error!("Failed to delete library: {}", e);
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}
