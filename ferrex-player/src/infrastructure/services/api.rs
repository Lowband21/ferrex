//! API service trait and implementations
//!
//! Provides abstraction over HTTP API operations,
//! replacing direct ApiClient access per RUS-136.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use ferrex_core::library::Library;
use ferrex_core::media::MediaReference;
use ferrex_core::watch_status::{UserWatchState, UpdateProgressRequest};
use ferrex_core::auth::device::AuthenticatedDevice;
use ferrex_core::user::AuthToken;
use crate::infrastructure::repositories::RepositoryResult;

/// Generic API service trait for server communication
#[async_trait]
pub trait ApiService: Send + Sync {
    /// Make a GET request to the API
    async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str) -> RepositoryResult<T>;
    
    /// Make a POST request to the API
    async fn post<T: for<'de> Deserialize<'de>, B: Serialize + Send + Sync>(&self, path: &str, body: &B) -> RepositoryResult<T>;
    
    /// Make a PUT request to the API
    async fn put<T: for<'de> Deserialize<'de>, B: Serialize + Send + Sync>(&self, path: &str, body: &B) -> RepositoryResult<T>;
    
    /// Make a DELETE request to the API
    async fn delete<T: for<'de> Deserialize<'de>>(&self, path: &str) -> RepositoryResult<T>;
    
    // === Common API operations ===
    
    /// Fetch all libraries from the server
    async fn fetch_libraries(&self) -> RepositoryResult<Vec<Library>>;
    
    /// Fetch media for a specific library
    async fn fetch_library_media(&self, library_id: Uuid) -> RepositoryResult<Vec<MediaReference>>;
    
    /// Start a library scan
    async fn scan_library(&self, library_id: Uuid) -> RepositoryResult<()>;
    
    /// Check server health
    async fn health_check(&self) -> RepositoryResult<bool>;
    
    // === Additional API operations ===
    
    /// Get watch status for all media
    async fn get_watch_state(&self) -> RepositoryResult<UserWatchState>;
    
    /// Update progress for a media item
    async fn update_progress(&self, request: &UpdateProgressRequest) -> RepositoryResult<()>;
    
    /// List authenticated devices for current user
    async fn list_user_devices(&self) -> RepositoryResult<Vec<AuthenticatedDevice>>;
    
    /// Revoke a device
    async fn revoke_device(&self, device_id: Uuid) -> RepositoryResult<()>;
    
    /// Query media with complex filters
    async fn query_media(&self, query: ferrex_core::query::MediaQuery) -> RepositoryResult<Vec<ferrex_core::query::MediaReferenceWithStatus>>;
    
    /// Check if setup is required
    async fn check_setup_status(&self) -> RepositoryResult<crate::infrastructure::api_client::SetupStatus>;
    
    /// Create initial admin user during setup
    async fn create_initial_admin(
        &self,
        username: String,
        password: String,
        pin: Option<String>,
    ) -> RepositoryResult<(ferrex_core::user::User, AuthToken)>;
    
    /// Make a GET request to a public endpoint (no auth)
    async fn get_public<T: for<'de> Deserialize<'de>>(&self, path: &str) -> RepositoryResult<T>;
    
    /// Build a full URL from a path
    fn build_url(&self, path: &str) -> String;
    
    /// Get the base URL
    fn base_url(&self) -> &str;
    
    /// Set the authentication token
    async fn set_token(&self, token: Option<AuthToken>);
    
    /// Get the current authentication token
    async fn get_token(&self) -> Option<AuthToken>;
}

/// Mock implementation for testing
#[cfg(test)]
pub mod mock {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use std::collections::HashMap;
    
    pub struct MockApiService {
        pub get_calls: Arc<RwLock<Vec<String>>>,
        pub post_calls: Arc<RwLock<Vec<String>>>,
        pub libraries: Arc<RwLock<Vec<Library>>>,
        pub media: Arc<RwLock<HashMap<Uuid, Vec<MediaReference>>>>,
    }
    
    impl MockApiService {
        pub fn new() -> Self {
            Self {
                get_calls: Arc::new(RwLock::new(Vec::new())),
                post_calls: Arc::new(RwLock::new(Vec::new())),
                libraries: Arc::new(RwLock::new(Vec::new())),
                media: Arc::new(RwLock::new(HashMap::new())),
            }
        }
        
