use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use chrono::{Duration, Utc};
use ferrex_core::api::types::collections::*;
use ferrex_core::domain::users::auth::{
    device::AuthDeviceStatus, domain::value_objects::SessionScope,
};
use ferrex_core::player_prelude::{
    ActiveScansResponse, AuthToken, AuthenticatedDevice, ConfirmClaimResponse,
    CreateLibraryRequest, FilterIndicesRequest, ImageManifestRequest,
    ImageManifestResponse, LatestProgressResponse, Library, LibraryId,
    LibraryType, Media, MediaQuery, MediaRootBrowseResponse, MediaWithStatus,
    MovieBatchFetchRequest, MovieBatchId, MovieBatchSyncRequest,
    MovieBatchSyncResponse, Platform, Role, ScanCommandAcceptedResponse,
    ScanCommandRequest, ScanConfig, ScanLifecycleStatus, ScanMetrics,
    ScanRunMode, ScanStartDisposition, SeriesBundleFetchRequest,
    SeriesBundleSyncRequest, SeriesBundleSyncResponse, SeriesID,
    StartClaimResponse, StartScanRequest, UpdateLibraryRequest,
    UpdateProgressRequest, User, UserPermissions, UserPreferences,
    UserWatchState,
};
use ferrex_model::image::ImageQuery;
use ferrex_model::{MediaID, MovieID, MovieReferenceBatchSize};
use rkyv::util::AlignedVec;
use uuid::Uuid;

#[cfg(feature = "demo")]
use crate::api_types::{DemoResetRequest, DemoStatus};
use crate::services::api::{ApiService, ImageFetchResult};
use ferrex_player_foundation::repository::{RepositoryError, RepositoryResult};

#[derive(Debug, Clone)]
pub struct TestApiService {
    inner: Arc<RwLock<InnerApiState>>,
    base_url: Arc<str>,
}

#[derive(Debug, Clone)]
struct InnerApiState {
    libraries: Vec<Library>,
    library_media: HashMap<Uuid, Vec<Media>>,
    collections: HashMap<CollectionId, CollectionRecord>,
    collection_order: Vec<CollectionId>,
    shelf_placements: Vec<ShelfPlacement>,
    tmdb_collections: Vec<TmdbCollectionSummary>,
    next_collection_query_error: Option<String>,
    next_collection_write_error: Option<String>,
    watch_state: UserWatchState,
    setup_required: bool,
    setup_token_required: bool,
    auth_token: Option<AuthToken>,
    devices: Vec<AuthenticatedDevice>,
    last_claim: Option<StartClaimResponse>,
    current_user: Option<User>,
    current_permissions: Option<UserPermissions>,
    playback_ticket_result: Option<Result<String, String>>,
}

#[derive(Debug, Clone)]
struct CollectionRecord {
    detail: CollectionDetail,
    items: Vec<CollectionMember>,
}

type CollectionFixtures =
    (HashMap<CollectionId, CollectionRecord>, Vec<CollectionId>);

impl Default for TestApiService {
    fn default() -> Self {
        Self::new("https://localhost:3000")
    }
}

impl TestApiService {
    pub fn new(base_url: impl Into<String>) -> Self {
        let base_url_string = base_url.into();
        let library = sample_library("Sample Library");
        let devices = vec![sample_device(Uuid::now_v7())];
        let (collections, collection_order) = sample_collections();

        let sample_user = sample_user("demo_admin");
        let sample_permissions = sample_permissions(sample_user.id);

        Self {
            inner: Arc::new(RwLock::new(InnerApiState {
                libraries: vec![library],
                library_media: HashMap::new(),
                collections,
                collection_order,
                shelf_placements: Vec::new(),
                tmdb_collections: sample_tmdb_collections(),
                next_collection_query_error: None,
                next_collection_write_error: None,
                watch_state: UserWatchState::new(),
                setup_required: true,
                setup_token_required: false,
                auth_token: None,
                devices,
                last_claim: None,
                current_user: Some(sample_user),
                current_permissions: Some(sample_permissions),
                playback_ticket_result: None,
            })),
            base_url: Arc::from(base_url_string),
        }
    }

    pub fn set_setup_required(&self, value: bool) {
        if let Ok(mut guard) = self.inner.write() {
            guard.setup_required = value;
        }
    }

    pub fn set_requires_setup_token(&self, value: bool) {
        if let Ok(mut guard) = self.inner.write() {
            guard.setup_token_required = value;
        }
    }

    pub fn push_library(&self, library: Library) {
        if let Ok(mut guard) = self.inner.write() {
            guard.libraries.push(library);
        }
    }

    pub fn set_devices(&self, devices: Vec<AuthenticatedDevice>) {
        if let Ok(mut guard) = self.inner.write() {
            guard.devices = devices;
        }
    }

    pub fn set_watch_state(&self, watch_state: UserWatchState) {
        if let Ok(mut guard) = self.inner.write() {
            guard.watch_state = watch_state;
        }
    }

    pub fn set_playback_ticket(&self, access_token: impl Into<String>) {
        if let Ok(mut guard) = self.inner.write() {
            guard.playback_ticket_result = Some(Ok(access_token.into()));
        }
    }

    pub fn set_playback_ticket_error(&self, message: impl Into<String>) {
        if let Ok(mut guard) = self.inner.write() {
            guard.playback_ticket_result = Some(Err(message.into()));
        }
    }

    /// Seed or replace a collection detail in the in-memory API stub.
    pub fn upsert_collection(
        &self,
        detail: CollectionDetail,
        items: Vec<CollectionMember>,
    ) {
        if let Ok(mut guard) = self.inner.write() {
            let collection_id = detail.summary.identity.id;
            if !guard.collection_order.contains(&collection_id) {
                guard.collection_order.push(collection_id);
            }
            guard
                .collections
                .insert(collection_id, CollectionRecord { detail, items });
            guard.sync_collection(collection_id);
        }
    }

    /// Append an item to an existing collection for focused UI tests.
    pub fn push_collection_item(
        &self,
        collection_id: CollectionId,
        item: CollectionMember,
    ) {
        if let Ok(mut guard) = self.inner.write()
            && let Some(record) = guard.collections.get_mut(&collection_id)
        {
            record.items.push(item);
            guard.sync_collection(collection_id);
        }
    }

    /// Add a TMDB list/collection summary returned by the test stub.
    pub fn push_tmdb_collection(&self, summary: TmdbCollectionSummary) {
        if let Ok(mut guard) = self.inner.write() {
            guard.tmdb_collections.push(summary);
        }
    }

    /// Cause the next collection read operation to fail.
    pub fn fail_next_collection_query(&self, message: impl Into<String>) {
        if let Ok(mut guard) = self.inner.write() {
            guard.next_collection_query_error = Some(message.into());
        }
    }

    /// Cause the next collection write operation to fail.
    pub fn fail_next_collection_write(&self, message: impl Into<String>) {
        if let Ok(mut guard) = self.inner.write() {
            guard.next_collection_write_error = Some(message.into());
        }
    }
}

impl InnerApiState {
    fn take_collection_query_error(&mut self) -> Option<String> {
        self.next_collection_query_error.take()
    }

    fn take_collection_write_error(&mut self) -> Option<String> {
        self.next_collection_write_error.take()
    }

    fn sync_collection(&mut self, collection_id: CollectionId) {
        if let Some(record) = self.collections.get_mut(&collection_id) {
            record.items.sort_by(|left, right| {
                left.position
                    .cmp(&right.position)
                    .then_with(|| left.item_key.cmp(&right.item_key))
            });
            let item_count = record.items.len() as u32;
            record.detail.summary.item_count = item_count;
            record.detail.summary.materialization.item_count = item_count;
            record.detail.items_preview =
                record.items.iter().take(12).cloned().collect();
        }
    }

    fn collection_record(
        &self,
        collection_id: CollectionId,
    ) -> RepositoryResult<&CollectionRecord> {
        self.collections.get(&collection_id).ok_or_else(|| {
            RepositoryError::NotFound {
                entity_type: "Collection".into(),
                id: collection_id.to_string(),
            }
        })
    }

    fn collection_record_mut(
        &mut self,
        collection_id: CollectionId,
    ) -> RepositoryResult<&mut CollectionRecord> {
        self.collections.get_mut(&collection_id).ok_or_else(|| {
            RepositoryError::NotFound {
                entity_type: "Collection".into(),
                id: collection_id.to_string(),
            }
        })
    }
}

