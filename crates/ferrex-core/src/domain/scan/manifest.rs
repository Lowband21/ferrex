//! Manifest scanner domain contracts.
//!
//! This module defines the layout contract that future manifest walkers,
//! reconciliation jobs, and UI diagnostics share.  It deliberately stays in the
//! domain layer: the types below model scopes, entries, classifications,
//! fingerprints, diagnostics, and run/reconciliation status without depending on
//! Postgres tables or repository traits.
//!
//! ## Movies layout contract
//!
//! Supported:
//! - video files directly under a Movies library root (`/Movies/Alien.mkv`)
//! - one video file or a small set of related video files directly inside a
//!   movie folder (`/Movies/Alien (1979)/Alien.mkv`)
//!
//! Reported as diagnostics:
//! - nested movie folders and video files below nested folders
//! - recognized extras folders such as `Extras`, `Trailers`, `Featurettes`, or
//!   `Deleted Scenes`
//!
//! ## Series layout contract
//!
//! Supported:
//! - top-level series folders under a Series library root
//! - season folders directly under a series folder, including `Specials`
//! - episode files directly inside a season folder
//! - parseable episode files directly inside a series folder
//!
//! Reported as diagnostics:
//! - video files directly under the Series library root
//! - direct series-root episode files whose names do not parse to season/episode
//! - nested folders below season folders
//! - recognized extras folders
//!
//! Hidden/system paths and configured ignored extensions/patterns are classified
//! as ignored entries with stable diagnostic codes so operators can distinguish
//! deliberate filtering from unsupported media layouts.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::domain::media::tv_parser::TvParser;
use crate::types::ids::LibraryId;
use crate::types::library::LibraryType;

/// Stable identifier for a configured library root in a manifest scan.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct ManifestRootId(pub u16);

/// Stable identifier for a bounded manifest partition within a root.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct ManifestPartitionId(pub u16);

/// Scope covered by a manifest run or entry set.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum ManifestScope {
    /// A complete library root.
    Root(ManifestRootScope),
    /// A bounded partition/prefix of one library root.
    Partition(ManifestPartitionScope),
}

impl ManifestScope {
    /// Library that owns this scope.
    pub fn library_id(&self) -> LibraryId {
        match self {
            ManifestScope::Root(scope) => scope.library_id,
            ManifestScope::Partition(scope) => scope.root.library_id,
        }
    }

    /// Library type that drives layout classification.
    pub fn library_type(&self) -> LibraryType {
        match self {
            ManifestScope::Root(scope) => scope.library_type,
            ManifestScope::Partition(scope) => scope.root.library_type,
        }
    }

    /// Normalized filesystem path for the configured root.
    pub fn root_path_norm(&self) -> &str {
        match self {
            ManifestScope::Root(scope) => scope.root_path_norm.as_str(),
            ManifestScope::Partition(scope) => {
                scope.root.root_path_norm.as_str()
            }
        }
    }
}

/// A complete library-root manifest scope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManifestRootScope {
    pub library_id: LibraryId,
    pub library_type: LibraryType,
    pub root_id: ManifestRootId,
    pub root_path_norm: String,
}

/// A bounded partition inside a root manifest scope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManifestPartitionScope {
    pub root: ManifestRootScope,
    pub partition_id: ManifestPartitionId,
    /// Optional normalized prefix covered by this partition. `None` means the
    /// partition key is synthetic rather than tied to a single path prefix.
    pub prefix_norm: Option<String>,
}

/// Filesystem entry kind observed by a manifest walker.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestEntryKind {
    File,
    Directory,
}

/// Scanner fingerprint data that can be computed without database state.
#[derive(
    Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize,
)]
pub struct ManifestFingerprint {
    pub device_id: Option<u64>,
    pub inode: Option<u64>,
    pub size: u64,
    /// Milliseconds since the Unix epoch when available.
    pub mtime_ms: Option<i64>,
    /// Optional weak content hash for filesystems where inode/mtime are not
    /// stable enough to identify moves.
    pub weak_hash: Option<String>,
}

/// One file entry in a manifest run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManifestMediaEntry {
    pub scope: ManifestScope,
    pub path_norm: String,
    pub relative_path: String,
    pub fingerprint: ManifestFingerprint,
    pub classification: ManifestEntryClassification,
    #[serde(default)]
    pub diagnostics: Vec<ManifestDiagnostic>,
}

