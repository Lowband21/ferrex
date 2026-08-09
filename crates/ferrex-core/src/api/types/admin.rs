use serde::{Deserialize, Serialize};

/// Authenticated administrative database-reset request.
///
/// The confirmation value intentionally travels with every request so callers
/// cannot trigger destructive maintenance through an empty POST.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResetDatabaseRequest {
    /// Reset all users and authentication data.
    pub reset_users: bool,
    /// Reset all libraries and their library-owned data.
    pub reset_libraries: bool,
    /// Reset media entries. Library deletion currently owns the media cascade.
    pub reset_media: bool,
    /// Destructive-operation confirmation phrase.
    pub confirmation: String,
}

impl ResetDatabaseRequest {
    /// Build the full wipe requested by the player dashboard's Clear All Data action.
    pub fn clear_all_data() -> Self {
        Self {
            reset_users: true,
            reset_libraries: true,
            reset_media: true,
            confirmation: "RESET_DATABASE".to_string(),
        }
    }
}

/// Counts returned after an administrative database reset.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResetDatabaseResult {
    /// Number of users deleted.
    pub users_deleted: usize,
    /// Number of sessions deleted.
    pub sessions_deleted: usize,
    /// Number of roles restored to their default definitions.
    pub roles_reset: usize,
    /// Number of libraries deleted.
    pub libraries_deleted: usize,
    /// Number of media items deleted directly.
    pub media_deleted: usize,
    /// Number of watch-status entries deleted directly.
    pub watch_status_deleted: usize,
}

#[cfg(test)]
mod reset_database_tests {
    use super::ResetDatabaseRequest;

    #[test]
    fn clear_all_data_requests_every_destructive_reset() {
        let request = ResetDatabaseRequest::clear_all_data();

        assert!(request.reset_users);
        assert!(request.reset_libraries);
        assert!(request.reset_media);
        assert_eq!(request.confirmation, "RESET_DATABASE");
    }
}

/// Request parameters accepted by the media root browser endpoint.
///
/// `path` is expected to be a relative POSIX-style path anchored at the server's
/// configured media root. Empty or `.` resolve to the root itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaRootBrowseRequest {
    #[serde(default)]
    pub path: Option<String>,
}

/// Describes a single file-system entry relative to the media root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaRootEntry {
    /// Base name of the entry.
    pub name: String,
    /// Relative path from the media root using `/` separators.
    pub relative_path: String,
    /// Kind of entry detected.
    pub kind: MediaRootEntryKind,
    /// True when the entry is a symbolic link. Links are surfaced to humans but
    /// callers should gate navigation/selection on `kind`.
    #[serde(default)]
    pub is_symlink: bool,
}

/// Entry kind surfaced to the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaRootEntryKind {
    Directory,
    File,
    Other,
}

/// Breadcrumb segment from media root to the current directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaRootBreadcrumb {
    /// Human-readable label (usually the folder name or `/` for root).
    pub label: String,
    /// Relative path that navigating to this breadcrumb should request.
    pub relative_path: String,
}

/// Response payload returned by the media root browser endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaRootBrowseResponse {
    /// Absolute path the server is using inside the container/host.
    pub media_root: String,
    /// Relative path (within `media_root`) for this listing. Empty string
    /// indicates the root itself.
    pub current_path: String,
    /// Relative path to the parent directory, if any.
    pub parent_path: Option<String>,
    /// Normalised POSIX-style display path (`/` separators) for transparency.
    pub display_path: String,
    /// Breadcrumbs enabling easy navigation back to ancestors.
    pub breadcrumbs: Vec<MediaRootBreadcrumb>,
    /// Directory/file entries located under `current_path`.
    pub entries: Vec<MediaRootEntry>,
}
