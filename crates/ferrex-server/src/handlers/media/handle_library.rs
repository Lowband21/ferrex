use axum::{
    body::Bytes,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    http::header,
    response::{IntoResponse, Json, Response},
};
use ferrex_core::domain::users::{rbac, user::User};
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
        ResetLibraryRequest, ResetLibraryResult, ScanCommandAcceptedResponse,
        ScanRunMode, UpdateLibraryRequest,
    },
    types::LibraryType,
};
use ferrex_flatbuffers::{
    FLATBUFFERS_MIME, conversions::library as fb_library,
};
use rkyv::rancor::Error as RkyvError;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::infra::{
    app_state::AppState,
    content_negotiation::{AcceptedFormat, RKYV_OCTET_STREAM_MIME, WireFormat},
    demo_mode,
};

use ferrex_core::domain::scan::orchestration::LibraryActorConfig;
use futures::{StreamExt, TryStreamExt, stream};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use sqlx::PgPool;

const FILTER_CACHE_TTL: Duration = Duration::from_secs(30);
const MIN_SCAN_INTERVAL_MINUTES: u32 = 1;

static FILTER_CACHE: Lazy<RwLock<HashMap<FilterCacheKey, CachedIndices>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));
static LIBRARY_MAINTENANCE_LOCK: Lazy<tokio::sync::Mutex<()>> =
    Lazy::new(|| tokio::sync::Mutex::new(()));