/// One directory entry in a manifest run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManifestDirectoryEntry {
    pub scope: ManifestScope,
    pub path_norm: String,
    pub relative_path: String,
    pub classification: ManifestEntryClassification,
    #[serde(default)]
    pub diagnostics: Vec<ManifestDiagnostic>,
}

/// File or directory entry observed by a manifest run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "entry", rename_all = "snake_case")]
pub enum ManifestEntry {
    Media(ManifestMediaEntry),
    Directory(ManifestDirectoryEntry),
}

/// Classification assigned to a manifest entry.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", content = "classification", rename_all = "snake_case")]
pub enum ManifestEntryClassification {
    /// Entry participates in media discovery/reconciliation.
    Supported(ManifestSupportedClassification),
    /// Entry is deliberately filtered and does not require remediation.
    Ignored(ManifestDiagnosticReason),
    /// Entry is visible to operators as an unsupported layout or invalid media
    /// naming decision.
    Unsupported(ManifestDiagnosticReason),
}

impl ManifestEntryClassification {
    /// Whether this entry should proceed to media reconciliation.
    pub fn is_supported(&self) -> bool {
        matches!(self, Self::Supported(_))
    }

    /// Diagnostic reason attached to ignored or unsupported entries.
    pub fn diagnostic_reason(&self) -> Option<ManifestDiagnosticReason> {
        match self {
            Self::Supported(_) => None,
            Self::Ignored(reason) | Self::Unsupported(reason) => Some(*reason),
        }
    }
}

/// Supported layout decisions for Movies and Series libraries.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestSupportedClassification {
    LibraryRoot,
    MovieRootMedia,
    MovieFolder,
    MovieFolderMedia,
    SeriesRoot,
    SeasonFolder {
        season_number: u16,
        specials: bool,
    },
    SeasonEpisode {
        season_number: u16,
        episode_number: u16,
        specials: bool,
    },
    DirectSeriesRootEpisode {
        season_number: u16,
        episode_number: u16,
        specials: bool,
    },
}

/// Severity for manifest diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestDiagnosticSeverity {
    Info,
    Warning,
    Error,
}

/// Stable diagnostic reasons emitted by layout classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestDiagnosticReason {
    HiddenOrSystemPath,
    IgnoredExtension,
    IgnoredPathPattern,
    NonMediaFile,
    PathOutsideRoot,
    MovieNestedFolderUnsupported,
    MovieExtrasUnsupported,
    SeriesLibraryRootMediaUnsupported,
    SeriesDirectEpisodeParseFailed,
    SeriesEpisodeParseFailed,
    SeriesSeasonMismatch,
    SeriesNestedFolderUnsupported,
    SeriesExtrasUnsupported,
    UnsupportedLayout,
}

