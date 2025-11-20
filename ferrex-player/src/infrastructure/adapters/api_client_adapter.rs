//! ApiClient adapter that implements ApiService trait
//!
//! Wraps the existing ApiClient to provide a trait-based interface

use async_trait::async_trait;
use rkyv::rancor::Error;
use rkyv::util::AlignedVec;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::infrastructure::ApiClient;
use crate::infrastructure::api_client::SetupStatus;
#[cfg(feature = "demo")]
use crate::infrastructure::api_types::{DemoResetRequest, DemoStatus};
use crate::infrastructure::repository::{RepositoryError, RepositoryResult};
use crate::infrastructure::services::api::ApiService;
use ferrex_core::api_routes::{
    utils::replace_param,
    v1::{self, admin::MEDIA_ROOT_BROWSER},
};
use ferrex_core::api_types::setup::{ConfirmClaimResponse, StartClaimResponse};
use ferrex_core::player_prelude::{
    ActiveScansResponse, AuthToken, AuthenticatedDevice, CreateLibraryRequest,
    FilterIndicesRequest, IndicesResponse, LatestProgressResponse, Library,
    LibraryID, LibraryMediaResponse, Media, MediaID, MediaQuery,
    MediaRootBrowseResponse, MediaWithStatus, ScanCommandAcceptedResponse,
    ScanCommandRequest, ScanConfig, ScanMetrics, SortBy, SortOrder,
    StartScanRequest, UpdateLibraryRequest, UpdateProgressRequest, User,
    UserPermissions, UserWatchState,
};
use ferrex_core::player_prelude::{MediaIDLike, hash_filter_spec};
use parking_lot::RwLock;

const FILTER_INDICES_CACHE_TTL: Duration = Duration::from_secs(30);

/// Adapter that implements ApiService using the existing ApiClient
#[derive(Debug, Clone)]
pub struct ApiClientAdapter {
    client: Arc<ApiClient>,
    filter_indices_cache:
        Arc<RwLock<HashMap<PlayerFilterCacheKey, CachedPositions>>>,
}

impl ApiClientAdapter {
    pub fn new(client: Arc<ApiClient>) -> Self {
        Self {
            client,
            filter_indices_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Fetch all presorted IDs for a library by paging through /libraries/{id}/sorted-ids
    pub async fn fetch_sorted_ids(
        &self,
        library_id: Uuid,
        sort: &str,
        order: &str,
    ) -> RepositoryResult<Vec<Uuid>> {
        #[derive(Debug, Deserialize)]
        struct SortedIdsResponse {
            total: usize,
            offset: usize,
            limit: usize,
            ids: Vec<MediaID>,
        }

        let mut all_ids: Vec<Uuid> = Vec::new();
        let mut offset: usize = 0;
        let page_size: usize = 500;
        let base = replace_param(
            v1::libraries::SORTED_IDS,
            "{id}",
            library_id.to_string(),
        );

        loop {
            let path = format!(
                "{}?sort={}&order={}&offset={}&limit={}",
                base, sort, order, offset, page_size
            );

            let result: Result<SortedIdsResponse, _> = self
                .client
                .get(&path)
                .await
                .map_err(|e| RepositoryError::QueryFailed(e.to_string()));

            let resp = match result {
                Ok(r) => r,
                Err(e) => return Err(e),
            };

            // Map MediaID to raw UUIDs (movies-only for now)
            for mid in resp.ids {
                match mid {
                    MediaID::Movie(m) => all_ids.push(m.to_uuid()),
                    // Ignore non-movie entries for now
                    _ => {}
                }
            }

            offset = resp.offset + resp.limit;
            if all_ids.len() >= resp.total || resp.limit == 0 {
                break;
            }
        }

        Ok(all_ids)
    }
}

impl ApiClientAdapter {
    // Public wrappers to call from UI code
    pub async fn fetch_sorted_indices(
        &self,
        library_id: Uuid,
        sort: SortBy,
        order: SortOrder,
    ) -> RepositoryResult<Vec<u32>> {
        let path = replace_param(
            v1::libraries::SORTED_INDICES,
            "{id}",
            library_id.to_string(),
        );
        // Pass sort/order as query string (snake_case for sort field)
        let sort_str = match sort {
            SortBy::Title => "title",
            SortBy::DateAdded => "date_added",
            SortBy::CreatedAt => "created_at",
            SortBy::ReleaseDate => "release_date",
            SortBy::LastWatched => "last_watched",
            SortBy::WatchProgress => "watch_progress",
            SortBy::Rating => "rating",
            SortBy::Runtime => "runtime",
            SortBy::Popularity => "popularity",
            SortBy::Bitrate => "bitrate",
            SortBy::FileSize => "file_size",
            SortBy::ContentRating => "content_rating",
            SortBy::Resolution => "resolution",
        };
        let order_str = match order {
            SortOrder::Ascending => "asc",
            SortOrder::Descending => "desc",
        };
        let url = format!("{}?sort={}&order={}", path, sort_str, order_str);
        let aligned = self
            .client
            .get_rkyv(&url, None)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))?;
        let decoded: IndicesResponse =
            rkyv::from_bytes::<IndicesResponse, Error>(&aligned).map_err(
                |e| {
                    RepositoryError::QueryFailed(format!(
                        "rkyv decode: {:?}",
                        e
                    ))
                },
            )?;
        Ok(decoded.indices)
    }

