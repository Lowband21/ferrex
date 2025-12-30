use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::domain::media::tv_parser::TvParser;
use crate::domain::scan::actors::messages::MediaKindHint;
use crate::domain::scan::orchestration::context::{
    FolderScanContext, MovieScanHierarchy, ScanNodeKind, SeasonFolderPath,
    SeasonFolderScanContext, SeasonLink, SeriesFolderScanContext, SeriesHint,
    SeriesLink,
};
use crate::error::{MediaError, Result};
use ferrex_model::{MediaID, VideoMediaType};

use super::messages::{FolderScanSummary, MediaFileDiscovered};
use crate::domain::scan::orchestration::job::{
    FolderScanJob, MediaFingerprint,
};
use crate::domain::scan::orchestration::scan_cursor::{
    ListingEntry, compute_listing_hash,
};

/// Work item accepted by a `FolderScanActor`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FolderScanCommand {
    pub job: FolderScanJob,
}

impl FolderScanCommand {
    pub fn context(&self) -> &FolderScanContext {
        &self.job.context
    }
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

    pub fn complete(
        mut self,
        mut summary: FolderScanSummary,
    ) -> FolderScanSummary {
        let completed_at = Utc::now();
        self.completed_at = Some(completed_at);
        summary.completed_at = completed_at;
        summary
    }
}

/// Trait describing behaviour required from folder scan actors.
#[async_trait]
pub trait FolderScanActor: Send + Sync {
    async fn plan_listing(
        &self,
        job: &FolderScanJob,
    ) -> Result<FolderListingPlan>;

    async fn discover_media(
        &self,
        plan: &FolderListingPlan,
        job: &FolderScanJob,
    ) -> Result<Vec<MediaFileDiscovered>>;

    async fn derive_child_contexts(
        &self,
        plan: &FolderListingPlan,
        command: &FolderScanJob,
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
pub struct DefaultFolderScanActor;

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
        Self
    }

    fn is_media_file(&self, path: &Path) -> bool {
        is_media_file_path(path)
    }