impl ManifestDiagnosticReason {
    /// Stable machine-readable diagnostic code for API/UI surfaces.
    pub const fn code(self) -> &'static str {
        match self {
            Self::HiddenOrSystemPath => "scanner.layout.hidden_system_path",
            Self::IgnoredExtension => "scanner.layout.ignored_extension",
            Self::IgnoredPathPattern => "scanner.layout.ignored_path_pattern",
            Self::NonMediaFile => "scanner.layout.non_media_file",
            Self::PathOutsideRoot => "scanner.layout.path_outside_root",
            Self::MovieNestedFolderUnsupported => {
                "scanner.layout.movie_nested_folder_unsupported"
            }
            Self::MovieExtrasUnsupported => {
                "scanner.layout.movie_extras_unsupported"
            }
            Self::SeriesLibraryRootMediaUnsupported => {
                "scanner.layout.series_library_root_media_unsupported"
            }
            Self::SeriesDirectEpisodeParseFailed => {
                "scanner.layout.series_direct_episode_parse_failed"
            }
            Self::SeriesEpisodeParseFailed => {
                "scanner.layout.series_episode_parse_failed"
            }
            Self::SeriesSeasonMismatch => {
                "scanner.layout.series_season_mismatch"
            }
            Self::SeriesNestedFolderUnsupported => {
                "scanner.layout.series_nested_folder_unsupported"
            }
            Self::SeriesExtrasUnsupported => {
                "scanner.layout.series_extras_unsupported"
            }
            Self::UnsupportedLayout => "scanner.layout.unsupported_layout",
        }
    }

    /// Human remediation text suitable for UI/operator diagnostics.
    pub const fn remediation(self) -> &'static str {
        match self {
            Self::HiddenOrSystemPath => {
                "No action is required. Hidden and system paths are skipped by design."
            }
            Self::IgnoredExtension => {
                "No action is required unless this should be scanned; remove the extension from scanner ignored_extensions."
            }
            Self::IgnoredPathPattern => {
                "No action is required unless this should be scanned; remove or narrow the matching scanner ignored_path_patterns entry."
            }
            Self::NonMediaFile => {
                "No action is required. Only configured video extensions are media candidates."
            }
            Self::PathOutsideRoot => {
                "Move the path under one of the configured library roots or update the library root configuration."
            }
            Self::MovieNestedFolderUnsupported => {
                "Move the primary movie video directly under the movie folder or directly under the Movies root."
            }
            Self::MovieExtrasUnsupported => {
                "Movie extras are recorded as diagnostics for now; keep the primary movie video in the movie folder or Movies root."
            }
            Self::SeriesLibraryRootMediaUnsupported => {
                "Place episode files inside a series folder, optionally under a Season folder."
            }
            Self::SeriesDirectEpisodeParseFailed => {
                "Rename the file with an episode pattern such as S01E01 or move it into an appropriate Season folder."
            }
            Self::SeriesEpisodeParseFailed => {
                "Rename the episode file with a parseable episode number, for example S01E01 or 01 - Title inside a Season folder."
            }
            Self::SeriesSeasonMismatch => {
                "Move the episode to the matching Season folder or rename the file so its season number matches the folder."
            }
            Self::SeriesNestedFolderUnsupported => {
                "Move episode files directly into the series folder or directly into a Season/Specials folder."
            }
            Self::SeriesExtrasUnsupported => {
                "Series extras are recorded as diagnostics for now; keep playable episodes in the series or Season/Specials folder."
            }
            Self::UnsupportedLayout => {
                "Restructure the path to one of the documented Movies or Series scanner layouts."
            }
        }
    }

    /// Default severity for this diagnostic reason.
    pub const fn severity(self) -> ManifestDiagnosticSeverity {
        match self {
            Self::HiddenOrSystemPath
            | Self::IgnoredExtension
            | Self::IgnoredPathPattern
            | Self::NonMediaFile => ManifestDiagnosticSeverity::Info,
            Self::PathOutsideRoot => ManifestDiagnosticSeverity::Error,
            Self::MovieNestedFolderUnsupported
            | Self::MovieExtrasUnsupported
            | Self::SeriesLibraryRootMediaUnsupported
            | Self::SeriesDirectEpisodeParseFailed
            | Self::SeriesEpisodeParseFailed
            | Self::SeriesSeasonMismatch
            | Self::SeriesNestedFolderUnsupported
            | Self::SeriesExtrasUnsupported
            | Self::UnsupportedLayout => ManifestDiagnosticSeverity::Warning,
        }
    }
}

/// Operator/UI diagnostic emitted by manifest classification or reconciliation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManifestDiagnostic {
    pub path_norm: String,
    pub reason: ManifestDiagnosticReason,
    pub code: String,
    pub severity: ManifestDiagnosticSeverity,
    pub remediation: String,
}

impl ManifestDiagnostic {
    /// Build a diagnostic from a stable reason.
    pub fn new(
        path_norm: impl Into<String>,
        reason: ManifestDiagnosticReason,
    ) -> Self {
        Self {
            path_norm: path_norm.into(),
            reason,
            code: reason.code().to_string(),
            severity: reason.severity(),
            remediation: reason.remediation().to_string(),
        }
    }
}

/// Result returned by the layout classifier.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManifestClassificationResult {
    pub classification: ManifestEntryClassification,
    #[serde(default)]
    pub diagnostics: Vec<ManifestDiagnostic>,
}

impl ManifestClassificationResult {
    fn supported(classification: ManifestSupportedClassification) -> Self {
        Self {
            classification: ManifestEntryClassification::Supported(
                classification,
            ),
            diagnostics: Vec::new(),
        }
    }

