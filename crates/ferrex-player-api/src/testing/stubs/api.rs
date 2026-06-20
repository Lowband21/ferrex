use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use chrono::{Duration, Utc};
use ferrex_core::domain::users::auth::{
    device::AuthDeviceStatus, domain::value_objects::SessionScope,
};
use ferrex_core::{
    api::scan::{IncrementalScanStatusView, ScanQueueDepths},
    player_prelude::{
        ActiveScansResponse, AuthToken, AuthenticatedDevice,
        ConfirmClaimResponse, CreateLibraryRequest, FilterIndicesRequest,
        ImageManifestRequest, ImageManifestResponse, LatestProgressResponse,
        Library, LibraryId, LibraryType, Media, MediaQuery,
        MediaRootBrowseResponse, MediaWithStatus, MovieBatchFetchRequest,
        MovieBatchId, MovieBatchSyncRequest, MovieBatchSyncResponse, Platform,
        Role, ScanCommandAcceptedResponse, ScanCommandRequest, ScanConfig,
        ScanFailureDto, ScanLifecycleStatus, ScanMetrics, ScanPageMeta,
        ScanRecoveryRequest, ScanRecoveryResponse, ScanRunDetailResponse,
        ScanRunDto, ScanRunEventDto, ScanRunEventsPageResponse,
        ScanRunFailuresPageResponse, ScanRunListResponse, ScanRunMode,
        ScanStartDisposition, ScannerHealthResponse, SeriesBundleFetchRequest,
        SeriesBundleSyncRequest, SeriesBundleSyncResponse, SeriesID,
        StartClaimResponse, StartScanRequest, UpdateLibraryRequest,
        UpdateProgressRequest, User, UserPermissions, UserPreferences,
        UserWatchState,
    },
};
use ferrex_model::MovieReferenceBatchSize;
use ferrex_model::image::ImageQuery;
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
    watch_state: UserWatchState,
    setup_required: bool,
    setup_token_required: bool,
    auth_token: Option<AuthToken>,
    devices: Vec<AuthenticatedDevice>,
    last_claim: Option<StartClaimResponse>,
    current_user: Option<User>,
    current_permissions: Option<UserPermissions>,
    playback_ticket_result: Option<Result<String, String>>,
    scan_health: ScannerHealthResponse,
    scan_runs: Vec<ScanRunDto>,
    scan_run_details: HashMap<Uuid, ScanRunDetailResponse>,
    scan_run_events: HashMap<Uuid, Vec<ScanRunEventDto>>,
    scan_run_failures: HashMap<Uuid, Vec<ScanFailureDto>>,
    scan_recovery_requests: Vec<ScanRecoveryRequest>,
}

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

        let sample_user = sample_user("demo_admin");
        let sample_permissions = sample_permissions(sample_user.id);

        Self {
            inner: Arc::new(RwLock::new(InnerApiState {
                libraries: vec![library],
                library_media: HashMap::new(),
                watch_state: UserWatchState::new(),
                setup_required: true,
                setup_token_required: false,
                auth_token: None,
                devices,
                last_claim: None,
                current_user: Some(sample_user),
                current_permissions: Some(sample_permissions),
                playback_ticket_result: None,
                scan_health: empty_scan_health(),
                scan_runs: Vec::new(),
                scan_run_details: HashMap::new(),
                scan_run_events: HashMap::new(),
                scan_run_failures: HashMap::new(),
                scan_recovery_requests: Vec::new(),
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

    pub fn set_scan_health(&self, health: ScannerHealthResponse) {
        if let Ok(mut guard) = self.inner.write() {
            guard.scan_health = health;
        }
    }

    pub fn set_scan_runs(&self, runs: Vec<ScanRunDto>) {
        if let Ok(mut guard) = self.inner.write() {
            guard.scan_runs = runs;
        }
    }

    pub fn insert_scan_run_detail(&self, detail: ScanRunDetailResponse) {
        if let Ok(mut guard) = self.inner.write() {
            guard.scan_run_details.insert(detail.run.scan_id, detail);
        }
    }

    pub fn insert_scan_run_events(
        &self,
        scan_id: Uuid,
        events: Vec<ScanRunEventDto>,
    ) {
        if let Ok(mut guard) = self.inner.write() {
            guard.scan_run_events.insert(scan_id, events);
        }
    }

    pub fn insert_scan_run_failures(
        &self,
        scan_id: Uuid,
        failures: Vec<ScanFailureDto>,
    ) {
        if let Ok(mut guard) = self.inner.write() {
            guard.scan_run_failures.insert(scan_id, failures);
        }
    }

    pub fn scan_recovery_requests(&self) -> Vec<ScanRecoveryRequest> {
        self.inner
            .read()
            .expect("lock poisoned")
            .scan_recovery_requests
            .clone()
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

    async fn fetch_scanner_health(
        &self,
    ) -> RepositoryResult<ScannerHealthResponse> {
        Ok(self
            .inner
            .read()
            .expect("lock poisoned")
            .scan_health
            .clone())
    }

    async fn fetch_scan_runs(
        &self,
        library_id: Option<LibraryId>,
        status: Option<String>,
        limit: usize,
        offset: usize,
    ) -> RepositoryResult<ScanRunListResponse> {
        let guard = self.inner.read().expect("lock poisoned");
        let mut runs = guard.scan_runs.clone();
        if let Some(library_id) = library_id {
            runs.retain(|run| run.library_id == library_id);
        }
        if let Some(status) = status {
            runs.retain(|run| run.status == status);
        }
        runs.sort_by(|a, b| b.last_event_at.cmp(&a.last_event_at));
        let total = runs.len();
        let paged = runs
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect::<Vec<_>>();
        Ok(ScanRunListResponse {
            page: ScanPageMeta::new(limit, offset, paged.len(), total),
            runs: paged,
        })
    }

    async fn fetch_scan_run_detail(
        &self,
        scan_id: Uuid,
    ) -> RepositoryResult<ScanRunDetailResponse> {
        let guard = self.inner.read().expect("lock poisoned");
        if let Some(detail) = guard.scan_run_details.get(&scan_id) {
            return Ok(detail.clone());
        }
        if let Some(run) =
            guard.scan_runs.iter().find(|run| run.scan_id == scan_id)
        {
            return Ok(ScanRunDetailResponse {
                run: run.clone(),
                terminal_summary: serde_json::Value::Null,
            });
        }
        Err(RepositoryError::NotFound {
            entity_type: "ScanRun".into(),
            id: scan_id.to_string(),
        })
    }

    async fn fetch_scan_run_events(
        &self,
        scan_id: Uuid,
        after_sequence: Option<u64>,
        limit: usize,
    ) -> RepositoryResult<ScanRunEventsPageResponse> {
        let guard = self.inner.read().expect("lock poisoned");
        let mut events = guard
            .scan_run_events
            .get(&scan_id)
            .cloned()
            .unwrap_or_default();
        if let Some(after_sequence) = after_sequence {
            events.retain(|event| event.sequence > after_sequence);
        }
        events.sort_by(|a, b| a.sequence.cmp(&b.sequence));
        let total = events.len();
        let paged = events.into_iter().take(limit).collect::<Vec<_>>();
        Ok(ScanRunEventsPageResponse {
            scan_id,
            events: paged.clone(),
            page: ScanPageMeta::new(limit, 0, paged.len(), total),
            replay: None,
        })
    }

    async fn fetch_scan_run_failures(
        &self,
        scan_id: Uuid,
        limit: usize,
        offset: usize,
        include_debug: bool,
    ) -> RepositoryResult<ScanRunFailuresPageResponse> {
        let guard = self.inner.read().expect("lock poisoned");
        let mut failures = guard
            .scan_run_failures
            .get(&scan_id)
            .cloned()
            .unwrap_or_default();
        if !include_debug {
            for failure in failures.iter_mut() {
                failure.debug = None;
            }
        }
        failures.sort_by(|a, b| b.last_seen_at.cmp(&a.last_seen_at));
        let total = failures.len();
        let paged = failures
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect::<Vec<_>>();
        Ok(ScanRunFailuresPageResponse {
            scan_id,
            failures: paged.clone(),
            page: ScanPageMeta::new(limit, offset, paged.len(), total),
        })
    }

    async fn recover_scan_path(
        &self,
        request: ScanRecoveryRequest,
    ) -> RepositoryResult<ScanRecoveryResponse> {
        if let Ok(mut guard) = self.inner.write() {
            guard.scan_recovery_requests.push(request.clone());
        }
        Ok(ScanRecoveryResponse {
            library_id: request.library_id,
            path: request.path.clone(),
            normalized_path: request.path,
            job_id: Uuid::now_v7(),
            accepted: true,
            merged_into: None,
            idempotency_key: request
                .correlation_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| Uuid::now_v7().to_string()),
            message: "Scan recovery queued.".into(),
        })
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

fn empty_scan_health() -> ScannerHealthResponse {
    ScannerHealthResponse {
        queue_depths: ScanQueueDepths {
            folder_scan: 0,
            manifest_scan: 0,
            analyze: 0,
            metadata: 0,
            index: 0,
            image_fetch: 0,
        },
        active_scans: 0,
        retained_runs: 0,
        failed_runs: 0,
        incremental: IncrementalScanStatusView::default(),
    }
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