fn paginate<T: Clone>(
    values: &[T],
    page: &CollectionPagination,
) -> (Vec<T>, CollectionPageInfo) {
    let total = values.len();
    let limit = if page.limit == 0 {
        DEFAULT_COLLECTION_PAGE_LIMIT
    } else {
        page.limit.min(MAX_COLLECTION_PAGE_LIMIT)
    } as usize;
    let offset = page
        .cursor
        .as_deref()
        .and_then(|cursor| cursor.parse::<usize>().ok())
        .unwrap_or(0)
        .min(total);
    let end = offset.saturating_add(limit).min(total);
    let next_cursor = (end < total).then(|| end.to_string());

    (
        values[offset..end].to_vec(),
        CollectionPageInfo {
            next_cursor,
            limit: limit as u16,
            total: total as u64,
        },
    )
}

fn collection_media_scope_matches(
    scope: &CollectionMediaScope,
    media_type: CollectionMediaKind,
) -> bool {
    match scope {
        CollectionMediaScope::All => true,
        CollectionMediaScope::Types { media_types } => {
            media_types.contains(&media_type)
        }
        CollectionMediaScope::Library { media_types, .. } => {
            media_types.is_empty() || media_types.contains(&media_type)
        }
        CollectionMediaScope::ExplicitItems { .. } => true,
    }
}

fn detail_with_expansions(
    record: &CollectionRecord,
    placements: &[ShelfPlacement],
    request: &GetCollectionDetailRequest,
) -> CollectionDetail {
    let mut detail = record.detail.clone();
    detail.summary.item_count = record.items.len() as u32;
    detail.summary.materialization.item_count = record.items.len() as u32;

    if !request.include_rule {
        detail.rule = None;
    }
    if request.include_items_preview {
        detail.items_preview = record.items.iter().take(12).cloned().collect();
    } else {
        detail.items_preview.clear();
    }
    if request.include_shelf_placements {
        detail.shelf_placements = placements
            .iter()
            .filter(|placement| {
                placement.collection_id == detail.summary.identity.id
            })
            .cloned()
            .collect();
    } else {
        detail.shelf_placements.clear();
    }

    detail
}

fn bump_collection_version(summary: &mut CollectionSummary) {
    summary.version.revision = summary.version.revision.saturating_add(1);
    summary.version.etag = Some(format!(
        "collection-{}-{}",
        summary.identity.id, summary.version.revision
    ));
    summary.timestamps.updated_at = Utc::now();
}

fn ensure_expected_revision(
    summary: &CollectionSummary,
    expected_revision: Option<u64>,
) -> RepositoryResult<()> {
    if let Some(expected) = expected_revision
        && summary.version.revision != expected
    {
        return Err(RepositoryError::UpdateFailed(format!(
            "Collection version conflict: expected revision {}, found {}",
            expected, summary.version.revision
        )));
    }
    Ok(())
}

fn collection_rule_hash(
    rule: &DynamicCollectionRule,
) -> RepositoryResult<(String, String)> {
    let input = rule.rule_hash_input_json().map_err(|error| {
        RepositoryError::SerializationError(error.to_string())
    })?;
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    Ok((input, format!("{:016x}", hasher.finish())))
}

fn validate_rule(
    rule: &DynamicCollectionRule,
) -> RepositoryResult<ValidateCollectionRuleResponse> {
    let (rule_hash_input, rule_hash) = collection_rule_hash(rule)?;
    let mut errors = Vec::new();

    if rule.schema_version != COLLECTION_RULE_SCHEMA_VERSION {
        errors.push(CollectionRuleValidationError {
            path: "schema_version".into(),
            message: format!(
                "Unsupported collection rule schema version {}",
                rule.schema_version
            ),
        });
    }
    if rule.sort.schema_version != COLLECTION_SORT_SCHEMA_VERSION {
        errors.push(CollectionRuleValidationError {
            path: "sort.schema_version".into(),
            message: format!(
                "Unsupported collection sort schema version {}",
                rule.sort.schema_version
            ),
        });
    }
    if rule.limit.schema_version != COLLECTION_LIMIT_SCHEMA_VERSION {
        errors.push(CollectionRuleValidationError {
            path: "limit.schema_version".into(),
            message: format!(
                "Unsupported collection limit schema version {}",
                rule.limit.schema_version
            ),
        });
    }
    if rule.limit.max_items == Some(0) {
        errors.push(CollectionRuleValidationError {
            path: "limit.max_items".into(),
            message: "max_items must be greater than zero".into(),
        });
    }

    let valid = errors.is_empty();
    Ok(ValidateCollectionRuleResponse {
        valid,
        errors,
        rule_hash_input,
        rule_hash: valid.then_some(rule_hash),
    })
}

fn apply_rule_limit(
    mut items: Vec<CollectionMember>,
    rule: &DynamicCollectionRule,
) -> Vec<CollectionMember> {
    items.sort_by(|left, right| {
        left.position
            .cmp(&right.position)
            .then_with(|| left.item_key.cmp(&right.item_key))
    });
    if let Some(max_items) = rule.limit.max_items {
        items.truncate(max_items as usize);
    }
    items
}

fn collection_detail_from_create(
    request: CreateCollectionRequest,
) -> CollectionDetail {
    let now = Utc::now();
    let id = CollectionId::new();
    let provenance = request.provenance.unwrap_or(CollectionProvenance {
        source: request.source,
        ..CollectionProvenance::default()
    });
    let materialization_state = if request.rule.is_some() {
        CollectionMaterializationState::Pending
    } else {
        CollectionMaterializationState::Ready
    };

    CollectionDetail {
        summary: CollectionSummary {
            identity: CollectionIdentity::for_id(id),
            title: request.title,
            description: request.description,
            kind: request.kind,
            source: request.source,
            owner: request.owner,
            scope: request.scope,
            visibility: request.visibility,
            presentation: request.presentation,
            media_scope: request.media_scope,
            duplicate_policy: request.duplicate_policy,
            artwork: request.artwork,
            theme: request.theme,
            provenance,
            version: CollectionVersion {
                revision: 1,
                etag: Some(format!("collection-{}-1", id)),
                ..CollectionVersion::default()
            },
            timestamps: CollectionTimestamps {
                created_at: now,
                updated_at: now,
                archived_at: None,
            },
            item_count: 0,
            materialization: CollectionMaterializationStatus {
                state: materialization_state,
                ..CollectionMaterializationStatus::default()
            },
        },
        rule: request.rule,
        items_preview: Vec::new(),
        shelf_placements: Vec::new(),
    }
}

#[async_trait]
impl ApiService for TestApiService {
    async fn get_rkyv(
        &self,
        _path: &str,
        _query: Option<(&str, &str)>,
    ) -> RepositoryResult<AlignedVec> {
        Err(RepositoryError::QueryFailed(
            "TestApiService::get_rkyv not implemented".into(),
        ))
    }

    async fn get_bytes(
        &self,
        _path: &str,
        _query: Option<(&str, &str)>,
    ) -> RepositoryResult<Vec<u8>> {
        Err(RepositoryError::QueryFailed(
            "TestApiService::get_bytes not implemented".into(),
        ))
    }

    async fn get_image(
        &self,
        _path: &str,
        _size: ImageQuery,
    ) -> RepositoryResult<ImageFetchResult> {
        Err(RepositoryError::QueryFailed(
            "TestApiService::get_image not implemented".into(),
        ))
    }

    async fn post_image_manifest(
        &self,
        _request: ImageManifestRequest,
    ) -> RepositoryResult<ImageManifestResponse> {
        Err(RepositoryError::QueryFailed(
            "TestApiService::post_image_manifest not implemented".into(),
        ))
    }

    async fn fetch_libraries(&self) -> RepositoryResult<Vec<Library>> {
        Ok(self.inner.read().expect("lock poisoned").libraries.clone())
    }

