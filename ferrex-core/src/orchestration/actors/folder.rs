use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::{LibraryID, LibraryType, MediaError, Result};
use tracing::info;

use super::messages::{FolderScanSummary, MediaFileDiscovered, ParentDescriptors};
use crate::orchestration::job::{FolderScanJob, MediaAnalyzeJob, MediaFingerprint, ScanReason};
use crate::orchestration::scan_cursor::{ListingEntry, compute_listing_hash};

/// Context supplied to folder scans so they can infer parent relationships.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FolderScanContext {
    pub library_id: LibraryID,
    pub folder_path_norm: String,
    pub parent: ParentDescriptors,
    pub reason: ScanReason,
}

/// Work item accepted by a `FolderScanActor`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FolderScanCommand {
    pub job: FolderScanJob,
    pub context: FolderScanContext,
}

/// Summary of filesystem entries to process.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FolderListingPlan {
    pub directories: Vec<PathBuf>,
    pub media_files: Vec<PathBuf>,
    pub ancillary_files: Vec<PathBuf>,
    pub generated_listing_hash: String,
}

/// Captures state while the folder scan actor is running.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FolderScanState {
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub discovered: Vec<MediaFileDiscovered>,
    pub enqueued_folders: Vec<FolderScanContext>,
}

impl FolderScanState {
    pub fn new(started_at: DateTime<Utc>) -> Self {
        Self {
            started_at,
            completed_at: None,
            discovered: Vec::new(),
            enqueued_folders: Vec::new(),
        }
    }

    pub fn complete(mut self, mut summary: FolderScanSummary) -> FolderScanSummary {
        let completed_at = Utc::now();
        self.completed_at = Some(completed_at);
        summary.completed_at = completed_at;
        summary
    }
}

/// Trait describing behaviour required from folder scan actors.
#[async_trait]
pub trait FolderScanActor: Send + Sync {
    async fn plan_listing(&self, command: &FolderScanCommand) -> Result<FolderListingPlan>;

    async fn discover_media(
        &self,
        plan: &FolderListingPlan,
        context: &FolderScanContext,
    ) -> Result<Vec<MediaFileDiscovered>>;

    async fn derive_child_contexts(
        &self,
        plan: &FolderListingPlan,
        parent: &FolderScanContext,
    ) -> Result<Vec<FolderScanContext>>;

    fn finalize(
        &self,
        context: &FolderScanContext,
        plan: &FolderListingPlan,
        discovered: &[MediaFileDiscovered],
        children: &[FolderScanContext],
    ) -> Result<FolderScanSummary>;
}

/// Stateless `FolderScanActor` that performs filesystem operations for one folder per job.
#[derive(Debug)]
pub struct DefaultFolderScanActor {
    supported_extensions: Vec<String>,
}

/// Shared helper so other actors (e.g., LibraryActor) can apply the
/// same definition of what constitutes a media file.
pub fn is_supported_media_ext(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "mkv"
            | "mp4"
            | "avi"
            | "mov"
            | "webm"
            | "flv"
            | "wmv"
            | "mpg"
            | "mpeg"
            | "m4v"
            | "3gp"
            | "ts"
    )
}

pub fn is_media_file_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(is_supported_media_ext)
        .unwrap_or(false)
}

impl Default for DefaultFolderScanActor {
    fn default() -> Self {
        Self::new()
    }
}

impl DefaultFolderScanActor {
    pub fn new() -> Self {
        Self {
            supported_extensions: vec![
                "mkv".into(),
                "mp4".into(),
                "avi".into(),
                "mov".into(),
                "webm".into(),
                "flv".into(),
                "wmv".into(),
                "mpg".into(),
                "mpeg".into(),
                "m4v".into(),
                "3gp".into(),
                "ts".into(),
            ],
        }
    }

    fn is_media_file(&self, path: &Path) -> bool {
        is_media_file_path(path)
    }