    fn ignored(path_norm: &str, reason: ManifestDiagnosticReason) -> Self {
        Self {
            classification: ManifestEntryClassification::Ignored(reason),
            diagnostics: vec![ManifestDiagnostic::new(path_norm, reason)],
        }
    }

    fn unsupported(path_norm: &str, reason: ManifestDiagnosticReason) -> Self {
        Self {
            classification: ManifestEntryClassification::Unsupported(reason),
            diagnostics: vec![ManifestDiagnostic::new(path_norm, reason)],
        }
    }
}

/// Manifest run lifecycle independent from persistence implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestRunStatus {
    Pending,
    Running,
    Completed,
    CompletedWithDiagnostics,
    Failed,
    Canceled,
    Stalled,
}

/// Domain summary for a manifest run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManifestRun {
    pub run_id: Uuid,
    pub scope: ManifestScope,
    pub status: ManifestRunStatus,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub entries_seen: u64,
    pub diagnostics_seen: u64,
}

/// Filesystem layout contract and filter policy shared by manifest scanners.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScannerLayoutContract {
    media_extensions: BTreeSet<String>,
    ignored_extensions: BTreeSet<String>,
    ignored_path_patterns: Vec<String>,
}

impl Default for ScannerLayoutContract {
    fn default() -> Self {
        Self::new(
            crate::domain::scan::scanner::settings::default_video_file_extensions_vec(),
            Vec::<String>::new(),
            Vec::<String>::new(),
        )
    }
}

impl ScannerLayoutContract {
    /// Build a contract from configured media/ignore filters.
    pub fn new(
        media_extensions: impl IntoIterator<Item = String>,
        ignored_extensions: impl IntoIterator<Item = String>,
        ignored_path_patterns: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            media_extensions: media_extensions
                .into_iter()
                .filter_map(|ext| normalize_extension(&ext))
                .collect(),
            ignored_extensions: ignored_extensions
                .into_iter()
                .filter_map(|ext| normalize_extension(&ext))
                .collect(),
            ignored_path_patterns: ignored_path_patterns
                .into_iter()
                .map(|pattern| pattern.trim().replace('\\', "/"))
                .filter(|pattern| !pattern.is_empty())
                .collect(),
        }
    }

    /// Configured media extensions in normalized lower-case form.
    pub fn media_extensions(&self) -> impl Iterator<Item = &str> {
        self.media_extensions.iter().map(String::as_str)
    }

    /// Configured ignored extensions in normalized lower-case form.
    pub fn ignored_extensions(&self) -> impl Iterator<Item = &str> {
        self.ignored_extensions.iter().map(String::as_str)
    }

    /// Configured ignored path patterns.
    pub fn ignored_path_patterns(&self) -> impl Iterator<Item = &str> {
        self.ignored_path_patterns.iter().map(String::as_str)
    }

    /// Classify a filesystem path under the supplied library root.
    pub fn classify_path(
        &self,
        library_type: LibraryType,
        root_path_norm: impl AsRef<Path>,
        path: impl AsRef<Path>,
        entry_kind: ManifestEntryKind,
    ) -> ManifestClassificationResult {
        let root = root_path_norm.as_ref();
        let path = path.as_ref();
        let path_norm = path_to_string(path);

        let Ok(relative) = path.strip_prefix(root) else {
            return ManifestClassificationResult::unsupported(
                &path_norm,
                ManifestDiagnosticReason::PathOutsideRoot,
            );
        };

        self.classify_relative_path(
            library_type,
            relative,
            path,
            entry_kind,
            &path_norm,
        )
    }

    /// Classify a path already known to be relative to a library root.
    pub fn classify_relative_path(
        &self,
        library_type: LibraryType,
        relative_path: impl AsRef<Path>,
        full_path_hint: impl AsRef<Path>,
        entry_kind: ManifestEntryKind,
        path_norm: &str,
    ) -> ManifestClassificationResult {
        let relative_path = relative_path.as_ref();
        let full_path_hint = full_path_hint.as_ref();
        let segments = path_segments(relative_path);

        if segments.is_empty() {
            return match entry_kind {
                ManifestEntryKind::Directory => {
                    ManifestClassificationResult::supported(
                        ManifestSupportedClassification::LibraryRoot,
                    )
                }
                ManifestEntryKind::File => {
                    ManifestClassificationResult::unsupported(
                        path_norm,
                        ManifestDiagnosticReason::UnsupportedLayout,
                    )
                }
            };
        }

        if self.matches_ignored_path_pattern(relative_path, full_path_hint) {
            return ManifestClassificationResult::ignored(
                path_norm,
                ManifestDiagnosticReason::IgnoredPathPattern,
            );
        }

        if contains_hidden_or_system_segment(&segments) {
            return ManifestClassificationResult::ignored(
                path_norm,
                ManifestDiagnosticReason::HiddenOrSystemPath,
            );
        }

        if entry_kind == ManifestEntryKind::File {
            let extension = full_path_hint
                .extension()
                .and_then(|ext| ext.to_str())
                .and_then(normalize_extension);

            if extension
                .as_ref()
                .is_some_and(|ext| self.ignored_extensions.contains(ext))
            {
                return ManifestClassificationResult::ignored(
                    path_norm,
                    ManifestDiagnosticReason::IgnoredExtension,
                );
            }

            if !extension
                .as_ref()
                .is_some_and(|ext| self.media_extensions.contains(ext))
            {
                return ManifestClassificationResult::ignored(
                    path_norm,
                    ManifestDiagnosticReason::NonMediaFile,
                );
            }
        }

        match library_type {
            LibraryType::Movies => {
                classify_movie_path(&segments, entry_kind, path_norm)
            }
            LibraryType::Series => classify_series_path(
                &segments,
                entry_kind,
                full_path_hint,
                path_norm,
            ),
        }
    }

    fn matches_ignored_path_pattern(
        &self,
        relative_path: &Path,
        full_path: &Path,
    ) -> bool {
        let relative = path_to_string(relative_path);
        let full = path_to_string(full_path);
        self.ignored_path_patterns.iter().any(|pattern| {
            path_pattern_matches(pattern, &relative)
                || path_pattern_matches(pattern, &full)
        })
    }
}