async fn require_library_permission(
    state: &AppState,
    user: &User,
    permission: &str,
) -> Result<(), StatusCode> {
    let permissions = state
        .unit_of_work()
        .rbac
        .get_user_permissions(user.id)
        .await
        .map_err(|err| {
            error!(user_id = %user.id, error = %err, "failed to load library permissions");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if permissions.has_permission(permission) || permissions.has_role("admin") {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

fn actor_config_for_library(
    state: &AppState,
    library: &Library,
) -> LibraryActorConfig {
    LibraryActorConfig {
        library: LibraryReference {
            id: library.id,
            name: library.name.clone(),
            library_type: library.library_type,
            paths: library.paths.clone(),
        },
        root_paths: library.paths.clone(),
        max_outstanding_jobs: state
            .config()
            .scanner
            .library_actor_max_outstanding_jobs,
    }
}

/// Stop runtime producers before deleting library-owned data. If persistence
/// rejects the delete, restore the actor/watch configuration so the library is
/// not left present-but-inert.
pub(crate) async fn delete_library_with_runtime_cleanup(
    state: &AppState,
    library_id: LibraryId,
) -> Result<(), String> {
    let _maintenance_guard = LIBRARY_MAINTENANCE_LOCK.lock().await;
    let libraries = state.unit_of_work().libraries.clone();
    let library = libraries
        .get_library(library_id)
        .await
        .map_err(|err| {
            format!("Failed to load library before deletion: {err}")
        })?
        .ok_or_else(|| "Library not found".to_string())?;
    let orchestrator = state.scan_control().orchestrator();
    let actor_config = actor_config_for_library(state, &library);

    orchestrator
        .unregister_library(&actor_config, library.watch_for_changes)
        .await
        .map_err(|err| format!("Failed to stop library scan runtime: {err}"))?;

    if let Err(delete_error) = libraries.delete_library(library_id).await {
        let restore_result = orchestrator
            .register_library(actor_config, library.watch_for_changes)
            .await;

        return match restore_result {
            Ok(()) => Err(format!("Delete failed: {delete_error}")),
            Err(restore_error) => Err(format!(
                "Delete failed: {delete_error}; scan runtime restoration also failed: {restore_error}"
            )),
        };
    }

    state.scan_control().forget_library(library_id).await;
    invalidate_filter_cache_for(*library_id.as_uuid());
    Ok(())
}

async fn completed_reset_library(
    pool: &PgPool,
    operation_id: Uuid,
) -> Result<Option<LibraryId>, String> {
    sqlx::query_scalar!(
        r#"
        SELECT library_id
        FROM library_maintenance_operations
        WHERE operation_id = $1
          AND operation = 'reset'
        "#,
        operation_id,
    )
    .fetch_optional(pool)
    .await
    .map(|library_id| library_id.map(LibraryId))
    .map_err(|err| format!("Failed to load reset operation: {err}"))
}

async fn reset_library_data(
    pool: &PgPool,
    library_id: LibraryId,
    operation_id: Uuid,
) -> Result<bool, String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|err| format!("Failed to begin library reset: {err}"))?;

    sqlx::query!(
        r#"SELECT pg_advisory_xact_lock(hashtextextended($1::uuid::text, 0))"#,
        library_id.0,
    )
    .execute(&mut *tx)
    .await
    .map_err(|err| format!("Failed to lock library reset operation: {err}"))?;

    if let Some(existing_library_id) = sqlx::query_scalar!(
        r#"
        SELECT library_id
        FROM library_maintenance_operations
        WHERE operation_id = $1
          AND operation = 'reset'
        "#,
        operation_id,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|err| format!("Failed to replay library reset: {err}"))?
    {
        if existing_library_id != library_id.0 {
            return Err(format!(
                "Reset operation {operation_id} belongs to another library"
            ));
        }
        tx.commit()
            .await
            .map_err(|err| format!("Failed to finish reset replay: {err}"))?;
        return Ok(false);
    }

    let restored_library_id = sqlx::query_scalar!(
        r#"
        WITH deleted AS (
            DELETE FROM libraries
            WHERE id = $1
            RETURNING
                id,
                name,
                library_type,
                paths,
                scan_interval_minutes,
                enabled,
                auto_scan,
                watch_for_changes,
                analyze_on_scan,
                max_retry_attempts,
                movie_ref_batch_size,
                created_at
        )
        INSERT INTO libraries (
            id,
            name,
            library_type,
            paths,
            scan_interval_minutes,
            enabled,
            auto_scan,
            watch_for_changes,
            analyze_on_scan,
            max_retry_attempts,
            movie_ref_batch_size,
            created_at,
            updated_at
        )
        SELECT
            id,
            name,
            library_type,
            paths,
            scan_interval_minutes,
            enabled,
            auto_scan,
            watch_for_changes,
            analyze_on_scan,
            max_retry_attempts,
            movie_ref_batch_size,
            created_at,
            NOW()
        FROM deleted
        RETURNING id
        "#,
        library_id.0,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|err| format!("Failed to reset library-owned data: {err}"))?
    .ok_or_else(|| "Library not found".to_string())?;

    sqlx::query!(
        r#"
        INSERT INTO library_maintenance_operations (
            operation_id,
            library_id,
            operation
        )
        VALUES ($1, $2, 'reset')
        "#,
        operation_id,
        restored_library_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(|err| format!("Failed to record library reset: {err}"))?;

    tx.commit()
        .await
        .map_err(|err| format!("Failed to commit library reset: {err}"))?;
    Ok(true)
}

async fn reset_library_with_runtime_cleanup(
    state: &AppState,
    library_id: LibraryId,
    operation_id: Uuid,
) -> Result<ResetLibraryResult, String> {
    let _maintenance_guard = LIBRARY_MAINTENANCE_LOCK.lock().await;
    let postgres = state.postgres();
    let pool = postgres.pool();
    let libraries = state.unit_of_work().libraries.clone();

    if let Some(existing_library_id) =
        completed_reset_library(pool, operation_id).await?
        && existing_library_id != library_id
    {
        return Err(format!(
            "Reset operation {operation_id} belongs to another library"
        ));
    }

    let library = libraries
        .get_library(library_id)
        .await
        .map_err(|err| format!("Failed to load library before reset: {err}"))?
        .ok_or_else(|| "Library not found".to_string())?;
    let actor_config = actor_config_for_library(state, &library);
    let reset_already_completed =
        completed_reset_library(pool, operation_id).await?.is_some();

    if !reset_already_completed {
        state
            .scan_control()
            .orchestrator()
            .unregister_library(&actor_config, library.watch_for_changes)
            .await
            .map_err(|err| {
                format!("Failed to stop library scan runtime: {err}")
            })?;
    }

    let applied = match reset_library_data(pool, library_id, operation_id).await
    {
        Ok(applied) => applied,
        Err(reset_error) => {
            if !reset_already_completed {
                let restore_result = state
                    .scan_control()
                    .orchestrator()
                    .register_library(actor_config, library.watch_for_changes)
                    .await;
                return match restore_result {
                    Ok(()) => Err(reset_error),
                    Err(restore_error) => Err(format!(
                        "{reset_error}; scan runtime restoration also failed: {restore_error}"
                    )),
                };
            }
            return Err(reset_error);
        }
    };

    if applied {
        state.scan_control().forget_library(library_id).await;
        invalidate_filter_cache_for(*library_id.as_uuid());
    }

    let restored_library = libraries
        .get_library(library_id)
        .await
        .map_err(|err| format!("Failed to load reset library: {err}"))?
        .ok_or_else(|| "Reset library was not restored".to_string())?;
    state
        .scan_control()
        .orchestrator()
        .register_library(
            actor_config_for_library(state, &restored_library),
            restored_library.watch_for_changes,
        )
        .await
        .map_err(|err| {
            format!("Failed to restore library scan runtime: {err}")
        })?;

    let scan = if restored_library.enabled {
        let accepted = state
            .scan_control()
            .start_library_scan(
                library_id,
                Some(operation_id),
                ScanRunMode::Manual,
            )
            .await
            .map_err(|err| {
                format!(
                    "Library data reset completed, but the fresh scan could not be started: {err}"
                )
            })?;
        Some(ScanCommandAcceptedResponse {
            scan_id: accepted.scan_id,
            correlation_id: accepted.correlation_id,
            status: accepted.status.into(),
            mode: accepted.mode,
            idempotency_key: accepted.idempotency_key,
            run_key: accepted.run_key,
            disposition: accepted.disposition,
        })
    } else {
        None
    };

    Ok(ResetLibraryResult { library_id, scan })
}

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
    AcceptedFormat(response_format): AcceptedFormat,
) -> Result<Response, StatusCode> {
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
    let (payload_len, response) = match response_format {
        WireFormat::FlatBuffers => {
            let bytes = fb_library::serialize_library_list(&library_responses);
            let payload_len = bytes.len();
            (
                payload_len,
                (
                    [(header::CONTENT_TYPE, FLATBUFFERS_MIME)],
                    Bytes::from(bytes),
                )
                    .into_response(),
            )
        }
        WireFormat::RkyvOctetStream => {
            let bytes =
                match rkyv::to_bytes::<rkyv::rancor::Error>(&library_responses)
                {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        error!(
                            "Failed to serialize response with rkyv: {:?}",
                            e
                        );
                        return Err(StatusCode::INTERNAL_SERVER_ERROR);
                    }
                };
            let payload_len = bytes.len();
            (
                payload_len,
                (
                    [(header::CONTENT_TYPE, RKYV_OCTET_STREAM_MIME)],
                    Bytes::from(bytes.into_vec()),
                )
                    .into_response(),
            )
        }
        WireFormat::Json => (
            0,
            Json(ApiResponse::success(library_responses)).into_response(),
        ),
    };
    let serialize_elapsed = serialize_started.elapsed();

    let total_elapsed = request_started.elapsed();
    info!(
        "Libraries snapshot built: libraries={} media_items={} bytes={} format={:?} refs_elapsed={:?} fetch_elapsed={:?} serialize_elapsed={:?} total_elapsed={:?}",
        library_count,
        media_count,
        payload_len,
        response_format,
        refs_elapsed,
        fetch_elapsed,
        serialize_elapsed,
        total_elapsed
    );

    Ok(response)
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

fn validate_scan_interval(scan_interval_minutes: u32) -> Option<String> {
    (scan_interval_minutes < MIN_SCAN_INTERVAL_MINUTES).then(|| {
        format!(
            "scan_interval_minutes must be at least {MIN_SCAN_INTERVAL_MINUTES} minute"
        )
    })
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
    Extension(user): Extension<User>,
    Json(request): Json<CreateLibraryRequest>,
) -> Result<Json<ApiResponse<String>>, StatusCode> {
    require_library_permission(
        &state,
        &user,
        rbac::permissions::LIBRARIES_CREATE,
    )
    .await?;
    if demo_mode::is_demo_mode(&state) {
        return Ok(Json(ApiResponse::error(
            "Library creation is disabled in demo mode".to_string(),
        )));
    }

    info!("Creating new library: {}", request.name);

    let library_id = LibraryId::new();
    info!("Generated library ID: {}", library_id);

    if let Some(message) = validate_scan_interval(request.scan_interval_minutes)
    {
        return Ok(Json(ApiResponse::error(message)));
    }

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
        auto_scan: request.auto_scan,
        watch_for_changes: request.watch_for_changes,
        analyze_on_scan: request.analyze_on_scan,
        max_retry_attempts: request.max_retry_attempts,
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
                max_outstanding_jobs: state
                    .config()
                    .scanner
                    .library_actor_max_outstanding_jobs,
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
                    .start_library_scan(library.id, None, ScanRunMode::Manual)
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
    Extension(user): Extension<User>,
    Path(id): Path<String>, // TODO: Use LibraryID directly
    Json(request): Json<UpdateLibraryRequest>,
) -> Result<Json<ApiResponse<String>>, StatusCode> {
    require_library_permission(
        &state,
        &user,
        rbac::permissions::LIBRARIES_UPDATE,
    )
    .await?;

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

    let previous_paths = library.paths.clone();
    let previous_watch_for_changes = library.watch_for_changes;

    // Update fields if provided
    if let Some(name) = request.name {
        library.name = name;
    }
    if let Some(paths) = request.paths {
        library.paths = paths.into_iter().map(PathBuf::from).collect();
    }
    if let Some(scan_interval) = request.scan_interval_minutes {
        if let Some(message) = validate_scan_interval(scan_interval) {
            return Ok(Json(ApiResponse::error(message)));
        }
        library.scan_interval_minutes = scan_interval;
    }
    if let Some(enabled) = request.enabled {
        library.enabled = enabled;
    }
    if let Some(auto_scan) = request.auto_scan {
        library.auto_scan = auto_scan;
    }
    if let Some(watch_for_changes) = request.watch_for_changes {
        library.watch_for_changes = watch_for_changes;
    }
    if let Some(analyze_on_scan) = request.analyze_on_scan {
        library.analyze_on_scan = analyze_on_scan;
    }
    if let Some(max_retry_attempts) = request.max_retry_attempts {
        library.max_retry_attempts = max_retry_attempts;
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

    let actor_config = LibraryActorConfig {
        library: LibraryReference {
            id: library.id,
            name: library.name.clone(),
            library_type: library.library_type,
            paths: library.paths.clone(),
        },
        root_paths: library.paths.clone(),
        max_outstanding_jobs: state
            .config()
            .scanner
            .library_actor_max_outstanding_jobs,
    };
    let watch_runtime_changed = previous_paths != library.paths
        || previous_watch_for_changes != library.watch_for_changes;
    let watch_for_changes = library.watch_for_changes;

    match libraries_repo
        .update_library(LibraryId(uuid), library)
        .await
    {
        Ok(_) => {
            if watch_runtime_changed {
                state
                    .scan_control()
                    .orchestrator()
                    .unregister_library_watch(LibraryId(uuid))
                    .await;
            }

            if let Err(err) = state
                .scan_control()
                .orchestrator()
                .register_library(actor_config, watch_for_changes)
                .await
            {
                error!(
                    "Failed to refresh library {} scan runtime after update: {}",
                    id, err
                );
                return Ok(Json(ApiResponse::error(
                    "failed_to_refresh_library_scan_runtime".to_string(),
                )));
            }

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
    Extension(user): Extension<User>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<String>>, StatusCode> {
    require_library_permission(
        &state,
        &user,
        rbac::permissions::LIBRARIES_DELETE,
    )
    .await?;

    info!("Deleting library: {}", id);

    let library_uuid =
        Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;

    if demo_mode::is_demo_mode(&state)
        && !demo_mode::is_demo_library(&LibraryId(library_uuid))
    {
        return Ok(Json(ApiResponse::error("Library not found".to_string())));
    }

    match delete_library_with_runtime_cleanup(&state, LibraryId(library_uuid))
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

/// Atomically clear library-owned data while preserving library identity and
/// configuration, then start an idempotent fresh scan.
pub async fn reset_library_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(id): Path<String>,
    Json(request): Json<ResetLibraryRequest>,
) -> Result<Json<ApiResponse<ResetLibraryResult>>, StatusCode> {
    require_library_permission(
        &state,
        &user,
        rbac::permissions::LIBRARIES_DELETE,
    )
    .await?;
    require_library_permission(
        &state,
        &user,
        rbac::permissions::LIBRARIES_CREATE,
    )
    .await?;
    require_library_permission(
        &state,
        &user,
        rbac::permissions::LIBRARIES_SCAN,
    )
    .await?;

    let library_id =
        LibraryId(Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?);
    if demo_mode::is_demo_mode(&state)
        && !demo_mode::is_demo_library(&library_id)
    {
        return Ok(Json(ApiResponse::error("Library not found".to_string())));
    }

    match reset_library_with_runtime_cleanup(
        &state,
        library_id,
        request.operation_id,
    )
    .await
    {
        Ok(result) => Ok(Json(ApiResponse::success(result))),
        Err(err) => {
            error!(library_id = %library_id, error = %err, "failed to reset library");
            Ok(Json(ApiResponse::error(err)))
        }
    }
}

#[cfg(test)]
mod reset_tests {
    use super::*;
    use sqlx::Row;

    #[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
    async fn reset_is_idempotent_and_preserves_library_configuration(
        pool: PgPool,
    ) {
        let library_id = Uuid::now_v7();
        let operation_id = Uuid::now_v7();
        sqlx::query(
            r#"
            INSERT INTO libraries (
                id, name, library_type, paths, scan_interval_minutes, enabled,
                auto_scan, watch_for_changes, analyze_on_scan,
                max_retry_attempts, movie_ref_batch_size
            )
            VALUES ($1, 'Reset Me', 'movies', ARRAY['/media/reset']::varchar[],
                    137, true, false, true, true, 9, 333)
            "#,
        )
        .bind(library_id)
        .execute(&pool)
        .await
        .expect("seed library");
        sqlx::query(
            r#"
            INSERT INTO library_scan_runs (
                scan_id, library_id, mode, correlation_id, status
            )
            VALUES ($1, $2, 'manual', $3, 'running')
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(library_id)
        .bind(Uuid::now_v7())
        .execute(&pool)
        .await
        .expect("seed owned scan data");

        assert!(
            reset_library_data(&pool, LibraryId(library_id), operation_id,)
                .await
                .expect("apply reset")
        );
        assert!(
            !reset_library_data(&pool, LibraryId(library_id), operation_id,)
                .await
                .expect("replay reset")
        );

        let library = sqlx::query(
            r#"
            SELECT name, paths, scan_interval_minutes, enabled, auto_scan,
                   watch_for_changes, analyze_on_scan, max_retry_attempts,
                   movie_ref_batch_size, last_scan
            FROM libraries
            WHERE id = $1
            "#,
        )
        .bind(library_id)
        .fetch_one(&pool)
        .await
        .expect("load restored library");
        assert_eq!(library.get::<String, _>("name"), "Reset Me");
        assert_eq!(
            library.get::<Vec<String>, _>("paths"),
            vec!["/media/reset".to_string()]
        );
        assert_eq!(library.get::<i32, _>("scan_interval_minutes"), 137);
        assert!(library.get::<bool, _>("enabled"));
        assert!(!library.get::<bool, _>("auto_scan"));
        assert!(library.get::<bool, _>("watch_for_changes"));
        assert!(library.get::<bool, _>("analyze_on_scan"));
        assert_eq!(library.get::<i32, _>("max_retry_attempts"), 9);
        assert_eq!(library.get::<i32, _>("movie_ref_batch_size"), 333);
        assert!(
            library
                .get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_scan")
                .is_none()
        );

        let owned_scan_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM library_scan_runs WHERE library_id = $1",
        )
        .bind(library_id)
        .fetch_one(&pool)
        .await
        .expect("count reset scan rows");
        assert_eq!(owned_scan_count, 0);
    }
}