    fn is_subfolder_relevant(&self, name: &str, library_type: LibraryType) -> bool {
        match library_type {
            LibraryType::Series => {
                // Season folders, specials, extras
                name.to_lowercase().starts_with("season")
                    || name.eq_ignore_ascii_case("specials")
                    || name.eq_ignore_ascii_case("extras")
            }
            LibraryType::Movies => {
                // Extras, featurettes, behind the scenes
                name.eq_ignore_ascii_case("extras")
                    || name.eq_ignore_ascii_case("featurettes")
                    || name.eq_ignore_ascii_case("behind the scenes")
            }
        }
    }

    fn should_filter_directories(&self, parent: &ParentDescriptors) -> bool {
        parent.movie_id.is_some()
            || parent.series_id.is_some()
            || parent.season_id.is_some()
            || parent.episode_id.is_some()
            || parent.extra_tag.is_some()
    }

    async fn list_directory(&self, path: &Path) -> Result<Vec<ListingEntry>> {
        let mut entries = Vec::new();
        let mut dir = fs::read_dir(path).await.map_err(|e| {
            MediaError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to read directory: {}", e),
            ))
        })?;

        while let Some(entry_res) = dir.next_entry().await.transpose() {
            let entry = match entry_res {
                Ok(ent) => ent,
                Err(e) => {
                    tracing::warn!(target: "scan::jobs", path = %path.display(), error = %e, "skipping unreadable directory entry");
                    continue;
                }
            };

            let name_string = entry.file_name().to_string_lossy().to_string();
            let (is_dir, size, modified_ms) = match entry.metadata().await {
                Ok(metadata) => {
                    let is_dir = metadata.is_dir();
                    let size = metadata.len();
                    let modified_ms = metadata
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or_default();
                    (is_dir, size, modified_ms)
                }
                Err(e) => {
                    tracing::warn!(target: "scan::jobs", entry = %name_string, path = %path.display(), error = %e, "skipping entry due to metadata error");
                    // Skip this entry altogether
                    continue;
                }
            };

            entries.push(ListingEntry {
                name: name_string,
                is_dir,
                size,
                modified_ms,
            });
        }

        Ok(entries)
    }
}

#[async_trait]
impl FolderScanActor for DefaultFolderScanActor {
    async fn plan_listing(&self, command: &FolderScanCommand) -> Result<FolderListingPlan> {
        info!(
            target: "scan::jobs",
            library_id = %command.context.library_id,
            folder = %command.context.folder_path_norm,
            reason = ?command.context.reason,
            "starting folder scan"
        );
        let folder_path = PathBuf::from(&command.context.folder_path_norm);
        let entries = self.list_directory(&folder_path).await?;

        let mut directories = Vec::new();
        let mut media_files = Vec::new();
        let mut ancillary_files = Vec::new();

        for entry in &entries {
            let entry_path = folder_path.join(&entry.name);
            if entry.is_dir {
                // Skip hidden/system directories up front
                if entry.name.starts_with('.') {
                    continue;
                }

                let filter = self.should_filter_directories(&command.context.parent);
                if filter {
                    if let Some(lib_type) = command.context.parent.resolved_type {
                        if !self.is_subfolder_relevant(&entry.name, lib_type) {
                            continue;
                        }
                    }
                }

                directories.push(entry_path);
            } else if self.is_media_file(&entry_path) {
                media_files.push(entry_path);
            } else {
                ancillary_files.push(entry_path);
            }
        }

        let generated_listing_hash = compute_listing_hash(&entries);
        Ok(FolderListingPlan {
            directories,
            media_files,
            ancillary_files,
            generated_listing_hash,
        })
    }