fn classify_movie_path(
    segments: &[String],
    entry_kind: ManifestEntryKind,
    path_norm: &str,
) -> ManifestClassificationResult {
    match entry_kind {
        ManifestEntryKind::Directory if segments.len() == 1 => {
            ManifestClassificationResult::supported(
                ManifestSupportedClassification::MovieFolder,
            )
        }
        ManifestEntryKind::File if segments.len() == 1 => {
            ManifestClassificationResult::supported(
                ManifestSupportedClassification::MovieRootMedia,
            )
        }
        ManifestEntryKind::File if segments.len() == 2 => {
            ManifestClassificationResult::supported(
                ManifestSupportedClassification::MovieFolderMedia,
            )
        }
        _ if contains_extras_segment(segments) => {
            ManifestClassificationResult::unsupported(
                path_norm,
                ManifestDiagnosticReason::MovieExtrasUnsupported,
            )
        }
        _ => ManifestClassificationResult::unsupported(
            path_norm,
            ManifestDiagnosticReason::MovieNestedFolderUnsupported,
        ),
    }
}

fn classify_series_path(
    segments: &[String],
    entry_kind: ManifestEntryKind,
    full_path_hint: &Path,
    path_norm: &str,
) -> ManifestClassificationResult {
    match (entry_kind, segments.len()) {
        (ManifestEntryKind::Directory, 1) => {
            ManifestClassificationResult::supported(
                ManifestSupportedClassification::SeriesRoot,
            )
        }
        (ManifestEntryKind::File, 1) => {
            ManifestClassificationResult::unsupported(
                path_norm,
                ManifestDiagnosticReason::SeriesLibraryRootMediaUnsupported,
            )
        }
        (ManifestEntryKind::Directory, 2) => {
            classify_series_child_directory(segments, path_norm)
        }
        (ManifestEntryKind::File, 2) => {
            classify_direct_series_episode(full_path_hint, path_norm)
        }
        (ManifestEntryKind::File, 3) => {
            classify_season_episode(segments, full_path_hint, path_norm)
        }
        _ if contains_extras_segment(segments) => {
            ManifestClassificationResult::unsupported(
                path_norm,
                ManifestDiagnosticReason::SeriesExtrasUnsupported,
            )
        }
        _ => ManifestClassificationResult::unsupported(
            path_norm,
            ManifestDiagnosticReason::SeriesNestedFolderUnsupported,
        ),
    }
}

