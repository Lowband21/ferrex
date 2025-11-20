use crate::{
    database::traits::{ScanState, ScanStatus, ScanType},
    LibraryID, LibraryReference, MediaDatabase, Result,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanOptions {
    pub force_refresh: bool,
    pub skip_file_metadata: bool,
    pub skip_tmdb: bool,
    pub analyze_files: bool,
    pub retry_failed: bool,
    pub max_retries: u32,
    pub batch_size: usize,
    pub concurrent_workers: usize,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            force_refresh: false,
            skip_file_metadata: false,
            skip_tmdb: false,
            analyze_files: false,
            retry_failed: true,
            max_retries: 3,
            batch_size: 100,
            concurrent_workers: 4,
        }
    }
}

/// Manages scan state persistence and recovery
pub struct ScanOrchestrator {
    db: Arc<MediaDatabase>,
    active_scans: Arc<RwLock<Vec<Uuid>>>,
    paused_scans: Arc<RwLock<Vec<Uuid>>>,
}

impl ScanOrchestrator {
    pub fn new(db: Arc<MediaDatabase>) -> Self {
        Self {
            db,
            active_scans: Arc::new(RwLock::new(Vec::new())),
            paused_scans: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create a new scan and persist its state
    pub async fn create_scan(
        &self,
        library: &LibraryReference,
        scan_type: ScanType,
        options: ScanOptions,
    ) -> Result<ScanState> {
        let scan_state = ScanState {
            id: Uuid::new_v4(),
            library_id: library.id,
            scan_type,
            status: ScanStatus::Pending,
            total_folders: 0,
            processed_folders: 0,
            total_files: 0,
            processed_files: 0,
            current_path: None,
            error_count: 0,
            errors: vec![],
            started_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
            options: serde_json::to_value(&options)?,
        };

        // Persist to database
        self.db.backend().create_scan_state(&scan_state).await?;

        // Add to active scans
        self.active_scans.write().await.push(scan_state.id);

        info!(
            "Created {} scan {} for library {}",
            format!("{:?}", scan_type).to_lowercase(),
            scan_state.id,
            library.name
        );

        Ok(scan_state)
    }

    /// Update scan progress
    pub async fn update_scan_progress(
        &self,
        scan_id: Uuid,
        processed_folders: Option<i32>,
        processed_files: Option<i32>,
        current_path: Option<String>,
    ) -> Result<()> {
        if let Some(mut scan_state) = self.db.backend().get_scan_state(scan_id).await? {
            if let Some(folders) = processed_folders {
                scan_state.processed_folders = folders;
            }
            if let Some(files) = processed_files {
                scan_state.processed_files = files;
            }
            if let Some(path) = current_path {
                scan_state.current_path = Some(path);
            }
            scan_state.updated_at = Utc::now();

            self.db.backend().update_scan_state(&scan_state).await?;
        }

        Ok(())
    }

    /// Add an error to the scan state
    pub async fn add_scan_error(&self, scan_id: Uuid, error: String) -> Result<()> {
        if let Some(mut scan_state) = self.db.backend().get_scan_state(scan_id).await? {
            scan_state.error_count += 1;
            scan_state.errors.push(error);
            scan_state.updated_at = Utc::now();

            self.db.backend().update_scan_state(&scan_state).await?;
        }

        Ok(())
    }

    /// Mark scan as started
    pub async fn start_scan(&self, scan_id: Uuid) -> Result<()> {
        if let Some(mut scan_state) = self.db.backend().get_scan_state(scan_id).await? {
            scan_state.status = ScanStatus::Running;
            scan_state.started_at = Utc::now();
            scan_state.updated_at = Utc::now();

            self.db.backend().update_scan_state(&scan_state).await?;

            info!("Started scan {}", scan_id);
        }

        Ok(())
    }

    /// Mark scan as completed
    pub async fn complete_scan(&self, scan_id: Uuid) -> Result<()> {
        if let Some(mut scan_state) = self.db.backend().get_scan_state(scan_id).await? {
            scan_state.status = ScanStatus::Completed;
            scan_state.completed_at = Some(Utc::now());
            scan_state.updated_at = Utc::now();

            self.db.backend().update_scan_state(&scan_state).await?;

            // Remove from active scans
            self.active_scans.write().await.retain(|&id| id != scan_id);

            info!(
                "Completed scan {} - processed {}/{} folders, {}/{} files with {} errors",
                scan_id,
                scan_state.processed_folders,
                scan_state.total_folders,
                scan_state.processed_files,
                scan_state.total_files,
                scan_state.error_count
            );
        }

        Ok(())
    }

    /// Mark scan as failed
    pub async fn fail_scan(&self, scan_id: Uuid, error: String) -> Result<()> {
        if let Some(mut scan_state) = self.db.backend().get_scan_state(scan_id).await? {
            scan_state.status = ScanStatus::Failed;
            scan_state.errors.push(format!("FATAL: {}", error));
            scan_state.completed_at = Some(Utc::now());
            scan_state.updated_at = Utc::now();

            self.db.backend().update_scan_state(&scan_state).await?;

            // Remove from active scans
            self.active_scans.write().await.retain(|&id| id != scan_id);

            error!("Scan {} failed: {}", scan_id, error);
        }

        Ok(())
    }

    /// Pause a running scan
    pub async fn pause_scan(&self, scan_id: Uuid) -> Result<()> {
        if let Some(mut scan_state) = self.db.backend().get_scan_state(scan_id).await? {
            if scan_state.status != ScanStatus::Running {
                return Ok(());
            }

            scan_state.status = ScanStatus::Paused;
            scan_state.updated_at = Utc::now();

            self.db.backend().update_scan_state(&scan_state).await?;

            // Move from active to paused
            self.active_scans.write().await.retain(|&id| id != scan_id);
            self.paused_scans.write().await.push(scan_id);

            info!("Paused scan {}", scan_id);
        }

        Ok(())
    }

    /// Resume a paused scan
    pub async fn resume_scan(&self, scan_id: Uuid) -> Result<ScanState> {
        if let Some(mut scan_state) = self.db.backend().get_scan_state(scan_id).await? {
            if scan_state.status != ScanStatus::Paused {
                return Ok(scan_state);
            }

            scan_state.status = ScanStatus::Running;
            scan_state.updated_at = Utc::now();

            self.db.backend().update_scan_state(&scan_state).await?;

            // Move from paused to active
            self.paused_scans.write().await.retain(|&id| id != scan_id);
            self.active_scans.write().await.push(scan_id);

            info!(
                "Resumed scan {} at folder {}/{}, file {}/{}",
                scan_id,
                scan_state.processed_folders,
                scan_state.total_folders,
                scan_state.processed_files,
                scan_state.total_files
            );

            Ok(scan_state)
        } else {
            Err(crate::MediaError::NotFound(format!(
                "Scan {} not found",
                scan_id
            )))
        }
    }

    /// Cancel a scan
    pub async fn cancel_scan(&self, scan_id: Uuid) -> Result<()> {
        if let Some(mut scan_state) = self.db.backend().get_scan_state(scan_id).await? {
            scan_state.status = ScanStatus::Cancelled;
            scan_state.completed_at = Some(Utc::now());
            scan_state.updated_at = Utc::now();

            self.db.backend().update_scan_state(&scan_state).await?;

            // Remove from both active and paused
            self.active_scans.write().await.retain(|&id| id != scan_id);
            self.paused_scans.write().await.retain(|&id| id != scan_id);

            info!("Cancelled scan {}", scan_id);
        }

        Ok(())
    }

    /// Get active scans for a library
    pub async fn get_active_scans(&self, library_id: Option<Uuid>) -> Result<Vec<ScanState>> {
        self.db.backend().get_active_scans(library_id).await
    }

    /// Get the latest scan of a specific type
    pub async fn get_latest_scan(
        &self,
        library_id: LibraryID,
        scan_type: ScanType,
    ) -> Result<Option<ScanState>> {
        self.db
            .backend()
            .get_latest_scan(library_id, scan_type)
            .await
    }

    /// Check if a scan should be resumed on startup
    pub async fn recover_interrupted_scans(&self) -> Result<Vec<ScanState>> {
        let active_scans = self.db.backend().get_active_scans(None).await?;
        let mut recovered = Vec::new();

        for mut scan in active_scans {
            if scan.status == ScanStatus::Running {
                // Mark as paused so it can be resumed properly
                scan.status = ScanStatus::Paused;
                scan.updated_at = Utc::now();
                self.db.backend().update_scan_state(&scan).await?;

                self.paused_scans.write().await.push(scan.id);
                recovered.push(scan);
            }
        }

        if !recovered.is_empty() {
            info!("Recovered {} interrupted scans", recovered.len());
        }

        Ok(recovered)
    }

    /// Check if a scan is currently active
    pub async fn is_scan_active(&self, scan_id: Uuid) -> bool {
        self.active_scans.read().await.contains(&scan_id)
    }

    /// Check if a scan is paused
    pub async fn is_scan_paused(&self, scan_id: Uuid) -> bool {
        self.paused_scans.read().await.contains(&scan_id)
    }

    /// Get scan state
    pub async fn get_scan_state(&self, scan_id: Uuid) -> Result<Option<ScanState>> {
        self.db.backend().get_scan_state(scan_id).await
    }
}
