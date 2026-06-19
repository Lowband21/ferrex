use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
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

use super::messages::{
    FolderScanOutcome, FolderScanSummary, MediaFileDiscovered,
};
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
    /// Number of raw directory entries observed before scan-context filtering.
    #[serde(default)]
    pub total_entries: usize,
    /// True when the requested folder no longer exists or is no longer a directory.
    /// Missing folders reconcile as recursive tombstones instead of dead-lettering.
    #[serde(default)]
    pub folder_missing: bool,
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

/// Extension policy applied when classifying files during folder scans.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScannerFileFilterPolicy {
    media_extensions: BTreeSet<String>,
    ignored_extensions: BTreeSet<String>,
    ignored_path_patterns: Vec<String>,
}

impl ScannerFileFilterPolicy {
    pub fn new(
        media_extensions: impl IntoIterator<Item = String>,
        ignored_extensions: impl IntoIterator<Item = String>,
    ) -> Self {
        Self::new_with_ignored_path_patterns(
            media_extensions,
            ignored_extensions,
            Vec::<String>::new(),
        )
    }

    pub fn new_with_ignored_path_patterns(
        media_extensions: impl IntoIterator<Item = String>,
        ignored_extensions: impl IntoIterator<Item = String>,
        ignored_path_patterns: impl IntoIterator<Item = String>,
    ) -> Self {
        let media_extensions = media_extensions
            .into_iter()
            .filter_map(|ext| normalize_extension(&ext))
            .collect();
        let ignored_extensions = ignored_extensions
            .into_iter()
            .filter_map(|ext| normalize_extension(&ext))
            .collect();
        let ignored_path_patterns = ignored_path_patterns
            .into_iter()
            .map(|pattern| pattern.trim().replace('\\', "/"))
            .filter(|pattern| !pattern.is_empty())
            .collect();

        Self {
            media_extensions,
            ignored_extensions,
            ignored_path_patterns,
        }
    }

    pub fn media_extensions(&self) -> impl Iterator<Item = &str> {
        self.media_extensions.iter().map(String::as_str)
    }

    pub fn ignored_extensions(&self) -> impl Iterator<Item = &str> {
        self.ignored_extensions.iter().map(String::as_str)
    }

    pub fn ignored_path_patterns(&self) -> impl Iterator<Item = &str> {
        self.ignored_path_patterns.iter().map(String::as_str)
    }

    pub fn is_supported_media_ext(&self, ext: &str) -> bool {
        let Some(ext) = normalize_extension(ext) else {
            return false;
        };
        self.media_extensions.contains(&ext)
            && !self.ignored_extensions.contains(&ext)
    }

    pub fn is_media_file_path(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|ext| self.is_supported_media_ext(ext))
            .unwrap_or(false)
    }

    pub fn is_ignored_path(&self, path: &Path) -> bool {
        let value = path.to_string_lossy().replace('\\', "/");
        self.ignored_path_patterns
            .iter()
            .any(|pattern| path_pattern_matches(pattern, &value))
    }
}

impl Default for ScannerFileFilterPolicy {
    fn default() -> Self {
        Self::new(
            crate::domain::scan::scanner::settings::default_video_file_extensions_vec(),
            Vec::<String>::new(),
        )
    }
}

fn normalize_extension(ext: &str) -> Option<String> {
    let normalized = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    (!normalized.is_empty()
        && !normalized.contains('/')
        && !normalized.contains('\\')
        && !normalized.contains('*'))
    .then_some(normalized)
}

fn path_pattern_matches(pattern: &str, value: &str) -> bool {
    let pattern = pattern.trim().replace('\\', "/");
    if pattern.is_empty() {
        return false;
    }

    if pattern.contains('*') || pattern.contains('?') {
        return wildcard_match(pattern.as_bytes(), value.as_bytes());
    }

    value == pattern
        || value
            .strip_prefix('/')
            .is_some_and(|trimmed| trimmed == pattern)
        || value
            .split('/')
            .any(|component| component.eq_ignore_ascii_case(&pattern))
}

