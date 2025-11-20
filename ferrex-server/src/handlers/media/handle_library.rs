use axum::{
    body::Bytes,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use ferrex_core::LibraryActorConfig;
use ferrex_core::LibraryType;
use ferrex_core::Media;
use ferrex_core::MediaDetailsOption;
use ferrex_core::query::{
    filtering::hash_filter_spec,
    types::{SortBy, SortOrder},
};
use ferrex_core::user::User;
use ferrex_core::{
    ApiResponse, CreateLibraryRequest, FetchMediaRequest, Library, LibraryID, LibraryMediaResponse,
    LibraryReference, MediaID, UpdateLibraryRequest,
};
use ferrex_core::{FilterIndicesRequest, IndicesResponse};
use rkyv::rancor::Error as RkyvError;
use serde::Deserialize;
use sqlx::Row;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
    infra::app_state::AppState,
    media::index_filters::{FilterQueryError, build_filtered_movie_query},
};

use once_cell::sync::Lazy;
use parking_lot::RwLock;

const FILTER_CACHE_TTL: Duration = Duration::from_secs(30);

static FILTER_CACHE: Lazy<RwLock<HashMap<FilterCacheKey, CachedIndices>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FilterCacheKey {
    library_id: Uuid,
    spec_hash: u64,
    user_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
struct CachedIndices {
    indices: Vec<u32>,
    stored_at: Instant,
}

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
        "created_at" | "created" => Some(SortBy::CreatedAt),
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
    Extension(user): Extension<User>,
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

    // Build SQL that uses precomputed positions from movie_sort_positions
    let pool = state
        .db
        .backend()
        .as_any()
        .downcast_ref::<ferrex_core::database::postgres::PostgresDatabase>()
        .expect("Postgres backend")
        .pool();

    let (order_col, order_direction) = match (sort_field, sort_order) {
        (SortBy::Title, SortOrder::Ascending) => ("msp.title_pos", "ASC"),
        (SortBy::Title, SortOrder::Descending) => ("msp.title_pos_desc", "ASC"),
        (SortBy::DateAdded, SortOrder::Ascending) => ("msp.date_added_pos", "ASC"),
        (SortBy::DateAdded, SortOrder::Descending) => ("msp.date_added_pos_desc", "ASC"),
        (SortBy::CreatedAt, SortOrder::Ascending) => ("msp.created_at_pos", "ASC"),
        (SortBy::CreatedAt, SortOrder::Descending) => ("msp.created_at_pos_desc", "ASC"),
        (SortBy::ReleaseDate, SortOrder::Ascending) => ("msp.release_date_pos", "ASC"),
        (SortBy::ReleaseDate, SortOrder::Descending) => ("msp.release_date_pos_desc", "ASC"),
        (SortBy::Rating, SortOrder::Ascending) => ("msp.rating_pos", "ASC"),
        (SortBy::Rating, SortOrder::Descending) => ("msp.rating_pos_desc", "ASC"),
        (SortBy::Runtime, SortOrder::Ascending) => ("msp.runtime_pos", "ASC"),
        (SortBy::Runtime, SortOrder::Descending) => ("msp.runtime_pos_desc", "ASC"),
        (SortBy::Popularity, SortOrder::Ascending) => ("msp.popularity_pos", "ASC"),
        (SortBy::Popularity, SortOrder::Descending) => ("msp.popularity_pos_desc", "ASC"),
        (SortBy::Bitrate, SortOrder::Ascending) => ("msp.bitrate_pos", "ASC"),
        (SortBy::Bitrate, SortOrder::Descending) => ("msp.bitrate_pos_desc", "ASC"),
        (SortBy::FileSize, SortOrder::Ascending) => ("msp.file_size_pos", "ASC"),
        (SortBy::FileSize, SortOrder::Descending) => ("msp.file_size_pos_desc", "ASC"),
        (SortBy::ContentRating, SortOrder::Ascending) => ("msp.content_rating_pos", "ASC"),
        (SortBy::ContentRating, SortOrder::Descending) => ("msp.content_rating_pos_desc", "ASC"),
        (SortBy::Resolution, SortOrder::Ascending) => ("msp.resolution_pos", "ASC"),
        (SortBy::Resolution, SortOrder::Descending) => ("msp.resolution_pos_desc", "ASC"),
        // Fallback to title
        _ => ("msp.title_pos", "ASC"),
    };

    let mut qb = sqlx::QueryBuilder::new(
        "SELECT (msp.title_pos - 1)::INT4 AS idx FROM movie_sort_positions msp WHERE msp.library_id = ",
    );
    qb.push_bind(library_ref.id.as_uuid());
    qb.push(" ORDER BY ");
    qb.push(order_col);
    qb.push(" ");
    qb.push(order_direction);

    if let Some(offset) = params.offset {
        qb.push(" OFFSET ");
        qb.push_bind(offset as i64);
    }
    if let Some(limit) = params.limit {
        qb.push(" LIMIT ");
        qb.push_bind(limit as i64);
    }

    let rows = match qb.build().fetch_all(pool).await {
        Ok(rows) => rows,
        Err(e) => {
            error!(
                "Failed to query precomputed positions for library {}: {}",
                library_id, e
            );
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let mut positions = Vec::with_capacity(rows.len());
    for row in rows {
        let idx: i32 = row.get("idx");
        if idx >= 0 {
            positions.push(idx as u32);
        }
    }

    respond_with_indices(positions)
}

/// Filter indices (movies Phase 1)
pub async fn post_library_filtered_indices_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(library_id): Path<Uuid>,
    Json(spec): Json<FilterIndicesRequest>,
) -> impl IntoResponse {
    info!("Getting filtered indices for library: {}", library_id);

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

    let pool = state
        .db
        .backend()
        .as_any()
        .downcast_ref::<ferrex_core::database::postgres::PostgresDatabase>()
        .expect("Postgres backend")
        .pool();

    let library_uuid = library_ref.id.as_uuid();
    let user_scope = requires_user_scope(&spec).then_some(user.id);
    // Check short-lived in-process cache first
    let cache_key = FilterCacheKey {
        library_id: library_uuid,
        spec_hash: hash_filter_spec(&spec),
        user_id: user_scope,
    };
    if let Some(indices) = get_cached_indices(&cache_key) {
        return respond_with_indices(indices);
    }

    let mut qb = match build_filtered_movie_query(library_ref.id.as_uuid(), &spec, Some(user.id)) {
        Ok(builder) => builder,
        Err(err) => {
            warn!("Rejected filtered indices request: {}", err);
            return Err(match err {
                FilterQueryError::MissingUserContext(_) => StatusCode::BAD_REQUEST,
                FilterQueryError::UnsupportedMediaType(_) => StatusCode::NOT_IMPLEMENTED,
                FilterQueryError::InvalidNumeric(_) => StatusCode::BAD_REQUEST,
            });
        }
    };
    let rows = match qb.build().fetch_all(pool).await {
        Ok(rows) => rows,
        Err(e) => {
            error!("Failed to execute filtered indices query: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let mut positions = Vec::with_capacity(rows.len());
    for row in rows {
        let idx: i32 = row.get("idx");
        if idx >= 0 {
            positions.push(idx as u32);
        }
    }

    insert_cached_indices(cache_key, positions.clone());
    respond_with_indices(positions)
}

fn get_cached_indices(key: &FilterCacheKey) -> Option<Vec<u32>> {
    let mut guard = FILTER_CACHE.write();
    if let Some(entry) = guard.get(key) {
        if entry.stored_at.elapsed() < FILTER_CACHE_TTL {
            return Some(entry.indices.clone());
        } else {
            guard.remove(key);
        }
    }
    None
}

fn insert_cached_indices(key: FilterCacheKey, indices: Vec<u32>) {
    FILTER_CACHE.write().insert(
        key,
        CachedIndices {
            indices,
            stored_at: Instant::now(),
        },
    );
}

fn respond_with_indices(
    indices: Vec<u32>,
) -> Result<([(axum::http::header::HeaderName, &'static str); 1], Bytes), StatusCode> {
    let response = IndicesResponse {
        content_version: 1,
        indices,
    };

    match rkyv::to_bytes::<RkyvError>(&response) {
        Ok(bytes) => Ok((
            [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
            Bytes::from(bytes.into_vec()),
        )),
        Err(e) => {
            error!("Failed to serialize indices response: {:?}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

fn requires_user_scope(spec: &FilterIndicesRequest) -> bool {
    spec.watch_status.is_some()
        || matches!(spec.sort, Some(SortBy::WatchProgress | SortBy::LastWatched))
}

pub fn invalidate_filter_cache_for(library_id: Uuid) {
    FILTER_CACHE
        .write()
        .retain(|key, _| key.library_id != library_id);
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
        MediaID::Movie(id) => match state.db.backend().get_movie_reference(&id).await {
            Ok(movie) => {
                if matches!(movie.details, MediaDetailsOption::Endpoint(_)) {
                    warn!(
                        "Movie {} is missing required TMDB metadata; manual intervention required",
                        movie.id
                    );
                    return Ok(Json(ApiResponse::error(
                        "Movie metadata unavailable; manual matching required".into(),
                    )));
                }

                Ok(Json(ApiResponse::success(Media::Movie(movie))))
            }
            Err(e) => {
                error!("Failed to get movie reference: {}", e);
                Ok(Json(ApiResponse::error(e.to_string())))
            }
        },
        MediaID::Series(id) => match state.db.backend().get_series_reference(&id).await {
            Ok(series) => {
                if matches!(series.details, MediaDetailsOption::Endpoint(_)) {
                    warn!(
                        "Series {} is missing required TMDB metadata; manual intervention required",
                        series.id
                    );
                    return Ok(Json(ApiResponse::error(
                        "Series metadata unavailable; manual matching required".into(),
                    )));
                }

                Ok(Json(ApiResponse::success(Media::Series(series))))
            }
            Err(e) => {
                error!("Failed to get series reference: {}", e);
                Ok(Json(ApiResponse::error(e.to_string())))
            }
        },
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
/*
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
                        state.scan_control.publish_media_event(MediaEvent::MovieUpdated { movie });
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
                        state.scan_control.publish_media_event(MediaEvent::SeriesUpdated { series });
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
*/

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

    let db = state.db.clone();
    let orchestrator = state.scan_control.orchestrator();

    match db.backend().create_library(library.clone()).await {
        Ok(id) => {
            info!("Library successfully created in database with ID: {}", id);

            let actor_config = LibraryActorConfig {
                library: LibraryReference {
                    id: library.id,
                    name: library.name.clone(),
                    library_type: library.library_type,
                    paths: library.paths.clone(),
                },
                root_paths: library.paths.clone(),
                max_outstanding_jobs: 8,
            };

            if let Err(err) = orchestrator.register_library(actor_config).await {
                error!(
                    "Failed to register library {} with orchestrator: {}",
                    library.id, err
                );

                if let Err(delete_err) = db.backend().delete_library(&library.id.to_string()).await
                {
                    error!(
                        "Failed to roll back library {} after orchestrator error: {}",
                        library.id, delete_err
                    );
                }

                return Ok(Json(ApiResponse::error(
                    "failed_to_register_library".to_string(),
                )));
            }

            if request.start_scan && library.enabled {
                match state
                    .scan_control
                    .start_library_scan(library.id, None)
                    .await
                {
                    Ok(accepted) => {
                        info!(
                            "Immediate scan started for library {} with scan ID: {}",
                            library.id, accepted.scan_id
                        );
                    }
                    Err(e) => {
                        warn!(
                            "Failed to trigger immediate scan for library {}: {}",
                            library.id, e
                        );
                    }
                }
            } else {
                info!(
                    "Initial scan skipped for library {} (enabled={}, start_scan={})",
                    library.id, library.enabled, request.start_scan
                );
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
