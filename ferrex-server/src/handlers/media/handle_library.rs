use axum::{
    body::Bytes,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    http::header,
    response::{IntoResponse, Json},
};
use ferrex_core::domain::users::user::User;
use ferrex_core::error::MediaError;
use ferrex_core::query::{
    filtering::hash_filter_spec,
    types::{SortBy, SortOrder},
};
use ferrex_core::types::{
    Library, LibraryId, LibraryReference, Media, MediaID,
};
use ferrex_core::{
    api::types::{
        ApiResponse, CreateLibraryRequest, FetchMediaRequest,
        FilterIndicesRequest, IndicesResponse, LibraryMediaResponse,
        UpdateLibraryRequest,
    },
    types::LibraryType,
};
use rkyv::rancor::Error as RkyvError;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::infra::app_state::AppState;
use crate::infra::demo_mode;

use ferrex_core::domain::scan::orchestration::LibraryActorConfig;
use futures::{StreamExt, TryStreamExt, stream};
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
    if demo_mode::is_demo_mode(&state)
        && !demo_mode::is_demo_library(&LibraryId(library_id))
    {
        return Err(StatusCode::NOT_FOUND);
    }

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
    let request_started = Instant::now();
    let uow = state.unit_of_work();

    let refs_started = Instant::now();
    let libraries = match uow.libraries.list_library_references().await {
        Ok(libraries) => libraries,
        Err(e) => {
            error!("Failed to get libraries: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    let libraries = demo_mode::filter_library_references(&state, libraries);
    let refs_elapsed = refs_started.elapsed();

    // Library snapshots can be expensive: each library has a potentially large
    // media reference list. Previously this handler performed sequential I/O
    // which can easily exceed the player's 30s reqwest timeout.
    //
    // Fetch in limited parallelism to reduce tail latency without stampeding
    // the database.
    let fetch_started = Instant::now();
    let parallelism: usize = 4;
    let results: Result<Vec<Option<Library>>, StatusCode> =
        stream::iter(libraries.into_iter())
            .map(|library_ref| {
                let uow = Arc::clone(&uow);
                async move {
                    let library = uow
                        .libraries
                        .get_library(library_ref.id)
                        .await
                        .map_err(|e| {
                            error!(
                                "Failed to get library {}: {}",
                                library_ref.id, e
                            );
                            StatusCode::INTERNAL_SERVER_ERROR
                        })?;

                    // Movie and series libraries are bootstrapped via dedicated
                    // snapshot endpoints (`movie-batches` and `series-bundles`).
                    // Keep `/libraries` focused on library metadata so the
                    // snapshot stays small and fast to fetch.
                    if matches!(
                        library_ref.library_type,
                        LibraryType::Movies | LibraryType::Series
                    ) {
                        return Ok::<_, StatusCode>(library.map(|mut l| {
                            l.media = None;
                            l
                        }));
                    }

                    let media = uow
                        .media_refs
                        .get_library_media_references(
                            library_ref.id,
                            library_ref.library_type,
                        )
                        .await
                        .map_err(|e| {
                            error!(
                                "Failed to get library media {}: {}",
                                library_ref.id, e
                            );
                            StatusCode::INTERNAL_SERVER_ERROR
                        })?;

                    Ok::<_, StatusCode>(library.map(|mut l| {
                        l.media = Some(media);
                        l
                    }))
                }
            })
            .buffer_unordered(parallelism)
            .try_collect()
            .await;

    let fetch_elapsed = fetch_started.elapsed();
    let mut library_responses =
        results?.into_iter().flatten().collect::<Vec<_>>();

    // Stable ordering helps caching/consumers and improves debuggability.
    library_responses.sort_by_key(|l| l.id);

    let library_count = library_responses.len();
    let media_count: usize = library_responses
        .iter()
        .map(|l| l.media.as_ref().map(|m| m.len()).unwrap_or(0))
        .sum();

    let serialize_started = Instant::now();
    let bytes = match rkyv::to_bytes::<rkyv::rancor::Error>(&library_responses)
    {
        Ok(bytes) => bytes,
        Err(e) => {
            error!("Failed to serialize response with rkyv: {:?}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    let serialize_elapsed = serialize_started.elapsed();
    let payload_len = bytes.len();

    let total_elapsed = request_started.elapsed();
    info!(
        "Libraries snapshot built: libraries={} media_items={} bytes={} refs_elapsed={:?} fetch_elapsed={:?} serialize_elapsed={:?} total_elapsed={:?}",
        library_count,
        media_count,
        payload_len,
        refs_elapsed,
        fetch_elapsed,
        serialize_elapsed,
        total_elapsed
    );

    Ok::<_, StatusCode>((
        [(header::CONTENT_TYPE, "application/octet-stream")],
        Bytes::from(bytes.into_vec()),
    ))
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
    Extension(_user): Extension<User>,
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

    let library_uuid = library_ref.id.to_uuid();

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
                Ok(movie) => Ok(Json(ApiResponse::success(Media::Movie(
                    Box::new(movie),
                )))),
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
                Ok(Json(ApiResponse::success(Media::Series(Box::new(series)))))
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
                    Ok(Json(ApiResponse::success(Media::Season(Box::new(
                        season,
                    )))))
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
                    Ok(Json(ApiResponse::success(Media::Episode(Box::new(
                        episode,
                    )))))
                }
                Err(e) => {
                    error!("Failed to get episode reference: {}", e);
                    Ok(Json(ApiResponse::error(e.to_string())))
                }
            }
        }
    }
}

// Manual TMDB matching for media items
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
            let libraries =
                demo_mode::filter_library_references(&state, libraries);
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

    if demo_mode::is_demo_mode(&state)
        && !demo_mode::is_demo_library(&LibraryId(id))
    {
        return Ok(Json(ApiResponse::error("Library not found".to_string())));
    }

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
    if demo_mode::is_demo_mode(&state) {
        return Ok(Json(ApiResponse::error(
            "Library creation is disabled in demo mode".to_string(),
        )));
    }

    info!("Creating new library: {}", request.name);

    let library_id = LibraryId::new();
    info!("Generated library ID: {}", library_id);

    let movie_ref_batch_size =
        match ferrex_core::types::ids::MovieReferenceBatchSize::new(
            request.movie_ref_batch_size,
        ) {
            Ok(value) => value,
            Err(e) => {
                return Ok(Json(ApiResponse::error(format!(
                    "Invalid movie_ref_batch_size: {}",
                    e
                ))));
            }
        };

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
        movie_ref_batch_size,
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

    if demo_mode::is_demo_mode(&state)
        && !demo_mode::is_demo_library(&LibraryId(uuid))
    {
        return Ok(Json(ApiResponse::error("Library not found".to_string())));
    }
    let libraries_repo = state.unit_of_work().libraries.clone();

    let mut library = match libraries_repo.get_library(LibraryId(uuid)).await {
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
    if let Some(size) = request.movie_ref_batch_size {
        match ferrex_core::types::ids::MovieReferenceBatchSize::new(size) {
            Ok(value) => {
                library.movie_ref_batch_size = value;
            }
            Err(e) => {
                return Ok(Json(ApiResponse::error(format!(
                    "Invalid movie_ref_batch_size: {}",
                    e
                ))));
            }
        }
    }
    library.updated_at = chrono::Utc::now();

    match libraries_repo
        .update_library(LibraryId(uuid), library)
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

    if demo_mode::is_demo_mode(&state)
        && !demo_mode::is_demo_library(&LibraryId(library_uuid))
    {
        return Ok(Json(ApiResponse::error("Library not found".to_string())));
    }

    match state
        .unit_of_work()
        .libraries
        .delete_library(LibraryId(library_uuid))
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