        pub async fn add_test_library(&self, library: Library) {
            self.libraries.write().await.push(library);
        }
        
        pub async fn add_test_media(&self, library_id: Uuid, media: Vec<MediaReference>) {
            self.media.write().await.insert(library_id, media);
        }
    }
    
    #[async_trait]
    impl ApiService for MockApiService {
        async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str) -> RepositoryResult<T> {
            self.get_calls.write().await.push(path.to_string());
            
            // Mock implementation - return default value
            // In real tests, you'd configure expected responses
            Err(crate::infrastructure::repositories::RepositoryError::QueryFailed(
                "Mock not configured for this path".to_string()
            ))
        }
        
        async fn post<T: for<'de> Deserialize<'de>, B: Serialize + Send + Sync>(&self, path: &str, _body: &B) -> RepositoryResult<T> {
            self.post_calls.write().await.push(path.to_string());
            
            Err(crate::infrastructure::repositories::RepositoryError::QueryFailed(
                "Mock not configured for this path".to_string()
            ))
        }
        
        async fn put<T: for<'de> Deserialize<'de>, B: Serialize + Send + Sync>(&self, path: &str, _body: &B) -> RepositoryResult<T> {
            Err(crate::infrastructure::repositories::RepositoryError::UpdateFailed(
                format!("Mock PUT not implemented for {}", path)
            ))
        }
        
        async fn delete<T: for<'de> Deserialize<'de>>(&self, path: &str) -> RepositoryResult<T> {
            Err(crate::infrastructure::repositories::RepositoryError::DeleteFailed(
                format!("Mock DELETE not implemented for {}", path)
            ))
        }
        
        async fn fetch_libraries(&self) -> RepositoryResult<Vec<Library>> {
            Ok(self.libraries.read().await.clone())
        }
        
        async fn fetch_library_media(&self, library_id: Uuid) -> RepositoryResult<Vec<MediaReference>> {
            Ok(self.media.read().await
                .get(&library_id)
                .cloned()
                .unwrap_or_default())
        }
        
        async fn scan_library(&self, _library_id: Uuid) -> RepositoryResult<()> {
            // Mock implementation - just return success
            Ok(())
        }
        
        async fn health_check(&self) -> RepositoryResult<bool> {
            Ok(true)
        }
        
        async fn get_watch_state(&self) -> RepositoryResult<UserWatchState> {
            Ok(UserWatchState::new())
        }
        
        async fn update_progress(&self, _request: &UpdateProgressRequest) -> RepositoryResult<()> {
            Ok(())
        }
        
        async fn list_user_devices(&self) -> RepositoryResult<Vec<AuthenticatedDevice>> {
            Ok(Vec::new())
        }
        
        async fn revoke_device(&self, _device_id: Uuid) -> RepositoryResult<()> {
            Ok(())
        }
        
        async fn query_media(&self, _query: ferrex_core::query::MediaQuery) -> RepositoryResult<Vec<ferrex_core::query::MediaReferenceWithStatus>> {
            Ok(Vec::new())
        }
        
        async fn check_setup_status(&self) -> RepositoryResult<crate::infrastructure::api_client::SetupStatus> {
            Ok(crate::infrastructure::api_client::SetupStatus {
                needs_setup: false,
                has_admin: true,
                user_count: 1,
                library_count: 0,
            })
        }
        
        async fn create_initial_admin(
            &self,
            _username: String,
            _password: String,
            _pin: Option<String>,
        ) -> RepositoryResult<(ferrex_core::user::User, AuthToken)> {
            Err(crate::infrastructure::repositories::RepositoryError::QueryFailed(
                "Mock create admin not implemented".to_string()
            ))
        }
        
        async fn get_public<T: for<'de> Deserialize<'de>>(&self, _path: &str) -> RepositoryResult<T> {
            Err(crate::infrastructure::repositories::RepositoryError::QueryFailed(
                "Mock public get not implemented".to_string()
            ))
        }
        
        fn build_url(&self, path: &str) -> String {
            format!("http://mock/{}", path)
        }
        
        fn base_url(&self) -> &str {
            "http://mock"
        }
        
        async fn set_token(&self, _token: Option<AuthToken>) {
            // Mock implementation - no-op
        }
        
        async fn get_token(&self) -> Option<AuthToken> {
            None
        }
    }
}