fn classify_series_child_directory(
    segments: &[String],
    path_norm: &str,
) -> ManifestClassificationResult {
    let series_name = segments.first().map(String::as_str);
    let folder_name = segments.get(1).map(String::as_str).unwrap_or_default();

    if let Some(season_number) =
        TvParser::parse_season_folder_with_series(folder_name, series_name)
    {
        return ManifestClassificationResult::supported(
            ManifestSupportedClassification::SeasonFolder {
                season_number,
                specials: season_number == 0,
            },
        );
    }

    if is_extras_segment(folder_name) {
        ManifestClassificationResult::unsupported(
            path_norm,
            ManifestDiagnosticReason::SeriesExtrasUnsupported,
        )
    } else {
        ManifestClassificationResult::unsupported(
            path_norm,
            ManifestDiagnosticReason::SeriesNestedFolderUnsupported,
        )
    }
}

fn classify_direct_series_episode(
    full_path_hint: &Path,
    path_norm: &str,
) -> ManifestClassificationResult {
    let Some(info) = TvParser::parse_episode_info(full_path_hint) else {
        return ManifestClassificationResult::unsupported(
            path_norm,
            ManifestDiagnosticReason::SeriesDirectEpisodeParseFailed,
        );
    };

    ManifestClassificationResult::supported(
        ManifestSupportedClassification::DirectSeriesRootEpisode {
            season_number: info.season,
            episode_number: info.episode,
            specials: info.is_special,
        },
    )
}

fn classify_season_episode(
    segments: &[String],
    full_path_hint: &Path,
    path_norm: &str,
) -> ManifestClassificationResult {
    let series_name = segments.first().map(String::as_str);
    let season_folder_name =
        segments.get(1).map(String::as_str).unwrap_or_default();
    let Some(season_number) = TvParser::parse_season_folder_with_series(
        season_folder_name,
        series_name,
    ) else {
        return if contains_extras_segment(segments) {
            ManifestClassificationResult::unsupported(
                path_norm,
                ManifestDiagnosticReason::SeriesExtrasUnsupported,
            )
        } else {
            ManifestClassificationResult::unsupported(
                path_norm,
                ManifestDiagnosticReason::SeriesNestedFolderUnsupported,
            )
        };
    };

    let Some(info) = TvParser::parse_episode_info(full_path_hint) else {
        return ManifestClassificationResult::unsupported(
            path_norm,
            ManifestDiagnosticReason::SeriesEpisodeParseFailed,
        );
    };

    if info.season != season_number {
        return ManifestClassificationResult::unsupported(
            path_norm,
            ManifestDiagnosticReason::SeriesSeasonMismatch,
        );
    }

    ManifestClassificationResult::supported(
        ManifestSupportedClassification::SeasonEpisode {
            season_number,
            episode_number: info.episode,
            specials: season_number == 0 || info.is_special,
        },
    )
}

fn normalize_extension(ext: impl AsRef<str>) -> Option<String> {
    let normalized = ext
        .as_ref()
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase();
    (!normalized.is_empty()
        && !normalized.contains('/')
        && !normalized.contains('\\')
        && !normalized.contains('*'))
    .then_some(normalized)
}

fn path_segments(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => {
                Some(value.to_string_lossy().to_string())
            }
            _ => None,
        })
        .collect()
}

fn path_to_string(path: &Path) -> String {
    if path.as_os_str().is_empty() {
        String::new()
    } else {
        path.to_string_lossy().replace('\\', "/")
    }
}

fn contains_hidden_or_system_segment(segments: &[String]) -> bool {
    segments
        .iter()
        .any(|segment| is_hidden_or_system_segment(segment))
}

fn is_hidden_or_system_segment(segment: &str) -> bool {
    if segment.starts_with('.') {
        return true;
    }

    matches!(
        segment.to_ascii_lowercase().as_str(),
        "@eadir"
            | "#recycle"
            | "$recycle.bin"
            | "system volume information"
            | "thumbs.db"
            | "desktop.ini"
            | "ehthumbs.db"
            | "lost+found"
    )
}

