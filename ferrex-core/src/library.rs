use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::MediaReference;

/// Represents a media library with a specific type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    pub id: Uuid,
    pub name: String,
    pub library_type: LibraryType,
    pub paths: Vec<PathBuf>,
    pub scan_interval_minutes: u32,
    pub last_scan: Option<chrono::DateTime<chrono::Utc>>,
    pub enabled: bool,
    pub auto_scan: bool,
    pub watch_for_changes: bool,
    pub analyze_on_scan: bool,
    pub max_retry_attempts: u32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub media: Option<Vec<MediaReference>>,
}

/// The type of content a library contains
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum LibraryType {
    Movies,
    TvShows,
}

impl std::fmt::Display for LibraryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LibraryType::Movies => write!(f, "Movies"),
            LibraryType::TvShows => write!(f, "TV Shows"),
        }
    }
}

impl Library {
    /// Create a new library
    pub fn new(name: String, library_type: LibraryType, paths: Vec<PathBuf>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            name,
            library_type,
            paths,
            scan_interval_minutes: 60,
            last_scan: None,
            enabled: true,
            auto_scan: true,
            watch_for_changes: true,
            analyze_on_scan: false,
            max_retry_attempts: 3,
            created_at: now,
            updated_at: now,
            media:  None,
        }
    }

    /// Check if the library needs scanning based on the interval
    pub fn needs_scan(&self) -> bool {
        if !self.enabled {
            return false;
        }

        match self.last_scan {
            None => true,
            Some(last_scan) => {
                let elapsed = chrono::Utc::now().signed_duration_since(last_scan);
                elapsed.num_minutes() >= self.scan_interval_minutes as i64
            }
        }
    }

    /// Update the last scan timestamp
    pub fn update_last_scan(&mut self) {
        self.last_scan = Some(chrono::Utc::now());
        self.updated_at = chrono::Utc::now();
    }
}

// Request types moved to api_types.rs to avoid conflicts

/// Library scan result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryScanResult {
    pub library_id: Uuid,
    pub library_name: String,
    pub total_files: usize,
    pub new_files: usize,
    pub updated_files: usize,
    pub deleted_files: usize,
    pub errors: Vec<String>,
    pub duration_seconds: f64,
}