    pub async fn fetch_filtered_indices(
        &self,
        library_id: Uuid,
        spec: &FilterIndicesRequest,
    ) -> RepositoryResult<Vec<u32>> {
        let cache_key = PlayerFilterCacheKey {
            library_id,
            spec_hash: hash_filter_spec(spec),
        };

        if let Some(indices) = self.lookup_cached_indices(&cache_key) {
            return Ok(indices);
        }

        let path = replace_param(
            v1::libraries::FILTERED_INDICES,
            "{id}",
            library_id.to_string(),
        );
        let url = self.client.build_url(&path);
        let req = self.client.client.post(&url).json(spec);
        let req = self.client.build_request(req).await;
        let bytes = self
            .client
            .execute_rkyv_request(req)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))?;
        let decoded: IndicesResponse =
            rkyv::from_bytes::<IndicesResponse, Error>(&bytes).map_err(
                |e| {
                    RepositoryError::QueryFailed(format!(
                        "rkyv decode: {:?}",
                        e
                    ))
                },
            )?;
        self.store_cached_indices(cache_key, decoded.indices.clone());
        Ok(decoded.indices)
    }
}

#[async_trait]
impl ApiService for ApiClientAdapter {
    async fn get_rkyv(
        &self,
        path: &str,
        query: Option<(&str, &str)>,
    ) -> RepositoryResult<AlignedVec> {
        let bytes = self.client.get_rkyv(path, query).await;
        bytes.map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    async fn get_bytes(
        &self,
        path: &str,
        query: Option<(&str, &str)>,
    ) -> RepositoryResult<Vec<u8>> {
        self.client
            .get_bytes(path, query)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    async fn fetch_libraries(&self) -> RepositoryResult<Vec<Library>> {
        self.client
            .get(v1::libraries::COLLECTION)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    async fn fetch_library_media(
        &self,
        library_id: Uuid,
    ) -> RepositoryResult<Vec<Media>> {
        // Build URL for the library media endpoint
        let url = self.client.build_url(replace_param(
            v1::libraries::MEDIA,
            "{id}",
            library_id.to_string(),
        ));
        log::info!("Fetching library media from {}", url);
        let request = self.client.client.get(&url);
        let request = self.client.build_request(request).await;

        // Use the rkyv request method to get binary response
        let bytes = self
            .client
            .execute_rkyv_request(request)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))?;

        // Deserialize from rkyv bytes
        // For now, we deserialize to owned types for simplicity
        // Later we can optimize to work directly with archived data for zero-copy
        let response = rkyv::from_bytes::<
            LibraryMediaResponse,
            rkyv::rancor::Error,
        >(&bytes)
        .map_err(|e| {
            RepositoryError::QueryFailed(format!(
                "Failed to deserialize rkyv data: {:?}",
                e
            ))
        })?;

        Ok(response.media)
    }

    async fn health_check(&self) -> RepositoryResult<bool> {
        #[derive(Deserialize)]
        struct HealthResponse {
            status: String,
        }

        match self
            .client
            .get::<HealthResponse>("/health")
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
        {
            Ok(response) => {
                Ok(response.status == "ok" || response.status == "healthy")
            }
            Err(_) => Ok(false),
        }
    }

    async fn get_watch_state(&self) -> RepositoryResult<UserWatchState> {
        self.client
            .get_watch_state()
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    async fn update_progress(
        &self,
        request: &UpdateProgressRequest,
    ) -> RepositoryResult<()> {
        self.client
            .update_progress(request)
            .await
            .map_err(|e| RepositoryError::UpdateFailed(e.to_string()))
    }

    async fn list_user_devices(
        &self,
    ) -> RepositoryResult<Vec<AuthenticatedDevice>> {
        self.client
            .list_user_devices()
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    async fn revoke_device(&self, device_id: Uuid) -> RepositoryResult<()> {
        self.client
            .revoke_device(device_id)
            .await
            .map_err(|e| RepositoryError::UpdateFailed(e.to_string()))
    }

    async fn query_media(
        &self,
        query: MediaQuery,
    ) -> RepositoryResult<Vec<MediaWithStatus>> {
        self.client
            .query_media(query)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    async fn fetch_filtered_indices(
        &self,
        library_id: Uuid,
        spec: &FilterIndicesRequest,
    ) -> RepositoryResult<Vec<u32>> {
        ApiClientAdapter::fetch_filtered_indices(self, library_id, spec).await
    }

    async fn check_setup_status(&self) -> RepositoryResult<SetupStatus> {
        // ApiClient's check_setup_status returns bool, but we need SetupStatus
        // Call the endpoint directly to get the full status
        self.client
            .get(v1::setup::STATUS)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    async fn create_initial_admin(
        &self,
        username: String,
        password: String,
        display_name: Option<String>,
        setup_token: Option<String>,
        claim_token: Option<String>,
    ) -> RepositoryResult<(User, AuthToken)> {
        let token = self
            .client
            .create_initial_admin(
                username,
                password,
                display_name,
                setup_token,
                claim_token,
            )
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))?;

        // Ensure subsequent authenticated requests use the fresh token
        self.client.set_token(Some(token.clone())).await;

        // Now get the user info
        let user: User = self
            .client
            .get(v1::users::CURRENT)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))?;

        Ok((user, token))
    }

    async fn fetch_current_user(&self) -> RepositoryResult<User> {
        self.client
            .get(v1::users::CURRENT)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    async fn fetch_my_permissions(&self) -> RepositoryResult<UserPermissions> {
        self.client
            .get(v1::roles::MY_PERMISSIONS)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    async fn start_setup_claim(
        &self,
        device_name: Option<String>,
    ) -> RepositoryResult<StartClaimResponse> {
        self.client
            .start_setup_claim(device_name)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    async fn confirm_setup_claim(
        &self,
        claim_code: String,
    ) -> RepositoryResult<ConfirmClaimResponse> {
        self.client
            .confirm_setup_claim(&claim_code)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    fn build_url(&self, path: &str) -> String {
        self.client.build_url(path)
    }

    fn base_url(&self) -> &str {
        self.client.base_url()
    }

    async fn set_token(&self, token: Option<AuthToken>) {
        self.client.set_token(token).await
    }

    async fn get_token(&self) -> Option<AuthToken> {
        self.client.get_token().await
    }

    async fn create_library(
        &self,
        request: CreateLibraryRequest,
    ) -> RepositoryResult<LibraryID> {
        self.client
            .post(v1::libraries::COLLECTION, &request)
            .await
            .map_err(|e| RepositoryError::CreateFailed(e.to_string()))
    }

    async fn update_library(
        &self,
        id: LibraryID,
        request: UpdateLibraryRequest,
    ) -> RepositoryResult<()> {
        let path = replace_param(
            v1::libraries::ITEM,
            "{id}",
            id.as_uuid().to_string(),
        );
        let _: String = self
            .client
            .put(&path, &request)
            .await
            .map_err(|e| RepositoryError::UpdateFailed(e.to_string()))?;
        Ok(())
    }

    async fn delete_library(&self, id: LibraryID) -> RepositoryResult<()> {
        let path = replace_param(
            v1::libraries::ITEM,
            "{id}",
            id.as_uuid().to_string(),
        );
        let _: String = self
            .client
            .delete(&path)
            .await
            .map_err(|e| RepositoryError::DeleteFailed(e.to_string()))?;
        Ok(())
    }

    async fn start_library_scan(
        &self,
        library_id: LibraryID,
        request: StartScanRequest,
    ) -> RepositoryResult<ScanCommandAcceptedResponse> {
        let path = replace_param(
            v1::libraries::scans::START,
            "{id}",
            library_id.to_string(),
        );
        self.client
            .post(&path, &request)
            .await
            .map_err(|e| RepositoryError::UpdateFailed(e.to_string()))
    }

    async fn pause_library_scan(
        &self,
        library_id: LibraryID,
        request: ScanCommandRequest,
    ) -> RepositoryResult<ScanCommandAcceptedResponse> {
        let path = replace_param(
            v1::libraries::scans::PAUSE,
            "{id}",
            library_id.to_string(),
        );
        self.client
            .post(&path, &request)
            .await
            .map_err(|e| RepositoryError::UpdateFailed(e.to_string()))
    }

    async fn resume_library_scan(
        &self,
        library_id: LibraryID,
        request: ScanCommandRequest,
    ) -> RepositoryResult<ScanCommandAcceptedResponse> {
        let path = replace_param(
            v1::libraries::scans::RESUME,
            "{id}",
            library_id.to_string(),
        );
        self.client
            .post(&path, &request)
            .await
            .map_err(|e| RepositoryError::UpdateFailed(e.to_string()))
    }

    async fn cancel_library_scan(
        &self,
        library_id: LibraryID,
        request: ScanCommandRequest,
    ) -> RepositoryResult<ScanCommandAcceptedResponse> {
        let path = replace_param(
            v1::libraries::scans::CANCEL,
            "{id}",
            library_id.to_string(),
        );
        self.client
            .post(&path, &request)
            .await
            .map_err(|e| RepositoryError::UpdateFailed(e.to_string()))
    }

    async fn fetch_active_scans(
        &self,
    ) -> RepositoryResult<ActiveScansResponse> {
        self.client
            .get(v1::scan::ACTIVE)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    async fn fetch_latest_scan_progress(
        &self,
        scan_id: uuid::Uuid,
    ) -> RepositoryResult<LatestProgressResponse> {
        let path = format!("{}?scan_id={}", v1::scan::PROGRESS, scan_id);
        self.client
            .get(&path)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    async fn fetch_scan_metrics(&self) -> RepositoryResult<ScanMetrics> {
        self.client
            .get(v1::scan::METRICS)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    async fn fetch_scan_config(&self) -> RepositoryResult<ScanConfig> {
        let wrapped: ScanConfig = self
            .client
            .get(v1::scan::CONFIG)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))?;
        Ok(wrapped)
    }

    async fn browse_media_root(
        &self,
        path: Option<&str>,
    ) -> RepositoryResult<MediaRootBrowseResponse> {
        let mut url = MEDIA_ROOT_BROWSER.to_string();
        if let Some(rel) = path {
            if !rel.is_empty() {
                url.push_str("?path=");
                url.push_str(&urlencoding::encode(rel));
            }
        }

        self.client
            .get(&url)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    #[cfg(feature = "demo")]
    async fn fetch_demo_status(&self) -> RepositoryResult<DemoStatus> {
        self.client
            .get(v1::admin::demo::STATUS)
            .await
            .map_err(|e| RepositoryError::QueryFailed(e.to_string()))
    }

    #[cfg(feature = "demo")]
    async fn reset_demo(
        &self,
        request: DemoResetRequest,
    ) -> RepositoryResult<DemoStatus> {
        self.client
            .post(v1::admin::demo::RESET, &request)
            .await
            .map_err(|e| RepositoryError::UpdateFailed(e.to_string()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PlayerFilterCacheKey {
    library_id: Uuid,
    spec_hash: u64,
}

#[derive(Debug, Clone)]
struct CachedPositions {
    indices: Vec<u32>,
    stored_at: Instant,
}

impl ApiClientAdapter {
    fn lookup_cached_indices(
        &self,
        key: &PlayerFilterCacheKey,
    ) -> Option<Vec<u32>> {
        let mut cache = self.filter_indices_cache.write();
        if let Some(entry) = cache.get(key) {
            if entry.stored_at.elapsed() < FILTER_INDICES_CACHE_TTL {
                return Some(entry.indices.clone());
            } else {
                cache.remove(key);
            }
        }
        None
    }

    fn store_cached_indices(
        &self,
        key: PlayerFilterCacheKey,
        indices: Vec<u32>,
    ) {
        self.filter_indices_cache.write().insert(
            key,
            CachedPositions {
                indices,
                stored_at: Instant::now(),
            },
        );
    }
}
