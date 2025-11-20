use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use chrono::{Duration, Utc};
use ferrex_core::auth::device::AuthDeviceStatus;
use ferrex_core::identity::auth::domain::value_objects::SessionScope;
use ferrex_core::player_prelude::{
    ActiveScansResponse, AuthToken, AuthenticatedDevice, ConfirmClaimResponse,
    CreateLibraryRequest, FilterIndicesRequest, LatestProgressResponse, Library, LibraryID,
    LibraryType, Media, MediaQuery, MediaWithStatus, Platform, Role, ScanCommandAcceptedResponse,
    ScanCommandRequest, ScanConfig, ScanMetrics, StartClaimResponse, StartScanRequest,
    UpdateLibraryRequest, UpdateProgressRequest, User, UserPermissions, UserPreferences,
    UserWatchState, generate_trust_token,
};
use rkyv::util::AlignedVec;
use uuid::Uuid;

#[cfg(feature = "demo")]
use crate::infrastructure::api_types::{DemoResetRequest, DemoStatus};
use crate::infrastructure::repository::{RepositoryError, RepositoryResult};
use crate::infrastructure::services::api::ApiService;

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
    auth_token: Option<AuthToken>,
    devices: Vec<AuthenticatedDevice>,
    last_claim: Option<StartClaimResponse>,
    current_user: Option<User>,
    current_permissions: Option<UserPermissions>,
}

impl Default for TestApiService {
    fn default() -> Self {
        Self::new("http://localhost:3000")
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
                auth_token: None,
                devices,
                last_claim: None,
                current_user: Some(sample_user),
                current_permissions: Some(sample_permissions),
            })),
            base_url: Arc::from(base_url_string),
        }
    }

    pub fn set_setup_required(&self, value: bool) {
        if let Ok(mut guard) = self.inner.write() {
            guard.setup_required = value;
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

    async fn fetch_libraries(&self) -> RepositoryResult<Vec<Library>> {
        Ok(self.inner.read().expect("lock poisoned").libraries.clone())
    }

    async fn fetch_library_media(&self, library_id: Uuid) -> RepositoryResult<Vec<Media>> {
        let guard = self.inner.read().expect("lock poisoned");
        Ok(guard
            .library_media
            .get(&library_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn create_library(&self, request: CreateLibraryRequest) -> RepositoryResult<LibraryID> {
        let CreateLibraryRequest {
            name,
            library_type,
            paths,
            scan_interval_minutes,
            enabled,
            start_scan,
        } = request;

        let mut guard = self.inner.write().expect("lock poisoned");
        let library = Library {
            id: LibraryID::new(),
            name,
            library_type,
            paths: paths.into_iter().map(PathBuf::from).collect(),
            scan_interval_minutes,
            last_scan: None,
            enabled,
            auto_scan: start_scan,
            watch_for_changes: false,
            analyze_on_scan: false,
            max_retry_attempts: 3,
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
        id: LibraryID,
        request: UpdateLibraryRequest,
    ) -> RepositoryResult<()> {
        let mut guard = self.inner.write().expect("lock poisoned");
        if let Some(library) = guard.libraries.iter_mut().find(|lib| lib.id == id) {
            if let Some(name) = request.name {
                library.name = name;
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

    async fn delete_library(&self, id: LibraryID) -> RepositoryResult<()> {
        let mut guard = self.inner.write().expect("lock poisoned");
        guard.libraries.retain(|library| library.id != id);
        Ok(())
    }

    async fn start_library_scan(
        &self,
        _library_id: LibraryID,
        _request: StartScanRequest,
    ) -> RepositoryResult<ScanCommandAcceptedResponse> {
        Ok(ScanCommandAcceptedResponse {
            scan_id: Uuid::now_v7(),
            correlation_id: Uuid::now_v7(),
        })
    }

    async fn pause_library_scan(
        &self,
        _library_id: LibraryID,
        _request: ScanCommandRequest,
    ) -> RepositoryResult<ScanCommandAcceptedResponse> {
        Ok(ScanCommandAcceptedResponse {
            scan_id: Uuid::now_v7(),
            correlation_id: Uuid::now_v7(),
        })
    }

    async fn resume_library_scan(
        &self,
        _library_id: LibraryID,
        _request: ScanCommandRequest,
    ) -> RepositoryResult<ScanCommandAcceptedResponse> {
        Ok(ScanCommandAcceptedResponse {
            scan_id: Uuid::now_v7(),
            correlation_id: Uuid::now_v7(),
        })
    }

    async fn cancel_library_scan(
        &self,
        _library_id: LibraryID,
        _request: ScanCommandRequest,
    ) -> RepositoryResult<ScanCommandAcceptedResponse> {
        Ok(ScanCommandAcceptedResponse {
            scan_id: Uuid::now_v7(),
            correlation_id: Uuid::now_v7(),
        })
    }

    async fn fetch_active_scans(&self) -> RepositoryResult<ActiveScansResponse> {
        Ok(ActiveScansResponse {
            scans: Vec::new(),
            count: 0,
        })
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
    async fn reset_demo(&self, _request: DemoResetRequest) -> RepositoryResult<DemoStatus> {
        Err(RepositoryError::UpdateFailed(
            "Demo reset not available in test stub".into(),
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

    async fn update_progress(&self, request: &UpdateProgressRequest) -> RepositoryResult<()> {
        if let Ok(mut guard) = self.inner.write() {
            guard
                .watch_state
                .update_progress(request.media_id, request.position, request.duration);
        }
        Ok(())
    }

    async fn list_user_devices(&self) -> RepositoryResult<Vec<AuthenticatedDevice>> {
        Ok(self.inner.read().expect("lock poisoned").devices.clone())
    }

    async fn revoke_device(&self, device_id: Uuid) -> RepositoryResult<()> {
        if let Ok(mut guard) = self.inner.write() {
            guard.devices.retain(|device| device.id != device_id);
        }
        Ok(())
    }

    async fn query_media(&self, _query: MediaQuery) -> RepositoryResult<Vec<MediaWithStatus>> {
        Ok(Vec::new())
    }

    async fn fetch_filtered_indices(
        &self,
        _library_id: Uuid,
        _spec: &FilterIndicesRequest,
    ) -> RepositoryResult<Vec<u32>> {
        Ok(Vec::new())
    }

    async fn check_setup_status(
        &self,
    ) -> RepositoryResult<crate::infrastructure::api_client::SetupStatus> {
        let guard = self.inner.read().expect("lock poisoned");
        Ok(crate::infrastructure::api_client::SetupStatus {
            needs_setup: guard.setup_required,
            has_admin: !guard.setup_required,
            user_count: guard.current_user.iter().count(),
            library_count: guard.libraries.len(),
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
            refresh_token: generate_trust_token(),
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
                RepositoryError::QueryFailed("No current user available in TestApiService".into())
            })
    }

    async fn fetch_my_permissions(&self) -> RepositoryResult<UserPermissions> {
        self.inner
            .read()
            .expect("lock poisoned")
            .current_permissions
            .clone()
            .ok_or_else(|| {
                RepositoryError::QueryFailed("No permissions available in TestApiService".into())
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
}

fn sample_library(name: &str) -> Library {
    Library {
        id: LibraryID::new(),
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
        pin_hash: None,
        pin_set_at: None,
        pin_last_used_at: None,
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
        permissions: HashMap::from([("system:admin".into(), true), ("user:create".into(), true)]),
        permission_details: None,
    }
}
