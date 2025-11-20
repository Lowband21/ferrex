use serde::{Deserialize, Serialize};

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
