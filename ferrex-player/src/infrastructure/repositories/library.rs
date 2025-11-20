//! Library repository trait and related types
//!
//! Defines the interface for library data access operations.

use async_trait::async_trait;
use std::path::PathBuf;
use uuid::Uuid;
use ferrex_core::library::{Library, LibraryType};
use super::RepositoryResult;

/// Query options for libraries
#[derive(Debug, Clone, Default)]
pub struct LibraryQuery {
    pub library_type: Option<LibraryType>,
    pub user_id: Option<Uuid>,
    pub include_hidden: bool,
}

/// Repository trait for library data access
#[async_trait]
pub trait LibraryRepository: Send + Sync {
    // ===== Read Operations =====
    
    /// Get a library by ID
    async fn get(&self, id: Uuid) -> RepositoryResult<Option<Library>>;
    
    /// Get all libraries
    async fn get_all(&self) -> RepositoryResult<Vec<Library>>;
    
    /// Query libraries with filters
    async fn query(&self, query: &LibraryQuery) -> RepositoryResult<Vec<Library>>;
    
    /// Get libraries by type
    async fn get_by_type(&self, library_type: LibraryType) -> RepositoryResult<Vec<Library>>;
    
    /// Check if a library exists
    async fn exists(&self, id: Uuid) -> RepositoryResult<bool>;
    
    /// Get the default library
    async fn get_default(&self) -> RepositoryResult<Option<Library>>;
    
    // ===== Write Operations =====
    
    /// Create a new library
    async fn create(&self, name: String, library_type: LibraryType, paths: Vec<PathBuf>) -> RepositoryResult<Library>;
    
    /// Update a library
    async fn update(&self, id: Uuid, name: String, paths: Vec<PathBuf>) -> RepositoryResult<Library>;
    
    /// Delete a library
    async fn delete(&self, id: Uuid) -> RepositoryResult<bool>;
    
    /// Set a library as default
    async fn set_default(&self, id: Uuid) -> RepositoryResult<()>;
    
    // ===== Scan Operations =====
    
    /// Trigger a library scan
    async fn trigger_scan(&self, id: Uuid) -> RepositoryResult<()>;
    
    /// Get scan status for a library
    async fn get_scan_status(&self, id: Uuid) -> RepositoryResult<ScanStatus>;
}

/// Library scan status
#[derive(Debug, Clone, PartialEq)]
pub enum ScanStatus {
    Idle,
    Scanning { progress: f32, current_file: Option<String> },
    Completed { files_found: usize, duration_ms: u64 },
    Failed { error: String },
}

/// Mock implementation for testing
#[cfg(test)]
pub mod mock {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    
    pub struct MockLibraryRepository {
        storage: Arc<RwLock<HashMap<Uuid, Library>>>,
        default_id: Arc<RwLock<Option<Uuid>>>,
        pub create_called: Arc<RwLock<Vec<(String, LibraryType, Vec<PathBuf>)>>>,
        pub scan_triggered: Arc<RwLock<Vec<Uuid>>>,
    }
    
    impl MockLibraryRepository {
        pub fn new() -> Self {
            Self {
                storage: Arc::new(RwLock::new(HashMap::new())),
                default_id: Arc::new(RwLock::new(None)),
                create_called: Arc::new(RwLock::new(Vec::new())),
                scan_triggered: Arc::new(RwLock::new(Vec::new())),
            }
        }
        
        pub async fn insert_test_library(&self, library: Library) {
            self.storage.write().await.insert(library.id, library);
        }
    }
    
    #[async_trait]
    impl LibraryRepository for MockLibraryRepository {
        async fn get(&self, id: Uuid) -> RepositoryResult<Option<Library>> {
            Ok(self.storage.read().await.get(&id).cloned())
        }
        
        async fn get_all(&self) -> RepositoryResult<Vec<Library>> {
            Ok(self.storage.read().await.values().cloned().collect())
        }
        
        async fn query(&self, query: &LibraryQuery) -> RepositoryResult<Vec<Library>> {
            let storage = self.storage.read().await;
            let libraries: Vec<Library> = storage
                .values()
                .filter(|lib| {
                    if let Some(ref lib_type) = query.library_type {
                        if lib.library_type != *lib_type {
                            return false;
                        }
                    }
                    true
                })
                .cloned()
                .collect();
            Ok(libraries)
        }
        
        async fn get_by_type(&self, library_type: LibraryType) -> RepositoryResult<Vec<Library>> {
            self.query(&LibraryQuery {
                library_type: Some(library_type),
                ..Default::default()
            }).await
        }
        
        async fn exists(&self, id: Uuid) -> RepositoryResult<bool> {
            Ok(self.storage.read().await.contains_key(&id))
        }
        
        async fn get_default(&self) -> RepositoryResult<Option<Library>> {
            if let Some(id) = *self.default_id.read().await {
                self.get(id).await
            } else {
                Ok(self.get_all().await?.into_iter().next())
            }
        }
        
        async fn create(&self, name: String, library_type: LibraryType, paths: Vec<PathBuf>) -> RepositoryResult<Library> {
            self.create_called.write().await.push((name.clone(), library_type, paths.clone()));
            let library = Library {
                id: Uuid::new_v4(),
                name,
                library_type,
                paths,
                scan_interval_minutes: 60,
                last_scan: None,
                enabled: true,
                auto_scan: false,
                watch_for_changes: false,
                analyze_on_scan: false,
                max_retry_attempts: 3,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                media: None,
            };
            self.storage.write().await.insert(library.id, library.clone());
            Ok(library)
        }
        
        async fn update(&self, id: Uuid, name: String, paths: Vec<PathBuf>) -> RepositoryResult<Library> {
            let mut storage = self.storage.write().await;
            if let Some(library) = storage.get_mut(&id) {
                library.name = name;
                library.paths = paths;
                library.updated_at = chrono::Utc::now();
                Ok(library.clone())
            } else {
                Err(super::super::RepositoryError::NotFound {
                    entity_type: "Library".to_string(),
                    id: id.to_string(),
                })
            }
        }
        
        async fn delete(&self, id: Uuid) -> RepositoryResult<bool> {
            Ok(self.storage.write().await.remove(&id).is_some())
        }
        
        async fn set_default(&self, id: Uuid) -> RepositoryResult<()> {
            if self.exists(id).await? {
                *self.default_id.write().await = Some(id);
                Ok(())
            } else {
                Err(super::super::RepositoryError::NotFound {
                    entity_type: "Library".to_string(),
                    id: id.to_string(),
                })
            }
        }
        
        async fn trigger_scan(&self, id: Uuid) -> RepositoryResult<()> {
            self.scan_triggered.write().await.push(id);
            Ok(())
        }
        
        async fn get_scan_status(&self, _id: Uuid) -> RepositoryResult<ScanStatus> {
            Ok(ScanStatus::Idle)
        }
    }
}