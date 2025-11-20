use crate::AppState;
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use ferrex_core::LibraryType;
use ferrex_core::ManualMatchRequest;
use ferrex_core::Media;
use ferrex_core::MediaDetailsOption;
use ferrex_core::MediaEvent;
use ferrex_core::MediaIDLike;
use ferrex_core::ParsedMediaInfo;
use ferrex_core::TmdbDetails;
use ferrex_core::indices::IndexManager;
use ferrex_core::query::types::{SortBy, SortOrder};
use ferrex_core::{
    database::traits::MediaFilters, ApiResponse, CreateLibraryRequest, EnhancedMovieDetails,
    EnhancedSeriesDetails, FetchMediaRequest, Library, LibraryID, LibraryMediaResponse,
    LibraryReference, MediaID, UpdateLibraryRequest,
};
use ferrex_core::{FilterIndicesRequest, IndicesResponse};
use rkyv::rancor::Error as RkyvError;
use serde::Deserialize;
use sqlx::Row;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info, warn};
use uuid::Uuid;

pub async fn get_library_media_util(
    state: &AppState,
    library: LibraryReference,
) -> Result<LibraryMediaResponse, StatusCode> {
    let media = match state
        .db
        .backend()
        .get_library_media_references(library.id, library.library_type)
        .await
    {
        Ok(media) => media,
        Err(e) => {
            warn!("Failed to get library movies: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    Ok(LibraryMediaResponse { library, media })
}

/// Get all references for a library (lightweight, no TMDB metadata)
pub async fn get_library_media_handler(
    State(state): State<AppState>,
    Path(library_id): Path<Uuid>,
) -> impl IntoResponse {
    info!("Getting media references for library: {}", library_id);

    // Get library reference
    let library = match state.db.backend().get_library_reference(library_id).await {
        Ok(lib) => lib,
        Err(e) => {
            error!("Failed to get library reference: {}", e);
            return Err(StatusCode::NOT_FOUND);
        }
    };

    let response = get_library_media_util(&state, library).await?;

    info!(
        "Found {} media items for library {}",
        response.media.len(),
        library_id
    );

    // Serialize to rkyv format
    match rkyv::to_bytes::<rkyv::rancor::Error>(&response) {
        Ok(bytes) => Ok::<_, StatusCode>(Bytes::from(bytes.into_vec())),
        Err(e) => {
            error!("Failed to serialize response with rkyv: {:?}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn get_libraries_with_media_handler(State(state): State<AppState>) -> impl IntoResponse {
    match state.db.backend().list_library_references().await {
        Ok(libraries) => {
            let mut library_results = Vec::new();
            for library_ref in libraries {
                let library = state
                    .db
                    .backend()
                    .get_library(&library_ref.id)
                    .await
                    .map_err(|e| {
                        error!("Failed to get library: {}", e);
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?;
                let library_media_response = get_library_media_util(&state, library_ref).await?;
                if let Some(mut library) = library {
                    library.media = Some(library_media_response.media);
                    library_results.push(library);
                }
            }
            let library_responses: Vec<_> = library_results.into_iter().collect::<Vec<_>>();

            // Serialize to rkyv format
            match rkyv::to_bytes::<rkyv::rancor::Error>(&library_responses) {
                Ok(bytes) => Ok::<_, StatusCode>(Bytes::from(bytes.into_vec())),
                Err(e) => {
                    error!("Failed to serialize response with rkyv: {:?}", e);
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
        }
        Err(e) => {
            error!("Failed to get libraries: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SortedIdsQuery {
    pub sort: Option<String>,
    pub order: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

fn parse_sort_field(s: &str) -> Option<SortBy> {
    match s.to_lowercase().as_str() {
        "title" => Some(SortBy::Title),
        "date_added" | "added" => Some(SortBy::DateAdded),
        "release_date" | "year" => Some(SortBy::ReleaseDate),
        "rating" => Some(SortBy::Rating),
        "popularity" => Some(SortBy::Popularity),
        "runtime" | "duration" => Some(SortBy::Runtime),
        "file_size" | "size" => Some(SortBy::FileSize),
        "resolution" => Some(SortBy::Resolution),
        "bitrate" => Some(SortBy::Bitrate),
        _ => None,
    }
}

fn parse_sort_order(s: &str) -> Option<SortOrder> {
    match s.to_lowercase().as_str() {
        "asc" | "ascending" => Some(SortOrder::Ascending),
        "desc" | "descending" => Some(SortOrder::Descending),
        _ => None,
    }
}

/// Get presorted media indices for a library (movie libraries supported)
pub async fn get_library_sorted_indices_handler(
    State(state): State<AppState>,
    Path(library_id): Path<Uuid>,
    Query(params): Query<SortedIdsQuery>,
) -> impl IntoResponse {
    info!("Getting presorted IDs for library: {}", library_id);

    // Lookup library reference to get library type
    let library_ref = match state.db.backend().get_library_reference(library_id).await {
        Ok(lib) => lib,
        Err(e) => {
            error!("Failed to get library reference: {}", e);
            return Err(StatusCode::NOT_FOUND);
        }
    };

    // Map sort and order with sensible defaults (default: title asc)
    let sort_field = params
        .sort
        .as_deref()
        .and_then(parse_sort_field)
        .unwrap_or(SortBy::Title);
    let sort_order = params
        .order
        .as_deref()
        .and_then(parse_sort_order)
        .unwrap_or(SortOrder::Ascending);

    let _offset = params.offset.unwrap_or(0);
    let _limit = params.limit.unwrap_or(60).min(500);

    // Only support Movie libraries initially; return 501 for others
    let lib_type = library_ref.library_type;
    if lib_type != LibraryType::Movies {
        warn!(
            "Sorted IDs endpoint currently supports movies only; library {:?} not supported",
            lib_type
        );
        return Err(StatusCode::NOT_IMPLEMENTED);
    }

    // Build sorted IDs using core index manager (in-memory sort for now)
    let index_mgr = IndexManager::new(state.db.clone());
    let sorted_ids = match index_mgr
        .sort_media_ids_for_library(library_ref.id, lib_type, sort_field, sort_order)
        .await
    {
        Ok(ids) => ids,
        Err(e) => {
            error!("Failed to sort media for library {}: {}", library_id, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Persist indices for static sorts for faster future reuse
    // Note: persistence of indices is removed in Phase 1 simplified design

    // Build base-order map (by title asc) to translate UUID -> position
    let pool = state
        .db
        .backend()
        .as_any()
        .downcast_ref::<ferrex_core::database::postgres::PostgresDatabase>()
        .expect("Postgres backend")
        .pool();
    let base_ids: Vec<Uuid> = match sqlx::query_scalar!(
        r#"SELECT mr.id FROM movie_references mr WHERE mr.library_id=$1 ORDER BY mr.title"#,
        library_ref.id.as_uuid()
    )
    .fetch_all(pool)
    .await
    {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to get base order for library {}: {}", library_id, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    let mut pos_map = HashMap::with_capacity(base_ids.len());
    for (i, id) in base_ids.iter().enumerate() {
        pos_map.insert(*id, i as u32);
    }

    let mut positions: Vec<u32> = Vec::with_capacity(sorted_ids.len());
    for id in sorted_ids.into_iter() {
        if let Some(p) = pos_map.get(&id) {
            positions.push(*p);
        }
    }

    // Compose rkyv response
    let response = IndicesResponse {
        content_version: 1,
        indices: positions,
    };

    match rkyv::to_bytes::<RkyvError>(&response) {
        Ok(bytes) => Ok::<_, StatusCode>((
            [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
            Bytes::from(bytes.into_vec()),
        )),
        Err(e) => {
            error!("Failed to serialize indices response: {:?}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Filter indices (movies Phase 1)
pub async fn post_library_filtered_indices_handler(
    State(state): State<AppState>,
    Path(library_id): Path<Uuid>,
    Json(spec): Json<FilterIndicesRequest>,
) -> impl IntoResponse {
    info!("Getting filtered indices for library: {}", library_id);

    // Resolve library
    let library_ref = match state.db.backend().get_library_reference(library_id).await {
        Ok(lib) => lib,
        Err(e) => {
            error!("Failed to get library reference: {}", e);
            return Err(StatusCode::NOT_FOUND);
        }
    };

    if library_ref.library_type != LibraryType::Movies {
        warn!("Filtered indices currently supports movies only");
        return Err(StatusCode::NOT_IMPLEMENTED);
    }

    // Build base order map (title asc) for positions
    let pool = state
        .db
        .backend()
        .as_any()
        .downcast_ref::<ferrex_core::database::postgres::PostgresDatabase>()
        .expect("Postgres backend")
        .pool();
    let base_ids: Vec<Uuid> = match sqlx::query_scalar!(
        r#"SELECT mr.id FROM movie_references mr WHERE mr.library_id=$1 ORDER BY mr.title"#,
        library_ref.id.as_uuid()
    )
    .fetch_all(pool)
    .await
    {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to get base order for library {}: {}", library_id, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    let mut pos_map = HashMap::with_capacity(base_ids.len());
    for (i, id) in base_ids.iter().enumerate() {
        pos_map.insert(*id, i as u32);
    }

    // Build filtered id list using optimized SQL inlined here (Phase 1: genres, year_range, rating_range, search, sort)
    use sqlx::{Postgres, QueryBuilder};
    let mut qb = QueryBuilder::<Postgres>::new(
        r#"
        SELECT mr.id
        FROM movie_references mr
        JOIN media_files mf ON mr.file_id = mf.id
        LEFT JOIN movie_metadata mm ON mr.id = mm.movie_id
        WHERE mr.library_id =
        "#,
    );
    qb.push_bind(library_ref.id.as_uuid());

    // genres filter
    if !spec.genres.is_empty() {
        qb.push(" AND mm.genre_names && ");
        qb.push_bind(&spec.genres);
    }

    // year range
    if let Some((min_y, max_y)) = spec.year_range {
        qb.push(" AND mm.release_year BETWEEN ");
        qb.push_bind(min_y as i32);
        qb.push(" AND ");
        qb.push_bind(max_y as i32);
    }

    // rating range
    if let Some((min_r, max_r)) = spec.rating_range {
        use std::str::FromStr;
        qb.push(" AND mm.vote_average BETWEEN ");
        qb.push_bind(sqlx::types::BigDecimal::from_str(&min_r.to_string()).unwrap());
        qb.push(" AND ");
        qb.push_bind(sqlx::types::BigDecimal::from_str(&max_r.to_string()).unwrap());
    }

    // search (ILIKE simple)
    if let Some(text) = &spec.search {
        qb.push(" AND (mr.title ILIKE ");
        qb.push_bind(format!("%{}%", text));
        qb.push(" OR mm.overview ILIKE ");
        qb.push_bind(format!("%{}%", text));
        qb.push(")");
    }

    // sorting
    let sort_field = spec.sort.unwrap_or(SortBy::Title);
    let sort_order = spec.order.unwrap_or(SortOrder::Ascending);
    qb.push(" ORDER BY ");
    qb.push(match sort_field {
        SortBy::Title => "LOWER(mr.title)",
        SortBy::DateAdded => "mf.created_at",
        SortBy::ReleaseDate => "mm.release_date",
        SortBy::Rating => "mm.vote_average",
        SortBy::Runtime => "mm.runtime",
        SortBy::Popularity => "mm.popularity",
        _ => "LOWER(mr.title)",
    });
    qb.push(match sort_order {
        SortOrder::Ascending => " ASC NULLS LAST",
        SortOrder::Descending => " DESC NULLS LAST",
    });

    let rows = match qb.build().fetch_all(pool).await {
        Ok(rows) => rows,
        Err(e) => {
            error!("Failed to execute filtered indices query: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Map ids -> positions
    let mut positions: Vec<u32> = Vec::with_capacity(rows.len());
    for row in rows {
        let id: Uuid = row.get("id");
        if let Some(p) = pos_map.get(&id) {
            positions.push(*p);
        }
    }

    let response = IndicesResponse {
        content_version: 1,
        indices: positions,
    };
    match rkyv::to_bytes::<RkyvError>(&response) {
        Ok(bytes) => Ok::<_, StatusCode>((
            [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
            Bytes::from(bytes.into_vec()),
        )),
        Err(e) => {
            error!("Failed to serialize indices response: {:?}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Fetch a specific media item with full metadata from database
/// If metadata is missing (MediaDetailsOption::Endpoint), fetches from TMDB on-demand
pub async fn fetch_media_handler(
    State(state): State<AppState>,
    Json(request): Json<FetchMediaRequest>,
) -> Result<Json<ApiResponse<Media>>, StatusCode> {
    info!(
        "Fetching media: {:?} from library {}",
        request.media_id, request.library_id
    );

    match request.media_id {
        MediaID::Movie(id) => {
            match state.db.backend().get_movie_reference(&id).await {
                Ok(mut movie) => {
                    // Check if we need to fetch metadata from TMDB
                    if matches!(movie.details, MediaDetailsOption::Endpoint(_)) {
                        //info!("Movie {} has endpoint URL, fetching TMDB metadata", id);

                        // Get the associated media file to extract TMDB ID
                        if let Ok(Some(media_file)) =
                            state.db.backend().get_media(id.as_uuid()).await
                        {
                            if let Some(metadata) = &media_file.media_file_metadata {
                                if let Some(parsed) = &metadata.parsed_info {
                                    if let ParsedMediaInfo::Movie(movie_info) = parsed {
                                        // Search TMDB for the movie
                                        if let Some(tmdb_provider) =
                                            &state.metadata_service.tmdb_provider
                                        {
                                            match tmdb_provider
                                                .search_movies(
                                                    &movie_info.title,
                                                    movie_info.year.map(|y| y),
                                                )
                                                .await
                                            {
                                                Ok(results) if !results.is_empty() => {
                                                    let tmdb_id = results[0].tmdb_id;

                                                    // Get full details from TMDB
                                                    match tmdb_provider.get_movie(tmdb_id).await {
                                                        Ok(details) => {
                                                            // Update the movie with full details
                                                            movie.details =
                                                                MediaDetailsOption::Details(
                                                                    TmdbDetails::Movie(
                                                                        EnhancedMovieDetails {
                                                                            id: details.inner.id,
                                                                            title: details
                                                                                .inner
                                                                                .title
                                                                                .clone(),
                                                                            overview: Some(
                                                                                details
                                                                                    .inner
                                                                                    .overview
                                                                                    .clone(),
                                                                            ),
                                                                            release_date: details
                                                                                .inner
                                                                                .release_date
                                                                                .map(|d| {
                                                                                    d.to_string()
                                                                                }),
                                                                            runtime: None, // Not available in MovieBase
                                                                            vote_average: Some(
                                                                                details
                                                                                    .inner
                                                                                    .vote_average
                                                                                    as f32,
                                                                            ),
                                                                            vote_count: Some(
                                                                                details
                                                                                    .inner
                                                                                    .vote_count
                                                                                    as u32,
                                                                            ),
                                                                            popularity: Some(
                                                                                details
                                                                                    .inner
                                                                                    .popularity
                                                                                    as f32,
                                                                            ),
                                                                            genres: vec![], // Not available in MovieBase
                                                                            production_companies: vec![], // Not available in MovieBase
                                                                            poster_path: details
                                                                                .inner
                                                                                .poster_path
                                                                                .clone(),
                                                                            backdrop_path: details
                                                                                .inner
                                                                                .backdrop_path
                                                                                .clone(),
                                                                            logo_path: None,
                                                                            images:
                                                                                Default::default(),
                                                                            cast: vec![],
                                                                            crew: vec![],
                                                                            videos: vec![],
                                                                            keywords: vec![],
                                                                            external_ids:
                                                                                Default::default(),
                                                                        },
                                                                    ),
                                                                );

                                                            // Update in database
                                                            let _ = state
                                                                .db
                                                                .backend()
                                                                .store_movie_reference(&movie)
                                                                .await;

                                                            let mut buff = Uuid::encode_buffer();

                                                            info!("Successfully fetched and stored TMDB metadata for movie {}", id.as_str(&mut buff));
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

                    Ok(Json(ApiResponse::success(Media::Movie(movie))))
                }
                Err(e) => {
                    error!("Failed to get movie reference: {}", e);
                    Ok(Json(ApiResponse::error(e.to_string())))
                }
            }
        }
        MediaID::Series(id) => {
            match state.db.backend().get_series_reference(&id).await {
                Ok(mut series) => {
                    // Check if we need to fetch metadata from TMDB
                    if matches!(series.details, MediaDetailsOption::Endpoint(_)) {
                        let mut buff = Uuid::encode_buffer();
                        info!(
                            "Series {} has endpoint URL, fetching TMDB metadata",
                            id.as_str(&mut buff)
                        );

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
                                            if let Some(tmdb_provider) =
                                                &state.metadata_service.tmdb_provider
                                            {
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
                                                                id: details.inner.id,
                                                                name: details.inner.name.clone(),
                                                                overview: details.inner.overview.clone(),
                                                                first_air_date: details.inner.first_air_date.map(|d| d.to_string()),
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

                                                                let mut buff =
                                                                    Uuid::encode_buffer();
                                                                info!("Successfully fetched and stored TMDB metadata for series {}", id.as_str(&mut buff));
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

                    Ok(Json(ApiResponse::success(Media::Series(series))))
                }
                Err(e) => {
                    error!("Failed to get series reference: {}", e);
                    Ok(Json(ApiResponse::error(e.to_string())))
                }
            }
        }
        MediaID::Season(id) => {
            match state.db.backend().get_season_reference(&id).await {
                Ok(season) => {
                    // TODO: Implement on-demand season metadata fetching if needed
                    Ok(Json(ApiResponse::success(Media::Season(season))))
                }
                Err(e) => {
                    error!("Failed to get season reference: {}", e);
                    Ok(Json(ApiResponse::error(e.to_string())))
                }
            }
        }
        MediaID::Episode(id) => {
            match state.db.backend().get_episode_reference(&id).await {
                Ok(episode) => {
                    // TODO: Implement on-demand episode metadata fetching if needed
                    Ok(Json(ApiResponse::success(Media::Episode(episode))))
                }
                Err(e) => {
                    error!("Failed to get episode reference: {}", e);
                    Ok(Json(ApiResponse::error(e.to_string())))
                }
            }
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
        MediaID::Movie(id) => {
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
        MediaID::Series(id) => {
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

    let library_id = LibraryID::new();
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
            match state
                .folder_monitor
                .discover_library_folders_immediate(&library.id)
                .await
            { Err(e) => {
                warn!(
                    "Failed to trigger immediate folder discovery for library {}: {}",
                    id, e
                );
                // Continue anyway - folder discovery will happen in the next scheduled cycle
            } _ => {
                info!("Immediate folder discovery triggered for library {}", id);

                // Trigger an immediate scan for the newly created library after folder discovery
                info!("Triggering immediate scan for newly created library {}", id);
                match state
                    .scan_manager
                    .start_library_scan(Arc::new(vec![library.clone()]), false)
                    .await
                {
                    Ok(scan_id) => {
                        info!(
                            "Immediate scan started for library {} with scan ID: {}",
                            id, scan_id
                        );
                    }
                    Err(e) => {
                        warn!("Failed to trigger immediate scan for library {}: {}", id, e);
                        // Continue anyway - scan can be triggered manually or will happen on schedule
                    }
                }
            }}

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
    Path(id): Path<String>, // TODO: Use LibraryID directly
    Json(request): Json<UpdateLibraryRequest>,
) -> Result<Json<ApiResponse<String>>, StatusCode> {
    info!("Updating library: {}", id);

    // Get the existing library
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let mut library = match state.db.backend().get_library(&LibraryID(uuid)).await {
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
