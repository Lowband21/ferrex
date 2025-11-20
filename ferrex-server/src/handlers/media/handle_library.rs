use axum::{
    body::Bytes,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use ferrex_core::error::MediaError;
use ferrex_core::query::{
    filtering::hash_filter_spec,
    types::{SortBy, SortOrder},
};
use ferrex_core::types::{
    Library, LibraryID, LibraryReference, Media, MediaDetailsOption, MediaID,
};
use ferrex_core::user::User;
use ferrex_core::{
    api_types::{
        ApiResponse, CreateLibraryRequest, FetchMediaRequest,
        FilterIndicesRequest, IndicesResponse, LibraryMediaResponse,
        UpdateLibraryRequest,
    },
    orchestration::LibraryActorConfig,
    types::LibraryType,
};
use rkyv::rancor::Error as RkyvError;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::infra::app_state::AppState;

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
        .unit_of_work()
        .media_refs
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
    let library = match state
        .unit_of_work()
        .libraries
        .get_library_reference(library_id)
        .await
    {
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

pub async fn get_libraries_with_media_handler(
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state
        .unit_of_work()
        .libraries
        .list_library_references()
        .await
    {
        Ok(libraries) => {
            let mut library_results = Vec::new();
            for library_ref in libraries {
                let library = state
                    .unit_of_work()
                    .libraries
                    .get_library(library_ref.id)
                    .await
                    .map_err(|e| {
                        error!("Failed to get library: {}", e);
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?;
                let library_media_response =
                    get_library_media_util(&state, library_ref).await?;
                if let Some(mut library) = library {
                    library.media = Some(library_media_response.media);
                    library_results.push(library);
                }
            }
            let library_responses: Vec<_> =
                library_results.into_iter().collect::<Vec<_>>();

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
    let library_ref = match state
        .unit_of_work()
        .libraries
        .get_library_reference(library_id)
        .await
    {
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

    let indices = match state
        .unit_of_work()
        .indices
        .fetch_sorted_movie_indices(
            library_ref.id,
            sort_field,
            sort_order,
            params.offset,
            params.limit,
        )
        .await
    {
        Ok(indices) => indices,
        Err(err) => {
            error!(
                "Failed to fetch precomputed positions for library {}: {}",
                library_id, err
            );
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    respond_with_indices(indices)
}

/// Filter indices (movies Phase 1)
pub async fn post_library_filtered_indices_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(library_id): Path<Uuid>,
    Json(spec): Json<FilterIndicesRequest>,
) -> impl IntoResponse {
    info!("Getting filtered indices for library: {}", library_id);

    let library_ref = match state
        .unit_of_work()
        .libraries
        .get_library_reference(library_id)
        .await
    {
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

    let indices = match state
        .unit_of_work()
        .indices
        .fetch_filtered_movie_indices(library_ref.id, &spec, Some(user.id))
        .await
    {
        Ok(indices) => indices,
        Err(MediaError::InvalidMedia(msg)) => {
            warn!("Rejected filtered indices request: {}", msg);
            if msg.contains("unsupported media type") {
                return Err(StatusCode::NOT_IMPLEMENTED);
            }
            return Err(StatusCode::BAD_REQUEST);
        }
        Err(err) => {
            error!("Failed to execute filtered indices query: {}", err);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    insert_cached_indices(cache_key, indices.clone());
    respond_with_indices(indices)
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
) -> Result<
    ([(axum::http::header::HeaderName, &'static str); 1], Bytes),
    StatusCode,
> {
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
        || matches!(
            spec.sort,
            Some(SortBy::WatchProgress | SortBy::LastWatched)
        )
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
        MediaID::Movie(id) => {
            match state
                .unit_of_work()
                .media_refs
                .get_movie_reference(&id)
                .await
            {
                Ok(movie) => {
                    if matches!(movie.details, MediaDetailsOption::Endpoint(_))
                    {
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
            }
        }
        MediaID::Series(id) => match state
            .unit_of_work()
            .media_refs
            .get_series_reference(&id)
            .await
        {
            Ok(series) => {
                if matches!(series.details, MediaDetailsOption::Endpoint(_)) {
                    warn!(
                        "Series {} is missing required TMDB metadata; manual intervention required",
                        series.id
                    );
                    return Ok(Json(ApiResponse::error(
                        "Series metadata unavailable; manual matching required"
                            .into(),
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
            match state
                .unit_of_work()
                .media_refs
                .get_season_reference(&id)
                .await
            {
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
            match state
                .unit_of_work()
                .media_refs
                .get_episode_reference(&id)
                .await
            {
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
            match state.unit_of_work()
                .media_refs
                .update_movie_tmdb_id(&id, request.tmdb_id)
                .await
            {
                Ok(_) => {
                    // Send update event
                    if let Ok(movie) = state.unit_of_work().media_refs.get_movie_reference(&id).await {
                        state.scan_control().publish_media_event(MediaEvent::MovieUpdated { movie });
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
            match state.unit_of_work()
                .media_refs
                .update_series_tmdb_id(&id, request.tmdb_id)
                .await
            {
                Ok(_) => {
                    // Update all episodes in this series
                    // TODO: This should cascade to seasons and episodes

                    // Send update event
                    if let Ok(series) = state.unit_of_work().media_refs.get_series_reference(&id).await {
                        state.scan_control().publish_media_event(MediaEvent::SeriesUpdated { series });
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

    match state
        .unit_of_work()
        .libraries
        .list_library_references()
        .await
    {
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

    match state
        .unit_of_work()
        .libraries
        .get_library_reference(id)
        .await
    {
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

    let libraries_repo = state.unit_of_work().libraries.clone();
    let orchestrator = state.scan_control().orchestrator();

    match libraries_repo.create_library(library.clone()).await {
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

            if let Err(err) = orchestrator
                .register_library(actor_config, library.watch_for_changes)
                .await
            {
                error!(
                    "Failed to register library {} with orchestrator: {}",
                    library.id, err
                );

                if let Err(delete_err) =
                    libraries_repo.delete_library(library.id).await
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
                    .scan_control()
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

            Ok(Json(ApiResponse::success(id.to_string())))
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
    let libraries_repo = state.unit_of_work().libraries.clone();

    let mut library = match libraries_repo.get_library(LibraryID(uuid)).await {
        Ok(Some(lib)) => lib,
        Ok(None) => {
            return Ok(Json(ApiResponse::error(
                "Library not found".to_string(),
            )));
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

    match libraries_repo
        .update_library(LibraryID(uuid), library)
        .await
    {
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

    let library_uuid =
        Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;

    match state
        .unit_of_work()
        .libraries
        .delete_library(LibraryID(library_uuid))
        .await
    {
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