fn contains_extras_segment(segments: &[String]) -> bool {
    segments.iter().any(|segment| is_extras_segment(segment))
}

fn is_extras_segment(segment: &str) -> bool {
    let normalized =
        segment.trim().to_ascii_lowercase().replace(['_', '-'], " ");
    matches!(
        normalized.as_str(),
        "extra"
            | "extras"
            | "bonus"
            | "bonus features"
            | "featurette"
            | "featurettes"
            | "trailer"
            | "trailers"
            | "deleted scenes"
            | "behind the scenes"
            | "interview"
            | "interviews"
            | "sample"
            | "samples"
            | "short"
            | "shorts"
            | "special feature"
            | "special features"
    )
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

/// Build an owned relative path string for entries.
pub fn manifest_relative_path(
    root_path_norm: impl AsRef<Path>,
    path: impl AsRef<Path>,
) -> Option<String> {
    path.as_ref()
        .strip_prefix(root_path_norm.as_ref())
        .ok()
        .map(path_to_string)
}

/// Helper for tests and future walkers that need owned paths.
pub fn join_manifest_path(
    root: impl AsRef<Path>,
    relative: impl AsRef<Path>,
) -> PathBuf {
    root.as_ref().join(relative.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract() -> ScannerLayoutContract {
        ScannerLayoutContract::new(
            vec!["mkv".to_string(), "mp4".to_string()],
            vec!["part".to_string()],
            vec!["**/.staging/**".to_string()],
        )
    }

    fn classify(
        library_type: LibraryType,
        relative: &str,
        entry_kind: ManifestEntryKind,
    ) -> ManifestClassificationResult {
        let root = Path::new("/media/root");
        contract().classify_path(
            library_type,
            root,
            root.join(relative),
            entry_kind,
        )
    }

    #[test]
    fn movies_contract_supports_flat_root_media_and_movie_folder_media() {
        assert_eq!(
            classify(
                LibraryType::Movies,
                "Alien.mkv",
                ManifestEntryKind::File,
            )
            .classification,
            ManifestEntryClassification::Supported(
                ManifestSupportedClassification::MovieRootMedia,
            ),
        );

        assert_eq!(
            classify(
                LibraryType::Movies,
                "Alien (1979)",
                ManifestEntryKind::Directory,
            )
            .classification,
            ManifestEntryClassification::Supported(
                ManifestSupportedClassification::MovieFolder,
            ),
        );

        assert_eq!(
            classify(
                LibraryType::Movies,
                "Alien (1979)/Alien.mkv",
                ManifestEntryKind::File,
            )
            .classification,
            ManifestEntryClassification::Supported(
                ManifestSupportedClassification::MovieFolderMedia,
            ),
        );
    }

    #[test]
    fn series_contract_supports_seasons_specials_and_direct_episodes() {
        assert_eq!(
            classify(
                LibraryType::Series,
                "Fringe",
                ManifestEntryKind::Directory,
            )
            .classification,
            ManifestEntryClassification::Supported(
                ManifestSupportedClassification::SeriesRoot,
            ),
        );

        assert_eq!(
            classify(
                LibraryType::Series,
                "Fringe/Season 02",
                ManifestEntryKind::Directory,
            )
            .classification,
            ManifestEntryClassification::Supported(
                ManifestSupportedClassification::SeasonFolder {
                    season_number: 2,
                    specials: false,
                },
            ),
        );

        assert_eq!(
            classify(
                LibraryType::Series,
                "Fringe/Specials/S00E01 - Unearthed.mkv",
                ManifestEntryKind::File,
            )
            .classification,
            ManifestEntryClassification::Supported(
                ManifestSupportedClassification::SeasonEpisode {
                    season_number: 0,
                    episode_number: 1,
                    specials: true,
                },
            ),
        );

        assert_eq!(
            classify(
                LibraryType::Series,
                "Fringe/S01E01 - Pilot.mkv",
                ManifestEntryKind::File,
            )
            .classification,
            ManifestEntryClassification::Supported(
                ManifestSupportedClassification::DirectSeriesRootEpisode {
                    season_number: 1,
                    episode_number: 1,
                    specials: false,
                },
            ),
        );
    }

    #[test]
    fn unsupported_layouts_return_stable_diagnostics() {
        let movie_extra = classify(
            LibraryType::Movies,
            "Alien (1979)/Extras/Trailer.mkv",
            ManifestEntryKind::File,
        );
        assert_eq!(
            movie_extra.classification,
            ManifestEntryClassification::Unsupported(
                ManifestDiagnosticReason::MovieExtrasUnsupported,
            ),
        );
        assert_eq!(
            movie_extra.diagnostics[0].code,
            "scanner.layout.movie_extras_unsupported",
        );
        assert!(
            movie_extra.diagnostics[0]
                .remediation
                .contains("primary movie")
        );

        let nested_series = classify(
            LibraryType::Series,
            "Fringe/Season 01/CD1/S01E01.mkv",
            ManifestEntryKind::File,
        );
        assert_eq!(
            nested_series.classification,
            ManifestEntryClassification::Unsupported(
                ManifestDiagnosticReason::SeriesNestedFolderUnsupported,
            ),
        );
        assert_eq!(
            nested_series.diagnostics[0].code,
            "scanner.layout.series_nested_folder_unsupported",
        );

        let flat_series_root = classify(
            LibraryType::Series,
            "S01E01 - Unknown Show.mkv",
            ManifestEntryKind::File,
        );
        assert_eq!(
            flat_series_root.classification,
            ManifestEntryClassification::Unsupported(
                ManifestDiagnosticReason::SeriesLibraryRootMediaUnsupported,
            ),
        );
    }

    #[test]
    fn hidden_paths_ignored_extensions_and_patterns_are_classified() {
        let hidden = classify(
            LibraryType::Movies,
            ".scanner/cache.mkv",
            ManifestEntryKind::File,
        );
        assert_eq!(
            hidden.classification,
            ManifestEntryClassification::Ignored(
                ManifestDiagnosticReason::HiddenOrSystemPath,
            ),
        );

        let ignored_ext = classify(
            LibraryType::Movies,
            "Alien (1979)/Alien.part",
            ManifestEntryKind::File,
        );
        assert_eq!(
            ignored_ext.classification,
            ManifestEntryClassification::Ignored(
                ManifestDiagnosticReason::IgnoredExtension,
            ),
        );

        let ignored_pattern = classify(
            LibraryType::Movies,
            ".staging/Alien.mkv",
            ManifestEntryKind::File,
        );
        assert_eq!(
            ignored_pattern.classification,
            ManifestEntryClassification::Ignored(
                ManifestDiagnosticReason::IgnoredPathPattern,
            ),
            "explicit ignored_path_patterns win when they match hidden work directories",
        );

        let pattern_only = classify(
            LibraryType::Movies,
            "Incoming/.staging/Alien.mkv",
            ManifestEntryKind::File,
        );
        assert_eq!(
            pattern_only.classification,
            ManifestEntryClassification::Ignored(
                ManifestDiagnosticReason::IgnoredPathPattern,
            ),
            "explicit ignored_path_patterns win when they match hidden work directories",
        );
    }

    #[test]
    fn ignored_path_patterns_cover_non_hidden_paths() {
        let contract = ScannerLayoutContract::new(
            vec!["mkv".to_string()],
            Vec::<String>::new(),
            vec!["**/transcoding/**".to_string()],
        );
        let root = Path::new("/media/root");
        let result = contract.classify_path(
            LibraryType::Movies,
            root,
            root.join("Incoming/transcoding/Alien.mkv"),
            ManifestEntryKind::File,
        );

        assert_eq!(
            result.classification,
            ManifestEntryClassification::Ignored(
                ManifestDiagnosticReason::IgnoredPathPattern,
            ),
        );
        assert_eq!(
            result.diagnostics[0].code,
            "scanner.layout.ignored_path_pattern",
        );
    }

    #[test]
    fn series_episode_parse_failures_have_remediation() {
        let result = classify(
            LibraryType::Series,
            "Fringe/Pilot.mkv",
            ManifestEntryKind::File,
        );

        assert_eq!(
            result.classification,
            ManifestEntryClassification::Unsupported(
                ManifestDiagnosticReason::SeriesDirectEpisodeParseFailed,
            ),
        );
        assert!(result.diagnostics[0].remediation.contains("S01E01"));
    }
}