    async fn fetch_library_media(
        &self,
        library_id: Uuid,
    ) -> RepositoryResult<Vec<Media>> {
        let guard = self.inner.read().expect("lock poisoned");
        Ok(guard
            .library_media
            .get(&library_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn fetch_movie_reference_batch(
        &self,
        _library_id: LibraryId,
        _batch_id: MovieBatchId,
    ) -> RepositoryResult<AlignedVec> {
        Err(RepositoryError::QueryFailed(
            "TestApiService::fetch_movie_reference_batch not implemented"
                .into(),
        ))
    }

    async fn fetch_movie_reference_batch_bundle(
        &self,
        _library_id: LibraryId,
    ) -> RepositoryResult<AlignedVec> {
        Err(RepositoryError::QueryFailed(
            "TestApiService::fetch_movie_reference_batch_bundle not implemented"
                .into(),
        ))
    }

    async fn sync_movie_reference_batches(
        &self,
        _library_id: LibraryId,
        _request: MovieBatchSyncRequest,
    ) -> RepositoryResult<MovieBatchSyncResponse> {
        Err(RepositoryError::QueryFailed(
            "TestApiService::sync_movie_reference_batches not implemented"
                .into(),
        ))
    }

    async fn fetch_movie_reference_batches(
        &self,
        _library_id: LibraryId,
        _request: MovieBatchFetchRequest,
    ) -> RepositoryResult<AlignedVec> {
        Err(RepositoryError::QueryFailed(
            "TestApiService::fetch_movie_reference_batches not implemented"
                .into(),
        ))
    }

    async fn fetch_series_bundle(
        &self,
        _library_id: LibraryId,
        _series_id: SeriesID,
    ) -> RepositoryResult<AlignedVec> {
        Err(RepositoryError::QueryFailed(
            "TestApiService::fetch_series_bundle not implemented".into(),
        ))
    }

    async fn fetch_series_bundle_bundle(
        &self,
        _library_id: LibraryId,
    ) -> RepositoryResult<AlignedVec> {
        Err(RepositoryError::QueryFailed(
            "TestApiService::fetch_series_bundle_bundle not implemented".into(),
        ))
    }

    async fn sync_series_bundles(
        &self,
        _library_id: LibraryId,
        _request: SeriesBundleSyncRequest,
    ) -> RepositoryResult<SeriesBundleSyncResponse> {
        Err(RepositoryError::QueryFailed(
            "TestApiService::sync_series_bundles not implemented".into(),
        ))
    }

    async fn fetch_series_bundles(
        &self,
        _library_id: LibraryId,
        _request: SeriesBundleFetchRequest,
    ) -> RepositoryResult<AlignedVec> {
        Err(RepositoryError::QueryFailed(
            "TestApiService::fetch_series_bundles not implemented".into(),
        ))
    }

    async fn create_library(
        &self,
        request: CreateLibraryRequest,
    ) -> RepositoryResult<LibraryId> {
        let CreateLibraryRequest {
            name,
            library_type,
            paths,
            scan_interval_minutes,
            enabled,
            auto_scan,
            watch_for_changes,
            movie_ref_batch_size,
            start_scan: _,
        } = request;

        let mut guard = self.inner.write().expect("lock poisoned");
        let library = Library {
            id: LibraryId::new(),
            name,
            library_type,
            paths: paths.into_iter().map(PathBuf::from).collect(),
            scan_interval_minutes,
            last_scan: None,
            enabled,
            auto_scan,
            watch_for_changes,
            analyze_on_scan: false,
            max_retry_attempts: 3,
            movie_ref_batch_size: ferrex_model::MovieReferenceBatchSize::new(
                movie_ref_batch_size,
            )
            .expect("movie_ref_batch_size must be valid"),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            media: None,
        };
        let id = library.id;
        guard.libraries.push(library);
        Ok(id)
    }

    async fn update_library(
        &self,
        id: LibraryId,
        request: UpdateLibraryRequest,
    ) -> RepositoryResult<()> {
        let mut guard = self.inner.write().expect("lock poisoned");
        if let Some(library) =
            guard.libraries.iter_mut().find(|lib| lib.id == id)
        {
            if let Some(name) = request.name {
                library.name = name;
            }
            if let Some(paths) = request.paths {
                library.paths = paths.into_iter().map(PathBuf::from).collect();
            }
            if let Some(scan_interval_minutes) = request.scan_interval_minutes {
                library.scan_interval_minutes = scan_interval_minutes;
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
            if let Some(size) = request.movie_ref_batch_size {
                library.movie_ref_batch_size =
                    ferrex_model::MovieReferenceBatchSize::new(size)
                        .expect("movie_ref_batch_size must be valid");
            }
            library.updated_at = Utc::now();
            Ok(())
        } else {
            Err(RepositoryError::NotFound {
                entity_type: "Library".into(),
                id: id.to_string(),
            })
        }
    }

    async fn delete_library(&self, id: LibraryId) -> RepositoryResult<()> {
        let mut guard = self.inner.write().expect("lock poisoned");
        guard.libraries.retain(|library| library.id != id);
        Ok(())
    }

    async fn start_library_scan(
        &self,
        _library_id: LibraryId,
        _request: StartScanRequest,
    ) -> RepositoryResult<ScanCommandAcceptedResponse> {
        let mode = ScanRunMode::Manual;
        let run_key = mode.run_key(_library_id);
        Ok(ScanCommandAcceptedResponse {
            scan_id: Uuid::now_v7(),
            correlation_id: Uuid::now_v7(),
            status: ScanLifecycleStatus::Running,
            mode,
            idempotency_key: run_key.clone(),
            run_key,
            disposition: ScanStartDisposition::Created,
        })
    }

    async fn pause_library_scan(
        &self,
        _library_id: LibraryId,
        _request: ScanCommandRequest,
    ) -> RepositoryResult<ScanCommandAcceptedResponse> {
        let mode = ScanRunMode::Manual;
        let run_key = mode.run_key(_library_id);
        Ok(ScanCommandAcceptedResponse {
            scan_id: Uuid::now_v7(),
            correlation_id: Uuid::now_v7(),
            status: ScanLifecycleStatus::Paused,
            mode,
            idempotency_key: run_key.clone(),
            run_key,
            disposition: ScanStartDisposition::Reused,
        })
    }

    async fn resume_library_scan(
        &self,
        _library_id: LibraryId,
        _request: ScanCommandRequest,
    ) -> RepositoryResult<ScanCommandAcceptedResponse> {
        let mode = ScanRunMode::Manual;
        let run_key = mode.run_key(_library_id);
        Ok(ScanCommandAcceptedResponse {
            scan_id: Uuid::now_v7(),
            correlation_id: Uuid::now_v7(),
            status: ScanLifecycleStatus::Running,
            mode,
            idempotency_key: run_key.clone(),
            run_key,
            disposition: ScanStartDisposition::Reused,
        })
    }

    async fn cancel_library_scan(
        &self,
        _library_id: LibraryId,
        _request: ScanCommandRequest,
    ) -> RepositoryResult<ScanCommandAcceptedResponse> {
        let mode = ScanRunMode::Manual;
        let run_key = mode.run_key(_library_id);
        Ok(ScanCommandAcceptedResponse {
            scan_id: Uuid::now_v7(),
            correlation_id: Uuid::now_v7(),
            status: ScanLifecycleStatus::Canceled,
            mode,
            idempotency_key: run_key.clone(),
            run_key,
            disposition: ScanStartDisposition::Reused,
        })
    }

    async fn fetch_active_scans(
        &self,
    ) -> RepositoryResult<ActiveScansResponse> {
        serde_json::from_value(serde_json::json!({
            "scans": [],
            "count": 0,
            "incremental": false,
        }))
        .map_err(|err| RepositoryError::QueryFailed(err.to_string()))
    }

    async fn fetch_latest_scan_progress(
        &self,
        _scan_id: Uuid,
    ) -> RepositoryResult<LatestProgressResponse> {
        Err(RepositoryError::QueryFailed(
            "Scan progress not available in tests".into(),
        ))
    }

    async fn fetch_scan_metrics(&self) -> RepositoryResult<ScanMetrics> {
        Err(RepositoryError::QueryFailed(
            "Scan metrics not available in tests".into(),
        ))
    }

    async fn fetch_scan_config(&self) -> RepositoryResult<ScanConfig> {
        Err(RepositoryError::QueryFailed(
            "Scan config not available in tests".into(),
        ))
    }

    async fn browse_media_root(
        &self,
        _path: Option<&str>,
    ) -> RepositoryResult<MediaRootBrowseResponse> {
        Err(RepositoryError::QueryFailed(
            "Media root browser not available in test stub".into(),
        ))
    }

    async fn health_check(&self) -> RepositoryResult<bool> {
        Ok(true)
    }

    #[cfg(feature = "demo")]
    async fn fetch_demo_status(&self) -> RepositoryResult<DemoStatus> {
        Err(RepositoryError::QueryFailed(
            "Demo status not available in test stub".into(),
        ))
    }

    #[cfg(feature = "demo")]
    async fn reset_demo(
        &self,
        _request: DemoResetRequest,
    ) -> RepositoryResult<DemoStatus> {
        Err(RepositoryError::UpdateFailed(
            "Demo reset not available in test stub".into(),
        ))
    }

    #[cfg(feature = "demo")]
    async fn resize_demo(
        &self,
        _request: DemoResetRequest,
    ) -> RepositoryResult<DemoStatus> {
        Err(RepositoryError::UpdateFailed(
            "Demo resize not available in test stub".into(),
        ))
    }

    async fn get_watch_state(&self) -> RepositoryResult<UserWatchState> {
        Ok(self
            .inner
            .read()
            .expect("lock poisoned")
            .watch_state
            .clone())
    }

    async fn update_progress(
        &self,
        request: &UpdateProgressRequest,
    ) -> RepositoryResult<()> {
        if let Ok(mut guard) = self.inner.write() {
            guard.watch_state.update_progress(
                request.media_id,
                request.position,
                request.duration,
            );
        }
        Ok(())
    }

    async fn get_series_watch_state(
        &self,
        _tmdb_series_id: u64,
    ) -> RepositoryResult<ferrex_core::player_prelude::SeriesWatchStatus> {
        Err(RepositoryError::QueryFailed(
            "get_series_watch_state not implemented in test stub".into(),
        ))
    }

    async fn get_season_watch_state(
        &self,
        _tmdb_series_id: u64,
        _season_number: u16,
    ) -> RepositoryResult<ferrex_core::player_prelude::SeasonWatchStatus> {
        Err(RepositoryError::QueryFailed(
            "get_season_watch_state not implemented in test stub".into(),
        ))
    }

    async fn get_series_next_episode(
        &self,
        _tmdb_series_id: u64,
    ) -> RepositoryResult<Option<ferrex_core::player_prelude::NextEpisode>>
    {
        Ok(None)
    }

    async fn list_user_devices(
        &self,
    ) -> RepositoryResult<Vec<AuthenticatedDevice>> {
        Ok(self.inner.read().expect("lock poisoned").devices.clone())
    }

    async fn revoke_device(&self, device_id: Uuid) -> RepositoryResult<()> {
        if let Ok(mut guard) = self.inner.write() {
            guard.devices.retain(|device| device.id != device_id);
        }
        Ok(())
    }

    async fn query_media(
        &self,
        _query: MediaQuery,
    ) -> RepositoryResult<Vec<MediaWithStatus>> {
        Ok(Vec::new())
    }

    async fn list_collections(
        &self,
        request: ListCollectionsRequest,
    ) -> RepositoryResult<ListCollectionsResponse> {
        let mut guard = self.inner.write().expect("lock poisoned");
        if let Some(message) = guard.take_collection_query_error() {
            return Err(RepositoryError::QueryFailed(message));
        }

        let mut summaries = Vec::new();
        for collection_id in &guard.collection_order {
            let Some(record) = guard.collections.get(collection_id) else {
                continue;
            };
            let summary = &record.detail.summary;
            if !request.include_archived
                && summary.timestamps.archived_at.is_some()
            {
                continue;
            }
            if let Some(kind) = request.kind
                && summary.kind != kind
            {
                continue;
            }
            if let Some(scope) = request.scope
                && summary.scope != scope
            {
                continue;
            }
            if let Some(visibility) = request.visibility
                && summary.visibility != visibility
            {
                continue;
            }
            if let Some(media_type) = request.media_type
                && !collection_media_scope_matches(
                    &summary.media_scope,
                    media_type,
                )
            {
                continue;
            }

            let mut summary = summary.clone();
            if request.include_item_counts {
                summary.item_count = record.items.len() as u32;
                summary.materialization.item_count = record.items.len() as u32;
            }
            summaries.push(summary);
        }

        let (collections, page) = paginate(&summaries, &request.page);
        Ok(ListCollectionsResponse { collections, page })
    }

    async fn get_collection_detail(
        &self,
        collection_id: CollectionId,
        request: GetCollectionDetailRequest,
    ) -> RepositoryResult<GetCollectionDetailResponse> {
        let mut guard = self.inner.write().expect("lock poisoned");
        if let Some(message) = guard.take_collection_query_error() {
            return Err(RepositoryError::QueryFailed(message));
        }

        let record = guard.collection_record(collection_id)?;
        Ok(GetCollectionDetailResponse {
            collection: detail_with_expansions(
                record,
                &guard.shelf_placements,
                &request,
            ),
        })
    }

    async fn list_collection_items(
        &self,
        collection_id: CollectionId,
        request: ListCollectionItemsRequest,
    ) -> RepositoryResult<ListCollectionItemsResponse> {
        let mut guard = self.inner.write().expect("lock poisoned");
        if let Some(message) = guard.take_collection_query_error() {
            return Err(RepositoryError::QueryFailed(message));
        }

        let record = guard.collection_record(collection_id)?;
        let items: Vec<_> = record
            .items
            .iter()
            .filter(|item| {
                request
                    .availability
                    .is_none_or(|status| item.availability.status == status)
            })
            .cloned()
            .collect();
        let (items, page) = paginate(&items, &request.page);
        Ok(ListCollectionItemsResponse {
            collection_id,
            items,
            page,
            materialization: record.detail.summary.materialization.clone(),
        })
    }

    async fn create_collection(
        &self,
        request: CreateCollectionRequest,
    ) -> RepositoryResult<CreateCollectionResponse> {
        let mut guard = self.inner.write().expect("lock poisoned");
        if let Some(message) = guard.take_collection_write_error() {
            return Err(RepositoryError::CreateFailed(message));
        }

        let detail = collection_detail_from_create(request);
        let collection_id = detail.summary.identity.id;
        guard.collection_order.push(collection_id);
        guard.collections.insert(
            collection_id,
            CollectionRecord {
                detail: detail.clone(),
                items: Vec::new(),
            },
        );
        Ok(CreateCollectionResponse { collection: detail })
    }

    async fn update_collection(
        &self,
        collection_id: CollectionId,
        request: UpdateCollectionRequest,
    ) -> RepositoryResult<UpdateCollectionResponse> {
        let mut guard = self.inner.write().expect("lock poisoned");
        if let Some(message) = guard.take_collection_write_error() {
            return Err(RepositoryError::UpdateFailed(message));
        }

        let record = guard.collection_record_mut(collection_id)?;
        ensure_expected_revision(
            &record.detail.summary,
            request.expected_revision,
        )?;

        if let Some(title) = request.title {
            record.detail.summary.title = title;
        }
        if let Some(description) = request.description {
            record.detail.summary.description = Some(description);
        }
        if let Some(visibility) = request.visibility {
            record.detail.summary.visibility = visibility;
        }
        if let Some(presentation) = request.presentation {
            record.detail.summary.presentation = presentation;
        }
        if let Some(media_scope) = request.media_scope {
            record.detail.summary.media_scope = media_scope;
        }
        if let Some(duplicate_policy) = request.duplicate_policy {
            record.detail.summary.duplicate_policy = duplicate_policy;
        }
        if let Some(artwork) = request.artwork {
            record.detail.summary.artwork = artwork;
        }
        if let Some(theme) = request.theme {
            record.detail.summary.theme = theme;
        }
        if let Some(rule) = request.rule {
            record.detail.rule = Some(rule);
            record.detail.summary.kind = CollectionKind::DynamicRule;
            record.detail.summary.source = CollectionSource::DynamicRule;
            record.detail.summary.materialization.state =
                CollectionMaterializationState::Stale;
        }
        bump_collection_version(&mut record.detail.summary);
        guard.sync_collection(collection_id);

        let record = guard.collection_record(collection_id)?;
        Ok(UpdateCollectionResponse {
            collection: detail_with_expansions(
                record,
                &guard.shelf_placements,
                &GetCollectionDetailRequest {
                    include_rule: true,
                    include_items_preview: true,
                    include_shelf_placements: true,
                },
            ),
        })
    }

    async fn archive_collection(
        &self,
        collection_id: CollectionId,
        request: ArchiveCollectionRequest,
    ) -> RepositoryResult<ArchiveCollectionResponse> {
        let mut guard = self.inner.write().expect("lock poisoned");
        if let Some(message) = guard.take_collection_write_error() {
            return Err(RepositoryError::UpdateFailed(message));
        }

        let record = guard.collection_record_mut(collection_id)?;
        ensure_expected_revision(
            &record.detail.summary,
            request.expected_revision,
        )?;
        let archived_at = request.archived.then(Utc::now);
        record.detail.summary.timestamps.archived_at = archived_at;
        bump_collection_version(&mut record.detail.summary);
        Ok(ArchiveCollectionResponse {
            collection_id,
            archived_at,
            version: record.detail.summary.version.clone(),
        })
    }

    async fn manual_add_collection_items(
        &self,
        collection_id: CollectionId,
        request: ManualAddCollectionItemsRequest,
    ) -> RepositoryResult<ManualAddCollectionItemsResponse> {
        let mut guard = self.inner.write().expect("lock poisoned");
        if let Some(message) = guard.take_collection_write_error() {
            return Err(RepositoryError::UpdateFailed(message));
        }

        let record = guard.collection_record_mut(collection_id)?;
        ensure_expected_revision(
            &record.detail.summary,
            request.expected_revision,
        )?;
        let policy = request
            .duplicate_policy
            .unwrap_or(record.detail.summary.duplicate_policy);
        let mut results = Vec::with_capacity(request.items.len());
        let mut changed = false;

        for item in request.items {
            let item_key = CollectionMemberKey::for_media(&item.media_id);
            let already_present = record
                .items
                .iter()
                .any(|member| member.item_key == item_key);

            if already_present {
                match policy {
                    CollectionDuplicatePolicy::RejectDuplicates => {
                        return Err(RepositoryError::UpdateFailed(format!(
                            "Duplicate collection item conflict: {} already exists in {}",
                            item_key, collection_id
                        )));
                    }
                    CollectionDuplicatePolicy::KeepAll => {}
                    CollectionDuplicatePolicy::DeduplicateMedia
                    | CollectionDuplicatePolicy::DeduplicateLogical => {
                        results.push(CollectionManualAddResult {
                            item_key,
                            status: CollectionManualAddStatus::DuplicateSkipped,
                            message: Some(
                                "Item is already present in this collection"
                                    .into(),
                            ),
                        });
                        continue;
                    }
                }
            }

            let position = item.position.unwrap_or_else(|| {
                record
                    .items
                    .iter()
                    .map(|member| member.position)
                    .max()
                    .unwrap_or(0)
                    .saturating_add(1)
            });
            let mut member = CollectionMember::new(
                item.media_id,
                item.title_override
                    .unwrap_or_else(|| item.media_id.to_string()),
                position,
            );
            member.added_at = Some(Utc::now());
            record.items.push(member);
            results.push(CollectionManualAddResult {
                item_key,
                status: CollectionManualAddStatus::Added,
                message: None,
            });
            changed = true;
        }

        if changed {
            bump_collection_version(&mut record.detail.summary);
            guard.sync_collection(collection_id);
        }
        let version = guard
            .collection_record(collection_id)?
            .detail
            .summary
            .version
            .clone();
        Ok(ManualAddCollectionItemsResponse {
            collection_id,
            results,
            version,
        })
    }

    async fn manual_remove_collection_items(
        &self,
        collection_id: CollectionId,
        request: ManualRemoveCollectionItemsRequest,
    ) -> RepositoryResult<ManualRemoveCollectionItemsResponse> {
        let mut guard = self.inner.write().expect("lock poisoned");
        if let Some(message) = guard.take_collection_write_error() {
            return Err(RepositoryError::UpdateFailed(message));
        }

        let record = guard.collection_record_mut(collection_id)?;
        ensure_expected_revision(
            &record.detail.summary,
            request.expected_revision,
        )?;
        let mut removed_item_keys = Vec::new();
        let mut missing_item_keys = Vec::new();

        for item_key in request.item_keys {
            let before = record.items.len();
            record.items.retain(|member| member.item_key != item_key);
            if record.items.len() < before {
                removed_item_keys.push(item_key);
            } else {
                missing_item_keys.push(item_key);
            }
        }

        if !removed_item_keys.is_empty() {
            bump_collection_version(&mut record.detail.summary);
            guard.sync_collection(collection_id);
        }
        let version = guard
            .collection_record(collection_id)?
            .detail
            .summary
            .version
            .clone();
        Ok(ManualRemoveCollectionItemsResponse {
            collection_id,
            removed_item_keys,
            missing_item_keys,
            version,
        })
    }

    async fn manual_reorder_collection_items(
        &self,
        collection_id: CollectionId,
        request: ManualReorderCollectionItemsRequest,
    ) -> RepositoryResult<ManualReorderCollectionItemsResponse> {
        let mut guard = self.inner.write().expect("lock poisoned");
        if let Some(message) = guard.take_collection_write_error() {
            return Err(RepositoryError::UpdateFailed(message));
        }

        let record = guard.collection_record_mut(collection_id)?;
        ensure_expected_revision(
            &record.detail.summary,
            request.expected_revision,
        )?;
        let mut changed = false;
        for order in request.ordering {
            if let Some(member) = record
                .items
                .iter_mut()
                .find(|member| member.item_key == order.item_key)
                && member.position != order.position
            {
                member.position = order.position;
                changed = true;
            }
        }
        if changed {
            bump_collection_version(&mut record.detail.summary);
            guard.sync_collection(collection_id);
        }
        let version = guard
            .collection_record(collection_id)?
            .detail
            .summary
            .version
            .clone();
        Ok(ManualReorderCollectionItemsResponse {
            collection_id,
            version,
        })
    }

    async fn validate_collection_rule(
        &self,
        request: ValidateCollectionRuleRequest,
    ) -> RepositoryResult<ValidateCollectionRuleResponse> {
        let mut guard = self.inner.write().expect("lock poisoned");
        if let Some(message) = guard.take_collection_query_error() {
            return Err(RepositoryError::QueryFailed(message));
        }
        validate_rule(&request.rule)
    }

    async fn preview_collection_rule(
        &self,
        request: PreviewCollectionRuleRequest,
    ) -> RepositoryResult<PreviewCollectionRuleResponse> {
        let mut guard = self.inner.write().expect("lock poisoned");
        if let Some(message) = guard.take_collection_query_error() {
            return Err(RepositoryError::QueryFailed(message));
        }
        let validation = validate_rule(&request.rule)?;
        if !validation.valid {
            return Ok(PreviewCollectionRuleResponse {
                items: Vec::new(),
                page: CollectionPageInfo {
                    limit: request.page.limit,
                    ..CollectionPageInfo::default()
                },
                materialization: CollectionMaterializationStatus {
                    state: CollectionMaterializationState::Failed,
                    last_error: Some(
                        validation
                            .errors
                            .iter()
                            .map(|error| error.message.as_str())
                            .collect::<Vec<_>>()
                            .join("; "),
                    ),
                    ..CollectionMaterializationStatus::default()
                },
                rule_hash_input: validation.rule_hash_input,
                rule_hash: None,
            });
        }

        let items = guard
            .collections
            .values()
            .flat_map(|record| record.items.iter().cloned())
            .collect::<Vec<_>>();
        let items = apply_rule_limit(items, &request.rule);
        let total = items.len() as u32;
        let (items, page) = paginate(&items, &request.page);
        Ok(PreviewCollectionRuleResponse {
            items,
            page,
            materialization: CollectionMaterializationStatus {
                state: CollectionMaterializationState::Ready,
                item_count: total,
                rule_hash: validation.rule_hash.clone(),
                generated_at: Some(Utc::now()),
                ..CollectionMaterializationStatus::default()
            },
            rule_hash_input: validation.rule_hash_input,
            rule_hash: validation.rule_hash,
        })
    }

    async fn refresh_collection_rule(
        &self,
        collection_id: CollectionId,
        request: RefreshCollectionRuleRequest,
    ) -> RepositoryResult<RefreshCollectionRuleResponse> {
        let mut guard = self.inner.write().expect("lock poisoned");
        if let Some(message) = guard.take_collection_write_error() {
            return Err(RepositoryError::UpdateFailed(message));
        }

        let record = guard.collection_record_mut(collection_id)?;
        let rule = record.detail.rule.as_ref().ok_or_else(|| {
            RepositoryError::UpdateFailed(format!(
                "Collection {} does not have a dynamic rule",
                collection_id
            ))
        })?;
        let (_, rule_hash) = collection_rule_hash(rule)?;
        if let Some(expected) = request.expected_rule_hash.as_deref()
            && expected != rule_hash
        {
            return Err(RepositoryError::UpdateFailed(format!(
                "Collection rule conflict: expected hash {}, found {}",
                expected, rule_hash
            )));
        }

        record.detail.summary.materialization =
            CollectionMaterializationStatus {
                state: CollectionMaterializationState::Ready,
                item_count: record.items.len() as u32,
                rule_hash: Some(rule_hash.clone()),
                generated_at: Some(Utc::now()),
                ..CollectionMaterializationStatus::default()
            };
        record.detail.summary.provenance.rule_hash = Some(rule_hash);
        record.detail.summary.provenance.last_refreshed_at = Some(Utc::now());
        bump_collection_version(&mut record.detail.summary);
        Ok(RefreshCollectionRuleResponse {
            collection_id,
            materialization: record.detail.summary.materialization.clone(),
            version: record.detail.summary.version.clone(),
        })
    }

    async fn list_shelf_placements(
        &self,
        request: ListShelfPlacementsRequest,
    ) -> RepositoryResult<ListShelfPlacementsResponse> {
        let mut guard = self.inner.write().expect("lock poisoned");
        if let Some(message) = guard.take_collection_query_error() {
            return Err(RepositoryError::QueryFailed(message));
        }

        let mut placements: Vec<_> = guard
            .shelf_placements
            .iter()
            .filter(|placement| request.include_unpinned || placement.pinned)
            .filter(|placement| {
                request
                    .surface
                    .is_none_or(|surface| placement.surface == surface)
            })
            .filter(|placement| {
                request
                    .shelf_key
                    .as_ref()
                    .is_none_or(|shelf_key| &placement.shelf_key == shelf_key)
            })
            .cloned()
            .collect();
        placements.sort_by(|left, right| {
            left.position
                .cmp(&right.position)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(ListShelfPlacementsResponse { placements })
    }

    async fn pin_shelf_placement(
        &self,
        request: PinShelfPlacementRequest,
    ) -> RepositoryResult<PinShelfPlacementResponse> {
        let mut guard = self.inner.write().expect("lock poisoned");
        if let Some(message) = guard.take_collection_write_error() {
            return Err(RepositoryError::UpdateFailed(message));
        }

        let summary = guard
            .collection_record(request.collection_id)?
            .detail
            .summary
            .clone();
        let now = Utc::now();
        let position = request.position.unwrap_or_else(|| {
            guard
                .shelf_placements
                .iter()
                .filter(|placement| {
                    placement.surface == request.surface
                        && placement.shelf_key == request.shelf_key
                })
                .map(|placement| placement.position)
                .max()
                .unwrap_or(0)
                .saturating_add(1)
        });

        if let Some(placement) =
            guard.shelf_placements.iter_mut().find(|placement| {
                placement.collection_id == request.collection_id
                    && placement.surface == request.surface
                    && placement.shelf_key == request.shelf_key
            })
        {
            placement.position = position;
            placement.pinned = request.pinned;
            if let Some(presentation) = request.presentation {
                placement.presentation = presentation;
            }
            placement.updated_at = now;
            return Ok(PinShelfPlacementResponse {
                placement: placement.clone(),
            });
        }

        let placement = ShelfPlacement {
            schema_version: SHELF_PLACEMENT_SCHEMA_VERSION,
            id: ShelfPlacementId::new(),
            collection_id: request.collection_id,
            surface: request.surface,
            shelf_key: request.shelf_key,
            position,
            pinned: request.pinned,
            presentation: request.presentation.unwrap_or(summary.presentation),
            visibility: summary.visibility,
            created_at: now,
            updated_at: now,
        };
        guard.shelf_placements.push(placement.clone());
        Ok(PinShelfPlacementResponse { placement })
    }

    async fn reorder_shelf_placements(
        &self,
        request: ReorderShelfPlacementsRequest,
    ) -> RepositoryResult<ReorderShelfPlacementsResponse> {
        let mut guard = self.inner.write().expect("lock poisoned");
        if let Some(message) = guard.take_collection_write_error() {
            return Err(RepositoryError::UpdateFailed(message));
        }

        let now = Utc::now();
        for order in request.ordering {
            if let Some(placement) = guard
                .shelf_placements
                .iter_mut()
                .find(|placement| placement.id == order.placement_id)
            {
                placement.position = order.position;
                placement.updated_at = now;
            }
        }
        guard.shelf_placements.sort_by(|left, right| {
            left.position
                .cmp(&right.position)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(ReorderShelfPlacementsResponse {
            placements: guard.shelf_placements.clone(),
        })
    }

    async fn list_tmdb_collections(
        &self,
        request: TmdbListCollectionsRequest,
    ) -> RepositoryResult<TmdbListCollectionsResponse> {
        let mut guard = self.inner.write().expect("lock poisoned");
        if let Some(message) = guard.take_collection_query_error() {
            return Err(RepositoryError::QueryFailed(message));
        }

        let collections: Vec<_> = guard
            .tmdb_collections
            .iter()
            .filter(|summary| {
                request
                    .import_kind
                    .is_none_or(|kind| summary.import_kind == kind)
            })
            .cloned()
            .collect();
        let (collections, page) = paginate(&collections, &request.page);
        Ok(TmdbListCollectionsResponse { collections, page })
    }

    async fn import_tmdb_collection(
        &self,
        request: TmdbImportCollectionRequest,
    ) -> RepositoryResult<TmdbImportCollectionResponse> {
        let mut guard = self.inner.write().expect("lock poisoned");
        if let Some(message) = guard.take_collection_write_error() {
            return Err(RepositoryError::CreateFailed(message));
        }

        let now = Utc::now();
        let existing_id =
            guard
                .collections
                .iter()
                .find_map(|(collection_id, record)| {
                    (record.detail.summary.source == CollectionSource::Tmdb
                        && record
                            .detail
                            .summary
                            .provenance
                            .external_id
                            .as_deref()
                            == Some(request.tmdb_id.as_str()))
                    .then_some(*collection_id)
                });

        if let Some(collection_id) = existing_id {
            if !request.refresh_existing {
                return Err(RepositoryError::CreateFailed(format!(
                    "Duplicate TMDB collection conflict: {} is already imported",
                    request.tmdb_id
                )));
            }
            let placements = guard.shelf_placements.clone();
            let record = guard.collection_record_mut(collection_id)?;
            record.detail.summary.provenance.last_refreshed_at = Some(now);
            bump_collection_version(&mut record.detail.summary);
            let imported_items = record.items.len() as u32;
            let collection = detail_with_expansions(
                record,
                &placements,
                &GetCollectionDetailRequest {
                    include_rule: true,
                    include_items_preview: true,
                    include_shelf_placements: true,
                },
            );
            return Ok(TmdbImportCollectionResponse {
                collection,
                imported_items,
                skipped_items: 0,
                warnings: Vec::new(),
            });
        }

        let tmdb_summary = guard
            .tmdb_collections
            .iter()
            .find(|summary| summary.tmdb_id == request.tmdb_id)
            .cloned();
        let title = request
            .title_override
            .clone()
            .or_else(|| {
                tmdb_summary.as_ref().map(|summary| summary.title.clone())
            })
            .unwrap_or_else(|| format!("TMDB {}", request.tmdb_id));
        let description = tmdb_summary.and_then(|summary| summary.description);
        let kind = match request.import_kind {
            TmdbCollectionImportKind::Collection => {
                CollectionKind::TmdbCollection
            }
            TmdbCollectionImportKind::List
            | TmdbCollectionImportKind::Keyword => CollectionKind::TmdbList,
        };
        let detail = collection_detail_from_create(CreateCollectionRequest {
            title,
            description,
            kind,
            source: CollectionSource::Tmdb,
            owner: request.owner,
            scope: CollectionScope::Global,
            visibility: request.visibility,
            presentation: request.presentation,
            media_scope: request.media_scope,
            duplicate_policy: request.duplicate_policy,
            artwork: CollectionArtwork::default(),
            theme: CollectionTheme::default(),
            provenance: Some(CollectionProvenance {
                source: CollectionSource::Tmdb,
                imported_from: Some("tmdb".into()),
                external_id: Some(request.tmdb_id.clone()),
                generated_by: None,
                rule_hash: None,
                last_refreshed_at: Some(now),
            }),
            rule: None,
        });
        let collection_id = detail.summary.identity.id;
        guard.collection_order.push(collection_id);
        guard.collections.insert(
            collection_id,
            CollectionRecord {
                detail: detail.clone(),
                items: Vec::new(),
            },
        );
        Ok(TmdbImportCollectionResponse {
            collection: detail,
            imported_items: 0,
            skipped_items: 0,
            warnings: Vec::new(),
        })
    }

    async fn refresh_tmdb_collection(
        &self,
        mut request: TmdbImportCollectionRequest,
    ) -> RepositoryResult<TmdbImportCollectionResponse> {
        request.refresh_existing = true;
        self.import_tmdb_collection(request).await
    }

    async fn fetch_filtered_indices(
        &self,
        _library_id: Uuid,
        _spec: &FilterIndicesRequest,
    ) -> RepositoryResult<Vec<u32>> {
        Ok(Vec::new())
    }

    async fn check_setup_status(&self) -> RepositoryResult<crate::SetupStatus> {
        let guard = self.inner.read().expect("lock poisoned");
        Ok(crate::SetupStatus {
            needs_setup: guard.setup_required,
            has_admin: !guard.setup_required,
            user_count: guard.current_user.iter().count(),
            library_count: guard.libraries.len(),
            requires_setup_token: guard.setup_token_required,
            ..Default::default()
        })
    }

    async fn create_initial_admin(
        &self,
        username: String,
        password: String,
        display_name: Option<String>,
        _setup_token: Option<String>,
        claim_token: Option<String>,
    ) -> RepositoryResult<(User, AuthToken)> {
        if claim_token.is_none() {
            return Err(RepositoryError::QueryFailed(
                "Claim token required in test stub".into(),
            ));
        }

        let user_id = Uuid::now_v7();
        let user = User {
            id: user_id,
            username: username.clone(),
            display_name: display_name.unwrap_or_else(|| username.clone()),
            avatar_url: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_login: Some(Utc::now()),
            is_active: true,
            email: Some("admin@example.com".into()),
            preferences: UserPreferences::default(),
        };

        let permissions = sample_permissions(user_id);

        let token = AuthToken {
            access_token: format!("admin-{}", password),
            refresh_token: format!("refresh-{}", Uuid::now_v7()),
            expires_in: 3600,
            session_id: Some(Uuid::now_v7()),
            device_session_id: Some(Uuid::now_v7()),
            user_id: Some(user_id),
            scope: SessionScope::Full,
        };

        if let Ok(mut guard) = self.inner.write() {
            guard.setup_required = false;
            guard.auth_token = Some(token.clone());
            guard.current_user = Some(user.clone());
            guard.current_permissions = Some(permissions.clone());
        }

        Ok((user, token))
    }

    async fn start_setup_claim(
        &self,
        device_name: Option<String>,
    ) -> RepositoryResult<StartClaimResponse> {
        let response = StartClaimResponse {
            claim_id: Uuid::now_v7(),
            claim_code: "123456".into(),
            expires_at: Utc::now() + Duration::minutes(5),
            lan_only: true,
        };

        if let Ok(mut guard) = self.inner.write() {
            let mut resp = response.clone();
            if let Some(name) = device_name {
                resp.claim_code = format!("{}-CLAIM", name.to_uppercase());
            }
            guard.last_claim = Some(resp.clone());
            return Ok(resp);
        }

        Ok(response)
    }

    async fn confirm_setup_claim(
        &self,
        claim_code: String,
    ) -> RepositoryResult<ConfirmClaimResponse> {
        let mut token = format!("{}-TOKEN", claim_code.to_uppercase());
        if token.is_empty() {
            token = "TEST-CLAIM".into();
        }
        Ok(ConfirmClaimResponse {
            claim_id: Uuid::now_v7(),
            claim_token: token,
            expires_at: Utc::now() + Duration::minutes(10),
        })
    }

    async fn fetch_current_user(&self) -> RepositoryResult<User> {
        self.inner
            .read()
            .expect("lock poisoned")
            .current_user
            .clone()
            .ok_or_else(|| {
                RepositoryError::QueryFailed(
                    "No current user available in TestApiService".into(),
                )
            })
    }

    async fn fetch_my_permissions(&self) -> RepositoryResult<UserPermissions> {
        self.inner
            .read()
            .expect("lock poisoned")
            .current_permissions
            .clone()
            .ok_or_else(|| {
                RepositoryError::QueryFailed(
                    "No permissions available in TestApiService".into(),
                )
            })
    }

    fn build_url(&self, path: &str) -> String {
        let base = self.base_url.trim_end_matches('/');
        let path = path.trim_start_matches('/');
        format!("{}/{}", base, path)
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    async fn set_token(&self, token: Option<AuthToken>) {
        if let Ok(mut guard) = self.inner.write() {
            guard.auth_token = token;
        }
    }

    async fn get_token(&self) -> Option<AuthToken> {
        self.inner.read().expect("lock poisoned").auth_token.clone()
    }

    async fn fetch_playback_ticket(
        &self,
        _media_id: &str,
    ) -> RepositoryResult<String> {
        match self
            .inner
            .read()
            .expect("lock poisoned")
            .playback_ticket_result
            .clone()
        {
            Some(Ok(token)) => Ok(token),
            Some(Err(message)) => Err(RepositoryError::QueryFailed(message)),
            None => Err(RepositoryError::QueryFailed(
                "TestApiService::fetch_playback_ticket not implemented".into(),
            )),
        }
    }
}

fn sample_collections() -> CollectionFixtures {
    let now = Utc::now();
    let collection_id = CollectionId::new();
    let first_movie = MediaID::Movie(MovieID(Uuid::now_v7()));
    let second_movie = MediaID::Movie(MovieID(Uuid::now_v7()));
    let items = vec![
        CollectionMember {
            added_at: Some(now - Duration::minutes(10)),
            ..CollectionMember::new(first_movie, "First Sample Movie", 1)
        },
        CollectionMember {
            added_at: Some(now - Duration::minutes(5)),
            ..CollectionMember::new(second_movie, "Second Sample Movie", 2)
        },
    ];
    let detail = CollectionDetail {
        summary: CollectionSummary {
            identity: CollectionIdentity::for_id(collection_id),
            title: "Sample Collection".into(),
            description: Some(
                "A stable in-memory collection for UI tests".into(),
            ),
            kind: CollectionKind::Manual,
            source: CollectionSource::Manual,
            owner: CollectionOwner::default(),
            scope: CollectionScope::User,
            visibility: CollectionVisibility::Private,
            presentation: CollectionPresentationMode::Shelf,
            media_scope: CollectionMediaScope::Types {
                media_types: vec![CollectionMediaKind::Movie],
            },
            duplicate_policy: CollectionDuplicatePolicy::DeduplicateMedia,
            artwork: CollectionArtwork::default(),
            theme: CollectionTheme::default(),
            provenance: CollectionProvenance::default(),
            version: CollectionVersion {
                revision: 1,
                etag: Some(format!("collection-{}-1", collection_id)),
                ..CollectionVersion::default()
            },
            timestamps: CollectionTimestamps {
                created_at: now - Duration::days(1),
                updated_at: now,
                archived_at: None,
            },
            item_count: items.len() as u32,
            materialization: CollectionMaterializationStatus {
                state: CollectionMaterializationState::Ready,
                item_count: items.len() as u32,
                generated_at: Some(now),
                ..CollectionMaterializationStatus::default()
            },
        },
        rule: None,
        items_preview: items.clone(),
        shelf_placements: Vec::new(),
    };

    (
        HashMap::from([(collection_id, CollectionRecord { detail, items })]),
        vec![collection_id],
    )
}

fn sample_tmdb_collections() -> Vec<TmdbCollectionSummary> {
    vec![
        TmdbCollectionSummary {
            tmdb_id: "550".into(),
            title: "Sample TMDB List".into(),
            description: Some("A stable TMDB list fixture".into()),
            import_kind: TmdbCollectionImportKind::List,
            poster_path: Some("/sample-list.jpg".into()),
            item_count: 12,
        },
        TmdbCollectionSummary {
            tmdb_id: "collection-42".into(),
            title: "Sample TMDB Collection".into(),
            description: None,
            import_kind: TmdbCollectionImportKind::Collection,
            poster_path: None,
            item_count: 3,
        },
    ]
}

fn sample_library(name: &str) -> Library {
    Library {
        id: LibraryId::new(),
        name: name.into(),
        library_type: LibraryType::Movies,
        paths: vec![PathBuf::from("/var/lib/ferrex")],
        scan_interval_minutes: 120,
        last_scan: None,
        enabled: true,
        auto_scan: false,
        watch_for_changes: false,
        analyze_on_scan: false,
        max_retry_attempts: 3,
        movie_ref_batch_size: MovieReferenceBatchSize::default(),
        created_at: Utc::now() - Duration::days(1),
        updated_at: Utc::now(),
        media: None,
    }
}

fn sample_device(user_id: Uuid) -> AuthenticatedDevice {
    AuthenticatedDevice {
        id: Uuid::now_v7(),
        user_id,
        fingerprint: "test-device".into(),
        name: "Ferrex Player".into(),
        platform: Platform::Linux,
        app_version: Some("tester".into()),
        hardware_id: None,
        status: AuthDeviceStatus::Trusted,
        pin_configured: false,
        failed_attempts: 0,
        locked_until: None,
        first_authenticated_by: user_id,
        first_authenticated_at: Utc::now() - Duration::days(1),
        trusted_until: Some(Utc::now() + Duration::days(30)),
        last_seen_at: Utc::now(),
        last_activity: Utc::now(),
        auto_login_enabled: true,
        revoked_by: None,
        revoked_at: None,
        revoked_reason: None,
        created_at: Utc::now() - Duration::days(1),
        updated_at: Utc::now(),
        metadata: serde_json::json!({"source": "test"}),
    }
}

fn sample_user(username: &str) -> User {
    User {
        id: Uuid::now_v7(),
        username: username.into(),
        display_name: username.into(),
        avatar_url: None,
        created_at: Utc::now() - Duration::hours(1),
        updated_at: Utc::now(),
        last_login: Some(Utc::now()),
        is_active: true,
        email: Some(format!("{}@example.com", username)),
        preferences: UserPreferences::default(),
    }
}

fn sample_permissions(user_id: Uuid) -> UserPermissions {
    UserPermissions {
        user_id,
        roles: vec![Role {
            id: Uuid::now_v7(),
            name: "admin".into(),
            description: Some("Administrator".into()),
            is_system: true,
            created_at: Utc::now().timestamp(),
        }],
        permissions: HashMap::from([
            ("system:admin".into(), true),
            ("user:create".into(), true),
        ]),
        permission_details: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_request(title: &str) -> CreateCollectionRequest {
        CreateCollectionRequest {
            title: title.into(),
            description: None,
            kind: CollectionKind::Manual,
            source: CollectionSource::Manual,
            owner: CollectionOwner::default(),
            scope: CollectionScope::User,
            visibility: CollectionVisibility::Private,
            presentation: CollectionPresentationMode::Shelf,
            media_scope: CollectionMediaScope::Types {
                media_types: vec![CollectionMediaKind::Movie],
            },
            duplicate_policy: CollectionDuplicatePolicy::DeduplicateMedia,
            artwork: CollectionArtwork::default(),
            theme: CollectionTheme::default(),
            provenance: None,
            rule: None,
        }
    }

    fn manual_item(title: &str, position: u32) -> CollectionManualAddItem {
        CollectionManualAddItem {
            media_id: MediaID::Movie(MovieID(Uuid::now_v7())),
            title_override: Some(title.into()),
            position: Some(position),
        }
    }

    #[tokio::test]
    async fn collection_stub_lists_details_pages_and_reorders_items() {
        let service = TestApiService::default();
        service
            .create_collection(create_request("Second Collection"))
            .await
            .expect("seed second collection");

        let first_page = service
            .list_collections(ListCollectionsRequest {
                page: CollectionPagination {
                    cursor: None,
                    limit: 1,
                },
                include_item_counts: true,
                ..ListCollectionsRequest::default()
            })
            .await
            .expect("list collections");
        assert_eq!(first_page.collections.len(), 1);
        assert_eq!(first_page.page.total, 2);
        assert_eq!(first_page.page.next_cursor.as_deref(), Some("1"));

        let collection_id = first_page.collections[0].identity.id;
        let detail = service
            .get_collection_detail(
                collection_id,
                GetCollectionDetailRequest {
                    include_rule: true,
                    include_items_preview: true,
                    include_shelf_placements: false,
                },
            )
            .await
            .expect("collection detail");
        assert_eq!(detail.collection.items_preview.len(), 2);

        let first_items_page = service
            .list_collection_items(
                collection_id,
                ListCollectionItemsRequest {
                    page: CollectionPagination {
                        cursor: None,
                        limit: 1,
                    },
                    availability: Some(
                        CollectionMemberAvailabilityStatus::Available,
                    ),
                },
            )
            .await
            .expect("list collection items");
        assert_eq!(first_items_page.items.len(), 1);
        assert_eq!(first_items_page.page.total, 2);

        let all_items = service
            .list_collection_items(
                collection_id,
                ListCollectionItemsRequest::default(),
            )
            .await
            .expect("all items")
            .items;
        let last_key = all_items[1].item_key.clone();
        let first_key = all_items[0].item_key.clone();
        service
            .manual_reorder_collection_items(
                collection_id,
                ManualReorderCollectionItemsRequest {
                    ordering: vec![
                        CollectionManualOrder {
                            item_key: last_key.clone(),
                            position: 1,
                        },
                        CollectionManualOrder {
                            item_key: first_key,
                            position: 2,
                        },
                    ],
                    expected_revision: Some(1),
                },
            )
            .await
            .expect("reorder items");

        let reordered = service
            .list_collection_items(
                collection_id,
                ListCollectionItemsRequest::default(),
            )
            .await
            .expect("reordered items")
            .items;
        assert_eq!(reordered[0].item_key, last_key);
    }

    #[tokio::test]
    async fn collection_stub_reports_version_and_duplicate_conflicts() {
        let service = TestApiService::default();
        let mut request = create_request("No Duplicates");
        request.duplicate_policy = CollectionDuplicatePolicy::RejectDuplicates;
        let collection = service
            .create_collection(request)
            .await
            .expect("create collection")
            .collection;
        let collection_id = collection.summary.identity.id;
        let item = manual_item("Arrival", 1);
        let media_id = item.media_id;

        service
            .manual_add_collection_items(
                collection_id,
                ManualAddCollectionItemsRequest {
                    items: vec![item],
                    duplicate_policy: None,
                    expected_revision: Some(1),
                },
            )
            .await
            .expect("add item");

        let duplicate = service
            .manual_add_collection_items(
                collection_id,
                ManualAddCollectionItemsRequest {
                    items: vec![CollectionManualAddItem {
                        media_id,
                        title_override: Some("Arrival duplicate".into()),
                        position: Some(2),
                    }],
                    duplicate_policy: None,
                    expected_revision: Some(2),
                },
            )
            .await
            .expect_err("duplicate item should fail closed");
        assert!(duplicate.to_string().contains("Duplicate collection item"));

        let stale_update = service
            .update_collection(
                collection_id,
                UpdateCollectionRequest {
                    title: Some("Stale title".into()),
                    expected_revision: Some(1),
                    ..UpdateCollectionRequest::default()
                },
            )
            .await
            .expect_err("stale revision should fail");
        assert!(stale_update.to_string().contains("version conflict"));
    }

    #[tokio::test]
    async fn collection_stub_handles_rules_shelves_and_tmdb_refreshes() {
        let service = TestApiService::default();
        let sample_id = service
            .list_collections(ListCollectionsRequest::default())
            .await
            .expect("list default collections")
            .collections[0]
            .identity
            .id;

        let mut request = create_request("Dynamic Picks");
        request.kind = CollectionKind::DynamicRule;
        request.source = CollectionSource::DynamicRule;
        request.rule = Some(DynamicCollectionRule::default());
        let dynamic = service
            .create_collection(request)
            .await
            .expect("create dynamic collection")
            .collection;
        let dynamic_id = dynamic.summary.identity.id;
        let rule = dynamic.rule.clone().expect("dynamic rule");

        let validation = service
            .validate_collection_rule(ValidateCollectionRuleRequest {
                rule: rule.clone(),
            })
            .await
            .expect("validate rule");
        assert!(validation.valid);
        let preview = service
            .preview_collection_rule(PreviewCollectionRuleRequest {
                rule: rule.clone(),
                page: CollectionPagination {
                    cursor: None,
                    limit: 1,
                },
            })
            .await
            .expect("preview rule");
        assert_eq!(preview.items.len(), 1);
        assert_eq!(preview.page.total, 2);

        let refreshed = service
            .refresh_collection_rule(
                dynamic_id,
                RefreshCollectionRuleRequest {
                    force: true,
                    expected_rule_hash: validation.rule_hash,
                },
            )
            .await
            .expect("refresh rule");
        assert_eq!(
            refreshed.materialization.state,
            CollectionMaterializationState::Ready
        );

        let first = service
            .pin_shelf_placement(PinShelfPlacementRequest {
                collection_id: sample_id,
                surface: ShelfSurface::Home,
                shelf_key: "home.collections".into(),
                pinned: true,
                position: Some(1),
                presentation: None,
            })
            .await
            .expect("pin first")
            .placement;
        let second = service
            .pin_shelf_placement(PinShelfPlacementRequest {
                collection_id: dynamic_id,
                surface: ShelfSurface::Home,
                shelf_key: "home.collections".into(),
                pinned: true,
                position: Some(2),
                presentation: Some(CollectionPresentationMode::Hero),
            })
            .await
            .expect("pin second")
            .placement;
        service
            .reorder_shelf_placements(ReorderShelfPlacementsRequest {
                ordering: vec![
                    ShelfPlacementOrder {
                        placement_id: second.id,
                        position: 1,
                    },
                    ShelfPlacementOrder {
                        placement_id: first.id,
                        position: 2,
                    },
                ],
            })
            .await
            .expect("reorder shelves");
        let shelves = service
            .list_shelf_placements(ListShelfPlacementsRequest {
                surface: Some(ShelfSurface::Home),
                shelf_key: Some("home.collections".into()),
                include_unpinned: false,
            })
            .await
            .expect("list shelves");
        assert_eq!(shelves.placements[0].id, second.id);

        let tmdb_page = service
            .list_tmdb_collections(TmdbListCollectionsRequest::default())
            .await
            .expect("list tmdb collections");
        assert_eq!(tmdb_page.collections.len(), 2);

        let import_request = TmdbImportCollectionRequest {
            tmdb_id: "550".into(),
            import_kind: TmdbCollectionImportKind::List,
            title_override: None,
            owner: CollectionOwner::default(),
            visibility: CollectionVisibility::Shared,
            presentation: CollectionPresentationMode::Shelf,
            duplicate_policy: CollectionDuplicatePolicy::DeduplicateMedia,
            media_scope: CollectionMediaScope::All,
            refresh_existing: false,
        };
        let imported = service
            .import_tmdb_collection(import_request.clone())
            .await
            .expect("import tmdb collection");
        assert_eq!(imported.collection.summary.source, CollectionSource::Tmdb);

        let duplicate = service
            .import_tmdb_collection(import_request.clone())
            .await
            .expect_err("duplicate tmdb import should fail");
        assert!(duplicate.to_string().contains("Duplicate TMDB collection"));

        let refreshed = service
            .refresh_tmdb_collection(import_request)
            .await
            .expect("refresh tmdb import");
        assert_eq!(
            refreshed
                .collection
                .summary
                .provenance
                .external_id
                .as_deref(),
            Some("550")
        );
    }
}
