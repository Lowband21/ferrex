use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::LibraryID;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanRequest {
    pub library_id: LibraryID,
    pub force_refresh: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanResponse {
    pub status: ScanStatus,
    pub scan_id: Option<Uuid>,
    pub message: String,
}

impl ScanResponse {
    pub fn new(status: ScanStatus, scan_id: Option<Uuid>, message: String) -> Self {
        ScanResponse {
            status,
            scan_id,
            message,
        }
    }

    pub fn new_scan_started(scan_id: Uuid, message: String) -> Self {
        ScanResponse {
            status: ScanStatus::Scanning,
            scan_id: Some(scan_id),
            message,
        }
    }

    pub fn new_failed(message: String) -> Self {
        ScanResponse {
            status: ScanStatus::Failed,
            scan_id: None,
            message,
        }
    }

    pub fn new_canceled(scan_id: Uuid) -> Self {
        ScanResponse {
            status: ScanStatus::Cancelled,
            scan_id: Some(scan_id),
            message: "Scan canceled".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgress {
    pub scan_id: Uuid,
    pub status: ScanStatus,
    pub paths: Vec<PathBuf>,
    pub library_names: Vec<String>,
    pub library_ids: Vec<String>,
    pub folders_to_scan: usize,
    pub folders_scanned: usize,
    pub movies_scanned: usize,
    pub series_scanned: usize,
    pub seasons_scanned: usize,
    pub episodes_scanned: usize,
    pub skipped_samples: usize,
    pub errors: Vec<String>,
    pub current_media: Option<String>,
    pub current_library: Option<String>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub estimated_time_remaining: Option<std::time::Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScanStatus {
    Pending,
    Scanning,
    Completed,
    Failed,
    Cancelled,
}