    async fn list_directory(&self, path: &Path) -> Result<Vec<ListingEntry>> {
        let mut entries = Vec::new();
        let mut dir = fs::read_dir(path).await.map_err(|e| {
            MediaError::Io(std::io::Error::other(format!(
                "Failed to read directory {}: {}",
                path.display(),
                e
            )))
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
                        .and_then(|t| {
                            t.duration_since(std::time::UNIX_EPOCH).ok()
                        })
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or_default();
                    (is_dir, size, modified_ms)
                }
                Err(e) => {
                    tracing::warn!(target: "scan::jobs", entry = %name_string, path = %path.display(), error = %e, "skipping entry due to metadata error");
                    // Skip this entry altogether
                    // TODO: Collect the failures and allow rematching
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
    async fn plan_listing(
        &self,
        job: &FolderScanJob,
    ) -> Result<FolderListingPlan> {
        let context = &job.context;
        let folder_path = PathBuf::from(context.folder_path_norm());
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

                match context {
                    FolderScanContext::Series(_) => {
                        if TvParser::parse_season_folder(&entry.name).is_some()
                        {
                            directories.push(entry_path);
                        } else {
                            tracing::debug!(
                                target: "scan::jobs",
                                folder = %folder_path.display(),
                                child = %entry.name,
                                "ignoring non-season subdirectory under series root"
                            );
                        }
                    }
                    FolderScanContext::Season(_) => {
                        tracing::debug!(
                            target: "scan::jobs",
                            folder = %folder_path.display(),
                            child = %entry.name,
                            "ignoring subdirectory under season folder (extras unsupported for now)"
                        );
                    }
                    FolderScanContext::Movie(_) => {
                        tracing::debug!(
                            target: "scan::jobs",
                            folder = %folder_path.display(),
                            child = %entry.name,
                            "ignoring subdirectory under movie root (extras unsupported for now)"
                        );
                    }
                }
            } else if self.is_media_file(&entry_path) {
                match context {
                    FolderScanContext::Season(_)
                    | FolderScanContext::Movie(_) => {
                        media_files.push(entry_path);
                    }
                    FolderScanContext::Series(_) => {
                        tracing::warn!(
                            target: "scan::jobs",
                            file = %entry_path.display(),
                            "ignoring media file directly under series root (expected season folders)"
                        );
                    }
                }
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
        job: &FolderScanJob,
    ) -> Result<Vec<MediaFileDiscovered>> {
        let context = &job.context;

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

            let (variant, kind_hint, node, hierarchy) = match context {
                FolderScanContext::Movie(movie_ctx) => {
                    let hierarchy = MovieScanHierarchy {
                        movie_root_path: movie_ctx.movie_root_path.clone(),
                        movie_id: None,
                        extra_tag: None,
                    };
                    (
                        VideoMediaType::Movie,
                        MediaKindHint::Movie,
                        ScanNodeKind::MovieFolder,
                        crate::domain::scan::AnalyzeScanHierarchy::Movie(
                            hierarchy,
                        ),
                    )
                }
                FolderScanContext::Season(season_ctx) => {
                    let info = TvParser::parse_episode_info(file.as_path())
                        .ok_or_else(|| {
                            MediaError::InvalidMedia(format!(
                                "episode file did not match parsing rules: {}",
                                file.display()
                            ))
                        })?;

                    if info.season != season_ctx.season_number {
                        return Err(MediaError::InvalidMedia(format!(
                            "episode season mismatch (expected S{:02}, got S{:02}) for {}",
                            season_ctx.season_number,
                            info.season,
                            file.display()
                        )));
                    }

                    let series_folder_name =
                        Path::new(season_ctx.series_root_path.as_str())
                            .file_name()
                            .and_then(|n| n.to_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| {
                                season_ctx.series_root_path.as_str().to_string()
                            });

                    let series_hint = SeriesHint {
                        title: series_folder_name.clone(),
                        slug: None,
                        year: None,
                        region: None,
                    };

                    let season_hierarchy = crate::domain::scan::orchestration::context::SeasonScanHierarchy {
                        series_root_path: season_ctx.series_root_path.clone(),
                        series: SeriesLink::Hint(series_hint),
                        season: SeasonLink::Number(season_ctx.season_number),
                    };

                    let hierarchy =
                        crate::domain::scan::orchestration::context::EpisodeScanHierarchy::from_season_hierarch(
                            season_hierarchy,
                            crate::domain::scan::orchestration::context::EpisodeLink::Hint(
                                crate::domain::scan::orchestration::context::EpisodeHint {
                                    number: info.episode,
                                    title: None,
                                },
                            ),
                        );

                    (
                        VideoMediaType::Episode,
                        MediaKindHint::Episode,
                        ScanNodeKind::EpisodeFile,
                        crate::domain::scan::AnalyzeScanHierarchy::Episode(
                            hierarchy,
                        ),
                    )
                }
                FolderScanContext::Series(_) => {
                    return Err(MediaError::InvalidMedia(
                        "series root context should not discover media files"
                            .into(),
                    ));
                }
            };

            let media_id = MediaID::new(variant);

            out.push(MediaFileDiscovered {
                library_id: context.library_id(),
                path_norm: file.to_string_lossy().to_string(),
                fingerprint,
                classified_as: kind_hint,
                media_id,
                variant,
                node: node.clone(),
                hierarchy: hierarchy.clone(),
                context: context.clone(),
                scan_reason: job.scan_reason,
            });
        }
        Ok(out)
    }

    async fn derive_child_contexts(
        &self,
        plan: &FolderListingPlan,
        job: &FolderScanJob,
    ) -> Result<Vec<FolderScanContext>> {
        let parent = &job.context;

        let mut children = Vec::new();

        let FolderScanContext::Series(SeriesFolderScanContext {
            library_id,
            series_root_path,
        }) = parent
        else {
            return Ok(children);
        };

        for dir in &plan.directories {
            let folder_path_norm = dir.to_string_lossy().to_string();
            match SeasonFolderPath::try_new_under_series_root(
                series_root_path,
                folder_path_norm,
            ) {
                Ok((season_folder_path, season_number)) => {
                    children.push(FolderScanContext::Season(
                        SeasonFolderScanContext {
                            library_id: *library_id,
                            series_root_path: series_root_path.clone(),
                            season_folder_path,
                            season_number,
                        },
                    ));
                }
                Err(err) => {
                    tracing::warn!(
                        target: "scan::jobs",
                        error = %err,
                        folder = %dir.display(),
                        "skipping child directory (not a valid season folder)"
                    );
                    continue;
                }
            }
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