fn wildcard_match(pattern: &[u8], value: &[u8]) -> bool {
    let (mut pattern_idx, mut value_idx) = (0usize, 0usize);
    let mut star_idx: Option<usize> = None;
    let mut star_value_idx = 0usize;

    while value_idx < value.len() {
        if pattern_idx < pattern.len()
            && (pattern[pattern_idx] == b'?'
                || pattern[pattern_idx].eq_ignore_ascii_case(&value[value_idx]))
        {
            pattern_idx += 1;
            value_idx += 1;
        } else if pattern_idx < pattern.len() && pattern[pattern_idx] == b'*' {
            star_idx = Some(pattern_idx);
            pattern_idx += 1;
            star_value_idx = value_idx;
        } else if let Some(star) = star_idx {
            pattern_idx = star + 1;
            star_value_idx += 1;
            value_idx = star_value_idx;
        } else {
            return false;
        }
    }

    while pattern_idx < pattern.len() && pattern[pattern_idx] == b'*' {
        pattern_idx += 1;
    }

    pattern_idx == pattern.len()
}

/// Folder scan actor that performs filesystem operations for one folder per job.
#[derive(Debug)]
pub struct DefaultFolderScanActor {
    filters: ScannerFileFilterPolicy,
}

/// Shared helper so other actors (e.g., LibraryActor) can apply the
/// same definition of what constitutes a media file.
pub fn is_supported_media_ext(ext: &str) -> bool {
    ScannerFileFilterPolicy::default().is_supported_media_ext(ext)
}

pub fn is_media_file_path(path: &Path) -> bool {
    ScannerFileFilterPolicy::default().is_media_file_path(path)
}

impl Default for DefaultFolderScanActor {
    fn default() -> Self {
        Self::new()
    }
}

impl DefaultFolderScanActor {
    pub fn new() -> Self {
        Self {
            filters: ScannerFileFilterPolicy::default(),
        }
    }

    pub fn with_filter_policy(filters: ScannerFileFilterPolicy) -> Self {
        Self { filters }
    }