    async fn discover_media(
        &self,
        plan: &FolderListingPlan,
        context: &FolderScanContext,
    ) -> Result<Vec<MediaFileDiscovered>> {
        let mut out = Vec::new();
        for file in &plan.media_files {
            let md = match fs::metadata(file).await {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(target: "scan::jobs", file = %file.display(), error = %e, "skipping file due to metadata error");
                    continue;
                }
            };
            let modified_ms = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or_default();

            let fingerprint = MediaFingerprint {
                device_id: None,
                inode: None,
                size: md.len(),
                mtime: modified_ms,
                weak_hash: None,
            };

            let kind_hint = match context.parent.resolved_type {
                Some(LibraryType::Movies) => super::messages::MediaKindHint::Movie,
                Some(LibraryType::Series) => super::messages::MediaKindHint::Episode,
                None => super::messages::MediaKindHint::Unknown,
            };

            out.push(MediaFileDiscovered {
                library_id: context.library_id,
                path_norm: file.to_string_lossy().to_string(),
                fingerprint,
                classified_as: kind_hint,
                context: context.clone(),
            });
        }
        Ok(out)
    }

    async fn derive_child_contexts(
        &self,
        plan: &FolderListingPlan,
        parent: &FolderScanContext,
    ) -> Result<Vec<FolderScanContext>> {
        // Always derive children; persistence-level dedupe prevents redundant
        // enqueue even during bulk seed, and this keeps recursion uniform.
        let mut children = Vec::new();
        for dir in &plan.directories {
            children.push(FolderScanContext {
                library_id: parent.library_id,
                folder_path_norm: dir.to_string_lossy().to_string(),
                parent: parent.parent.clone(),
                reason: parent.reason.clone(),
            });
        }
        Ok(children)
    }

    fn finalize(
        &self,
        context: &FolderScanContext,
        plan: &FolderListingPlan,
        discovered: &[MediaFileDiscovered],
        children: &[FolderScanContext],
    ) -> Result<FolderScanSummary> {
        Ok(FolderScanSummary {
            context: context.clone(),
            discovered_files: discovered.len(),
            enqueued_subfolders: children.len(),
            listing_hash: plan.generated_listing_hash.clone(),
            completed_at: Utc::now(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LibraryID, LibraryType, MovieID};
    use tempfile::tempdir;
    use uuid::Uuid;

    fn make_command(path: &Path, parent: ParentDescriptors) -> FolderScanCommand {
        let folder_path_norm = path.to_string_lossy().to_string();
        let library_id = LibraryID(Uuid::now_v7());
        FolderScanCommand {
            job: FolderScanJob {
                library_id,
                folder_path_norm: folder_path_norm.clone(),
                parent_context: None,
                scan_reason: ScanReason::BulkSeed,
                enqueue_time: Utc::now(),
                device_id: None,
            },
            context: FolderScanContext {
                library_id,
                folder_path_norm,
                parent,
                reason: ScanReason::BulkSeed,
            },
        }
    }

    #[tokio::test]
    async fn plan_listing_includes_primary_subfolders_when_structure_unknown() {
        let tmp = tempdir().expect("tempdir");
        let movie_folder = tmp.path().join("Some Movie (2020)");
        tokio::fs::create_dir(&movie_folder)
            .await
            .expect("movie folder");

        let actor = DefaultFolderScanActor::new();
        let command = make_command(
            tmp.path(),
            ParentDescriptors {
                resolved_type: Some(LibraryType::Movies),
                ..ParentDescriptors::default()
            },
        );

        let plan = actor
            .plan_listing(&command)
            .await
            .expect("plan listing succeeds");

        assert!(plan.directories.iter().any(|dir| dir == &movie_folder));
    }

    #[tokio::test]
    async fn plan_listing_filters_when_parent_metadata_known() {
        let tmp = tempdir().expect("tempdir");
        let extras_folder = tmp.path().join("Extras");
        let random_folder = tmp.path().join("BehindScenesRaw");
        tokio::fs::create_dir(&extras_folder)
            .await
            .expect("extras folder");
        tokio::fs::create_dir(&random_folder)
            .await
            .expect("random folder");

        let actor = DefaultFolderScanActor::new();
        let command = make_command(
            tmp.path(),
            ParentDescriptors {
                resolved_type: Some(LibraryType::Movies),
                movie_id: Some(MovieID(Uuid::now_v7())),
                ..ParentDescriptors::default()
            },
        );

        let plan = actor
            .plan_listing(&command)
            .await
            .expect("plan listing succeeds");

        assert!(plan.directories.contains(&extras_folder));
        assert!(!plan.directories.contains(&random_folder));
    }
}
