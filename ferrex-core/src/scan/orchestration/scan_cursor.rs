//! Scan cursor tracking for incremental filesystem scanning.
//!
//! Scan cursors enable efficient incremental scanning by storing a hash of the
//! directory listing and metadata. When re-scanning, we can quickly determine
//! if a folder has changed by comparing hashes.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::Result;
use crate::types::ids::LibraryId;

/// Unique identifier for a scan cursor.
#[derive(
    Clone, Debug, Hash, Eq, PartialEq, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct ScanCursorId {
    pub library_id: LibraryId,
    pub path_hash: u64,
}

impl ScanCursorId {
    pub fn new(library_id: LibraryId, paths: &Vec<PathBuf>) -> Self {
        let mut hasher = DefaultHasher::new();
        paths.hash(&mut hasher);
        Self {
            library_id,
            path_hash: hasher.finish(),
        }
    }
}

/// Persistent scan cursor that tracks folder state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanCursor {
    pub id: ScanCursorId,
    pub folder_path_norm: String,
    pub listing_hash: String,
    pub entry_count: usize,
    pub last_scan_at: DateTime<Utc>,
    pub last_modified_at: Option<DateTime<Utc>>,
    pub device_id: Option<String>,
}

/// Entry in a directory listing for hash computation.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct ListingEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified_ms: i64,
}

impl ListingEntry {
    pub fn hash_input(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.name,
            if self.is_dir { "d" } else { "f" },
            self.size,
            self.modified_ms
        )
    }
}

/// Compute a deterministic hash of a directory listing.
pub fn compute_listing_hash(entries: &[ListingEntry]) -> String {
    let mut sorted = entries.to_vec();
    sorted.sort();

    let mut hasher = Sha256::new();
    for entry in &sorted {
        hasher.update(entry.hash_input().as_bytes());
        hasher.update(b"\n");
    }

    format!("{:x}", hasher.finalize())
}

/// Result of comparing a new listing against a stored cursor.
#[derive(Debug)]
pub enum CursorDiff {
    /// No cursor exists, this is the first scan
    NoCursor,
    /// Cursor exists and listing hasn't changed
    Unchanged,
    /// Cursor exists but listing has changed
    Changed {
        old_hash: String,
        new_hash: String,
        added_count: usize,
        removed_count: usize,
    },
}

impl CursorDiff {
    pub fn requires_scan(&self) -> bool {
        !matches!(self, CursorDiff::Unchanged)
    }
}

/// Compare new listing against stored cursor.
pub fn diff_cursor(
    cursor: Option<&ScanCursor>,
    entries: &[ListingEntry],
) -> CursorDiff {
    let new_hash = compute_listing_hash(entries);

    match cursor {
        None => CursorDiff::NoCursor,
        Some(cursor) if cursor.listing_hash == new_hash => {
            CursorDiff::Unchanged
        }
        Some(cursor) => {
            // Simple count-based diff for now
            let old_count = cursor.entry_count;
            let new_count = entries.len();

            CursorDiff::Changed {
                old_hash: cursor.listing_hash.clone(),
                new_hash,
                added_count: new_count.saturating_sub(old_count),
                removed_count: old_count.saturating_sub(new_count),
            }
        }
    }
}

/// Normalize path for consistent cursor keys.
pub fn normalize_path(path: &Path) -> String {
    // TODO: Handle case sensitivity based on filesystem
    path.to_string_lossy().to_string()
}

/// Repository trait for persisting scan cursors.
#[async_trait::async_trait]
pub trait ScanCursorRepository: Send + Sync {
    /// Get cursor for a specific folder.
    async fn get(&self, id: &ScanCursorId) -> Result<Option<ScanCursor>>;

    /// Get all cursors for a library.
    async fn list_by_library(
        &self,
        library_id: LibraryId,
    ) -> Result<Vec<ScanCursor>>;

    /// Store or update a cursor.
    async fn upsert(&self, cursor: ScanCursor) -> Result<()>;

    /// Delete cursors for a library.
    async fn delete_by_library(&self, library_id: LibraryId) -> Result<usize>;

    /// Get cursors that haven't been scanned recently.
    async fn list_stale(
        &self,
        library_id: LibraryId,
        older_than: DateTime<Utc>,
    ) -> Result<Vec<ScanCursor>>;
}