    fn is_media_file(&self, path: &Path) -> bool {
        self.filters.is_media_file_path(path)
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
        match fs::metadata(&folder_path).await {
            Ok(metadata) if !metadata.is_dir() => {
                tracing::warn!(
                    target: "scan::jobs",
                    path = %folder_path.display(),
                    "folder scan path is no longer a directory; reconciling as deleted"
                );
                return Ok(FolderListingPlan {
                    directories: Vec::new(),
                    media_files: Vec::new(),
                    ancillary_files: Vec::new(),
                    generated_listing_hash: compute_listing_hash(&[]),
                    total_entries: 0,
                    folder_missing: true,
                });
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                tracing::warn!(
                    target: "scan::jobs",
                    path = %folder_path.display(),
                    "folder scan path no longer exists; reconciling as deleted"
                );
                return Ok(FolderListingPlan {
                    directories: Vec::new(),
                    media_files: Vec::new(),
                    ancillary_files: Vec::new(),
                    generated_listing_hash: compute_listing_hash(&[]),
                    total_entries: 0,
                    folder_missing: true,
                });
            }
            Err(err) => {
                return Err(MediaError::Io(std::io::Error::other(format!(
                    "Failed to stat directory {}: {}",
                    folder_path.display(),
                    err
                ))));
            }
        }

        let entries = self.list_directory(&folder_path).await?;

        let mut directories = Vec::new();
        let mut media_files = Vec::new();
        let mut ancillary_files = Vec::new();

        for entry in &entries {
            let entry_path = folder_path.join(&entry.name);
            if self.filters.is_ignored_path(&entry_path) {
                tracing::debug!(
                    target: "scan::jobs",
                    path = %entry_path.display(),
                    "ignoring scanner-filtered path"
                );
                continue;
            }

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
            total_entries: entries.len(),
            folder_missing: false,
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
        let outcome = if plan.folder_missing {
            FolderScanOutcome::Missing
        } else if !discovered.is_empty() || !children.is_empty() {
            FolderScanOutcome::Changed
        } else if plan.total_entries == 0 {
            FolderScanOutcome::Empty
        } else {
            FolderScanOutcome::Unsupported
        };

        Ok(FolderScanSummary {
            context: context.clone(),
            discovered_files: discovered.len(),
            enqueued_subfolders: children.len(),
            listing_hash: plan.generated_listing_hash.clone(),
            outcome,
            completed_at: Utc::now(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::scan::context::{MovieFolderScanContext, MovieRootPath};
    use crate::domain::scan::orchestration::job::ScanReason;
    use crate::types::ids::LibraryId;

    fn movie_scan_job(library_root: &Path, movie_path: &Path) -> FolderScanJob {
        let library_root = library_root.to_string_lossy().to_string();
        let movie_path = movie_path.to_string_lossy().to_string();
        FolderScanJob {
            context: FolderScanContext::Movie(MovieFolderScanContext {
                library_id: LibraryId::new(),
                movie_root_path: MovieRootPath::try_new_under_library_root(
                    &library_root,
                    movie_path,
                )
                .expect("movie path under library root"),
            }),
            scan_reason: ScanReason::MaintenanceSweep,
            enqueue_time: Utc::now(),
            device_id: None,
        }
    }

    #[test]
    fn filter_policy_preserves_media_allow_and_ignore_lists() {
        let filters = ScannerFileFilterPolicy::new(
            vec![".MKV".to_string(), "mp4".to_string()],
            vec!["tmp".to_string()],
        );

        assert!(filters.is_media_file_path(Path::new("movie.mkv")));
        assert!(filters.is_media_file_path(Path::new("movie.MP4")));
        assert!(!filters.is_media_file_path(Path::new("movie.avi")));
        assert!(!filters.is_media_file_path(Path::new("partial.tmp")));
    }

    #[test]
    fn filter_policy_matches_ignored_path_patterns() {
        let filters = ScannerFileFilterPolicy::new_with_ignored_path_patterns(
            vec!["mkv".to_string()],
            Vec::<String>::new(),
            vec!["**/.staging/**".to_string(), "transcoding".to_string()],
        );

        assert!(
            filters.is_ignored_path(Path::new(
                "/media/Incoming/.staging/Movie.mkv"
            ))
        );
        assert!(filters.is_ignored_path(Path::new(
            "/media/Incoming/transcoding/Movie.mkv"
        )));
        assert!(!filters.is_ignored_path(Path::new("/media/Movies/Movie.mkv")));
    }

    #[tokio::test]
    async fn missing_folder_reconciles_as_deleted_plan() {
        let temp = tempfile::tempdir().expect("tempdir");
        let missing_movie = temp.path().join("Deleted Movie");
        let actor = DefaultFolderScanActor::new();

        let plan = actor
            .plan_listing(&movie_scan_job(temp.path(), &missing_movie))
            .await
            .expect("missing folders should not fail planning");

        assert!(plan.folder_missing);
        assert!(plan.directories.is_empty());
        assert!(plan.media_files.is_empty());
        assert_eq!(plan.generated_listing_hash, compute_listing_hash(&[]));
    }

    #[tokio::test]
    async fn file_at_folder_path_reconciles_as_deleted_plan() {
        let temp = tempfile::tempdir().expect("tempdir");
        let movie_path = temp.path().join("Not A Directory");
        tokio::fs::write(&movie_path, b"not a directory")
            .await
            .expect("create file at scan path");
        let actor = DefaultFolderScanActor::new();

        let plan = actor
            .plan_listing(&movie_scan_job(temp.path(), &movie_path))
            .await
            .expect("non-directory paths should not fail planning");

        assert!(plan.folder_missing);
        assert!(plan.directories.is_empty());
        assert!(plan.media_files.is_empty());
        assert_eq!(plan.generated_listing_hash, compute_listing_hash(&[]));
    }
}
