//! Bounded subtitle discovery, extraction, and parser utilities.
//!
//! This module intentionally keeps local filesystem paths and raw command
//! stderr out of public status/debug payloads. Source identity is represented by
//! deterministic hashes and stream indexes so callers can persist transcript
//! upserts without exposing user paths in logs or API surfaces.

use std::{
    collections::HashMap,
    fmt,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use ferrex_model::{LibraryId, MediaID};
use regex::{Captures, Regex};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::{
    fs,
    io::{self, AsyncRead, AsyncReadExt},
    process::Command,
    time,
};
use uuid::Uuid;

use crate::{
    api::types::intelligence::TimedTextSourceKind,
    database::repository_ports::transcripts::{
        TranscriptSegmentUpsert, TranscriptSourceUpsert,
    },
    error::{MediaError, Result},
};

const DEFAULT_LANGUAGE: &str = "und";
const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_CONVERT_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_MAX_SIDECAR_BYTES: usize = 4 * 1024 * 1024;
const DEFAULT_MAX_PROBE_BYTES: usize = 512 * 1024;
const DEFAULT_MAX_CONVERTED_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_MAX_CUES_PER_SOURCE: usize = 20_000;
const DEFAULT_MAX_SOURCES_PER_MEDIA: usize = 32;
const DEFAULT_MAX_TEXT_CHARS_PER_CUE: usize = 4_000;
const STDERR_CAP_BYTES: usize = 16 * 1024;

/// Supported subtitle text container/parser formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleFormat {
    /// SubRip (`.srt`).
    Srt,
    /// WebVTT (`.vtt`/`.webvtt`).
    WebVtt,
}

impl SubtitleFormat {
    fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "srt" => Some(Self::Srt),
            "vtt" | "webvtt" => Some(Self::WebVtt),
            _ => None,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            SubtitleFormat::Srt => "srt",
            SubtitleFormat::WebVtt => "webvtt",
        }
    }
}

/// A parsed subtitle cue after timestamp conversion and text normalization.
#[derive(Clone, PartialEq, Eq)]
pub struct ParsedSubtitleCue {
    /// Zero-based cue index in the normalized parsed output.
    pub cue_index: i32,
    /// Cue start timestamp in milliseconds.
    pub start_ms: i64,
    /// Cue end timestamp in milliseconds.
    pub end_ms: i64,
    /// Normalized cue text. This may still contain private user content and is
    /// intentionally omitted from `Debug` output.
    pub text: String,
}

impl fmt::Debug for ParsedSubtitleCue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParsedSubtitleCue")
            .field("cue_index", &self.cue_index)
            .field("start_ms", &self.start_ms)
            .field("end_ms", &self.end_ms)
            .field("text_chars", &self.text.chars().count())
            .field("text_hash", &sha256_hex(self.text.as_bytes()))
            .finish()
    }
}

/// Safe parser warning category for malformed cue blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleParseWarningKind {
    /// The block did not contain a recognizable `-->` timing line.
    MissingTiming,
    /// A timing line was present but one or both timestamps were invalid.
    InvalidTiming,
    /// The cue had no usable normalized text.
    EmptyText,
}

/// Safe parser warning. It never stores raw cue text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleParseWarning {
    /// One-based cue block ordinal in the source text.
    pub block_ordinal: usize,
    /// Warning category.
    pub kind: SubtitleParseWarningKind,
}

/// Parser result containing usable cues plus safe malformed-cue counters.
#[derive(Clone, PartialEq, Eq)]
pub struct SubtitleParseReport {
    /// Parsed cues in source order.
    pub cues: Vec<ParsedSubtitleCue>,
    /// Safe warnings for malformed or empty cue blocks.
    pub warnings: Vec<SubtitleParseWarning>,
}

impl SubtitleParseReport {
    fn new() -> Self {
        Self {
            cues: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn push_warning(
        &mut self,
        block_ordinal: usize,
        kind: SubtitleParseWarningKind,
    ) {
        self.warnings.push(SubtitleParseWarning {
            block_ordinal,
            kind,
        });
    }

    /// Count of malformed/empty cue blocks skipped by the parser.
    pub fn skipped_count(&self) -> usize {
        self.warnings.len()
    }
}

impl fmt::Debug for SubtitleParseReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubtitleParseReport")
            .field("cue_count", &self.cues.len())
            .field("warnings", &self.warnings)
            .finish()
    }
}

/// Hook used to redact normalized cue text before transcript segment upserts are
/// emitted. Implementations should be deterministic.
pub trait TimedTextRedactor: Send + Sync {
    /// Return the text that is safe to store in transcript segment rows.
    fn redact(&self, normalized_text: &str) -> String;
}

/// Redactor that intentionally leaves text untouched for parser/extractor
/// tests. Production config uses [`PrivacyTimedTextRedactor`] by default.
#[cfg(test)]
#[derive(Debug, Default)]
struct NoopTimedTextRedactor;

#[cfg(test)]
impl TimedTextRedactor for NoopTimedTextRedactor {
    fn redact(&self, normalized_text: &str) -> String {
        normalized_text.to_string()
    }
}

/// Deterministic built-in redactor for transcript text persisted by Ferrex.
/// It replaces common personal/contact data and credential/token patterns with
/// stable labels so search can still match surrounding context without storing
/// the secret value.
#[derive(Debug, Clone)]
pub struct PrivacyTimedTextRedactor {
    email: Option<Regex>,
    phone: Option<Regex>,
    url_secret: Option<Regex>,
    bearer: Option<Regex>,
    token_assignment: Option<Regex>,
    custom: Vec<Regex>,
}

impl PrivacyTimedTextRedactor {
    /// Build a redactor from the shared scanner transcript redaction config.
    pub fn from_config(
        config: &crate::types::scan::orchestration::config::TranscriptRedactionConfig,
    ) -> Result<Self> {
        if !config.enabled {
            return Ok(Self::disabled());
        }

        let custom = config
            .custom_regexes
            .iter()
            .enumerate()
            .map(|(idx, pattern)| {
                Regex::new(pattern).map_err(|err| {
                    MediaError::InvalidMedia(format!(
                        "transcript redaction custom_regexes[{idx}] is invalid: {err}"
                    ))
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            email: regex_if(
                config.redact_emails,
                r"(?i)\b[A-Z0-9._%+\-]+@[A-Z0-9.\-]+\.[A-Z]{2,}\b",
            )?,
            phone: regex_if(
                config.redact_phone_numbers,
                r"\b\+?\d[\d\s().\-]{5,}\d\b",
            )?,
            url_secret: regex_if(
                config.redact_url_secrets,
                r"(?i)([?&](?:access[_-]?token|refresh[_-]?token|id[_-]?token|token|api[_-]?key|key|secret|sig|signature|auth|password|pass|code)=)([^\s&#]+)",
            )?,
            bearer: regex_if(
                config.redact_bearer_tokens,
                r"(?i)\b(bearer\s+)([A-Za-z0-9._~+/=\-]{8,})",
            )?,
            token_assignment: regex_if(
                config.redact_bearer_tokens,
                r#"(?i)\b((?:api[_-]?key|access[_-]?token|refresh[_-]?token|id[_-]?token|token|secret|password)\s*[:=]\s*["']?)([A-Za-z0-9._~+/=\-]{8,})(["']?)"#,
            )?,
            custom,
        })
    }

    fn disabled() -> Self {
        Self {
            email: None,
            phone: None,
            url_secret: None,
            bearer: None,
            token_assignment: None,
            custom: Vec::new(),
        }
    }
}

impl Default for PrivacyTimedTextRedactor {
    fn default() -> Self {
        Self::from_config(
            &crate::types::scan::orchestration::config::TranscriptRedactionConfig::default(),
        )
        .expect("built-in transcript redaction patterns compile")
    }
}

impl TimedTextRedactor for PrivacyTimedTextRedactor {
    fn redact(&self, normalized_text: &str) -> String {
        let mut redacted = normalized_text.to_string();

        if let Some(email) = &self.email {
            redacted = email
                .replace_all(&redacted, "[REDACTED:email]")
                .into_owned();
        }

        if let Some(phone) = &self.phone {
            redacted = phone
                .replace_all(&redacted, |caps: &Captures<'_>| {
                    let value = caps.get(0).map_or("", |m| m.as_str());
                    let digits =
                        value.chars().filter(|ch| ch.is_ascii_digit()).count();
                    if digits >= 7 {
                        "[REDACTED:phone]".to_string()
                    } else {
                        value.to_string()
                    }
                })
                .into_owned();
        }

        if let Some(url_secret) = &self.url_secret {
            redacted = url_secret
                .replace_all(&redacted, |caps: &Captures<'_>| {
                    format!(
                        "{}[REDACTED:url_secret]",
                        caps.get(1).map_or("", |m| m.as_str())
                    )
                })
                .into_owned();
        }

        if let Some(bearer) = &self.bearer {
            redacted = bearer
                .replace_all(&redacted, |caps: &Captures<'_>| {
                    format!(
                        "{}[REDACTED:token]",
                        caps.get(1).map_or("", |m| m.as_str())
                    )
                })
                .into_owned();
        }

        if let Some(token_assignment) = &self.token_assignment {
            redacted = token_assignment
                .replace_all(&redacted, |caps: &Captures<'_>| {
                    format!(
                        "{}[REDACTED:token]{}",
                        caps.get(1).map_or("", |m| m.as_str()),
                        caps.get(3).map_or("", |m| m.as_str())
                    )
                })
                .into_owned();
        }

        for custom in &self.custom {
            redacted = custom
                .replace_all(&redacted, "[REDACTED:custom]")
                .into_owned();
        }

        redacted
    }
}

fn regex_if(enabled: bool, pattern: &str) -> Result<Option<Regex>> {
    if enabled {
        Regex::new(pattern).map(Some).map_err(|err| {
            MediaError::Internal(format!(
                "built-in transcript redaction pattern failed to compile: {err}"
            ))
        })
    } else {
        Ok(None)
    }
}

/// Runtime bounds and binary paths for subtitle extraction.
#[derive(Clone)]
pub struct TimedTextExtractionConfig {
    /// Configured `ffprobe` binary path.
    pub ffprobe_path: PathBuf,
    /// Configured `ffmpeg` binary path.
    pub ffmpeg_path: PathBuf,
    /// Extract text-convertible embedded subtitle streams.
    pub extract_embedded: bool,
    /// Extract sibling `.srt`/`.vtt` sidecar files.
    pub extract_sidecars: bool,
    /// Optional normalized language allow-list. Empty means all languages.
    pub allowed_languages: Vec<String>,
    /// Timeout for embedded stream enumeration.
    pub probe_timeout: Duration,
    /// Timeout for one embedded stream conversion.
    pub convert_timeout: Duration,
    /// Maximum bytes read from a sidecar subtitle file.
    pub max_sidecar_bytes: usize,
    /// Maximum bytes accepted from `ffprobe` stdout.
    pub max_probe_json_bytes: usize,
    /// Maximum bytes accepted from one converted subtitle stdout.
    pub max_converted_subtitle_bytes: usize,
    /// Maximum cues emitted for one source.
    pub max_cues_per_source: usize,
    /// Maximum transcript segments emitted across all sources for one media.
    pub max_segments_per_media: usize,
    /// Maximum sources processed for one media file.
    pub max_sources_per_media: usize,
    /// Maximum stored characters for a redacted cue.
    pub max_text_chars_per_cue: usize,
    /// Deterministic redaction hook applied after normalization.
    pub redactor: Arc<dyn TimedTextRedactor>,
}

impl fmt::Debug for TimedTextExtractionConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TimedTextExtractionConfig")
            .field("ffprobe_path", &"<configured>")
            .field("ffmpeg_path", &"<configured>")
            .field("extract_embedded", &self.extract_embedded)
            .field("extract_sidecars", &self.extract_sidecars)
            .field("allowed_languages", &self.allowed_languages)
            .field("probe_timeout", &self.probe_timeout)
            .field("convert_timeout", &self.convert_timeout)
            .field("max_sidecar_bytes", &self.max_sidecar_bytes)
            .field("max_probe_json_bytes", &self.max_probe_json_bytes)
            .field(
                "max_converted_subtitle_bytes",
                &self.max_converted_subtitle_bytes,
            )
            .field("max_cues_per_source", &self.max_cues_per_source)
            .field("max_segments_per_media", &self.max_segments_per_media)
            .field("max_sources_per_media", &self.max_sources_per_media)
            .field("max_text_chars_per_cue", &self.max_text_chars_per_cue)
            .field("redactor", &"<redactor>")
            .finish()
    }
}

impl Default for TimedTextExtractionConfig {
    fn default() -> Self {
        Self {
            ffprobe_path: PathBuf::from("ffprobe"),
            ffmpeg_path: PathBuf::from("ffmpeg"),
            extract_embedded: true,
            extract_sidecars: true,
            allowed_languages: Vec::new(),
            probe_timeout: DEFAULT_PROBE_TIMEOUT,
            convert_timeout: DEFAULT_CONVERT_TIMEOUT,
            max_sidecar_bytes: DEFAULT_MAX_SIDECAR_BYTES,
            max_probe_json_bytes: DEFAULT_MAX_PROBE_BYTES,
            max_converted_subtitle_bytes: DEFAULT_MAX_CONVERTED_BYTES,
            max_cues_per_source: DEFAULT_MAX_CUES_PER_SOURCE,
            max_segments_per_media: DEFAULT_MAX_CUES_PER_SOURCE,
            max_sources_per_media: DEFAULT_MAX_SOURCES_PER_MEDIA,
            max_text_chars_per_cue: DEFAULT_MAX_TEXT_CHARS_PER_CUE,
            redactor: Arc::new(PrivacyTimedTextRedactor::default()),
        }
    }
}

impl TimedTextExtractionConfig {
    /// Build extractor runtime settings from the shared scanner config while
    /// keeping binary paths at their runtime defaults. Server wiring may still
    /// override `ffprobe_path`/`ffmpeg_path` separately.
    pub fn from_indexing_config(
        config: &crate::types::scan::orchestration::config::TranscriptIndexingConfig,
    ) -> Result<Self> {
        let mut extraction = Self::default();
        let timeout =
            Duration::from_millis(config.extraction_timeout_ms.max(1));
        let subtitle_bytes = config.max_subtitle_bytes.max(1);
        let max_segments = config.max_segments_per_media.max(1);

        extraction.extract_embedded = config.embedded_enabled;
        extraction.extract_sidecars = config.sidecar_enabled;
        extraction.allowed_languages = normalize_language_allow_list(
            config.allowed_languages.iter().map(String::as_str),
        );
        extraction.probe_timeout = timeout;
        extraction.convert_timeout = timeout;
        extraction.max_sidecar_bytes = subtitle_bytes;
        extraction.max_converted_subtitle_bytes = subtitle_bytes;
        extraction.max_cues_per_source = max_segments;
        extraction.max_segments_per_media = max_segments;
        extraction.max_text_chars_per_cue = config.max_chars_per_segment.max(1);
        extraction.redactor =
            Arc::new(PrivacyTimedTextRedactor::from_config(&config.redaction)?);

        Ok(extraction)
    }
}

/// Per-media extraction request. `library_roots` bound sidecar discovery and
/// media probing scope.
#[derive(Debug, Clone)]
pub struct TimedTextExtractionRequest {
    /// Library that owns the media file.
    pub library_id: LibraryId,
    /// Playable movie or episode id.
    pub media_id: MediaID,
    /// Durable media-file id used by transcript storage.
    pub media_file_id: Uuid,
    /// Local media path. This path is never copied into extraction status.
    pub media_path: PathBuf,
    /// Configured library roots. Discovery is limited to these roots.
    pub library_roots: Vec<PathBuf>,
}

/// Safe source reference for skip/failure statuses.
#[derive(Clone, PartialEq, Eq)]
pub enum TimedTextSourceRef {
    /// Status applies to the media file as a whole.
    Media,
    /// Sidecar source identified by a root-relative path hash.
    Sidecar { path_hash: String },
    /// Embedded stream identified by stream index and codec.
    Embedded {
        stream_index: i32,
        codec_name: String,
    },
}

impl fmt::Debug for TimedTextSourceRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TimedTextSourceRef::Media => f.write_str("Media"),
            TimedTextSourceRef::Sidecar { path_hash } => f
                .debug_struct("Sidecar")
                .field("path_hash", path_hash)
                .finish(),
            TimedTextSourceRef::Embedded {
                stream_index,
                codec_name,
            } => f
                .debug_struct("Embedded")
                .field("stream_index", stream_index)
                .field("codec_name", codec_name)
                .finish(),
        }
    }
}

/// Skip category for sources that were intentionally not persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimedTextSkipKind {
    /// Media is not a playable movie/episode transcript target.
    UnsupportedMediaKind,
    /// Media or sidecar canonical path did not remain under a configured root.
    OutsideLibraryRoot,
    /// Subtitle codec is not text-convertible (for example PGS/DVD bitmap).
    UnsupportedSubtitleCodec,
    /// Source exceeded an input/output resource bound.
    ResourceLimitExceeded,
    /// Source parsed successfully but contained no usable cue text.
    EmptySource,
    /// Source contained only malformed cues.
    MalformedSource,
    /// Source language is not included in the configured allow-list.
    LanguageNotAllowed,
    /// Additional source skipped after the per-media source cap was reached.
    SourceLimitExceeded,
    /// Additional source skipped after the per-media segment cap was reached.
    SegmentLimitExceeded,
}

/// Safe skip status. Does not include paths, raw subtitle text, or command
/// stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedTextExtractionSkip {
    /// Source that was skipped.
    pub source: TimedTextSourceRef,
    /// Skip category.
    pub kind: TimedTextSkipKind,
    /// Safe machine-readable detail.
    pub detail: String,
}

/// Failure category for extraction/probing work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimedTextFailureKind {
    /// Media or sidecar IO failed.
    Io,
    /// `ffprobe` exited unsuccessfully.
    ProbeCommandFailed,
    /// `ffprobe` did not exit before the configured timeout.
    ProbeTimedOut,
    /// `ffprobe` stdout exceeded the configured cap.
    ProbeOutputTooLarge,
    /// `ffprobe` returned invalid stream JSON.
    ProbeJsonInvalid,
    /// `ffmpeg` exited unsuccessfully for a stream conversion.
    FfmpegCommandFailed,
    /// `ffmpeg` did not exit before the configured timeout.
    FfmpegTimedOut,
    /// Converted subtitle stdout exceeded the configured cap.
    FfmpegOutputTooLarge,
    /// Converted subtitle bytes could not be parsed into usable cues.
    ParseFailed,
}

/// Safe failure status. Does not include paths, raw subtitle text, or command
/// stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedTextExtractionFailure {
    /// Source that failed.
    pub source: TimedTextSourceRef,
    /// Failure category.
    pub kind: TimedTextFailureKind,
    /// Safe machine-readable detail.
    pub detail: String,
}

/// Transcript source and segment upserts emitted for one parsed subtitle source.
pub struct TimedTextSourceUpsertBatch {
    /// Source manifest upsert. This contains only hashed/safe locators.
    pub source: TranscriptSourceUpsert,
    /// Segment upserts with redacted cue text.
    pub segments: Vec<TranscriptSegmentUpsert>,
    /// Deterministic segment content hashes computed from cue index, time range,
    /// and redacted text.
    pub segment_content_hashes: Vec<String>,
}

impl fmt::Debug for TimedTextSourceUpsertBatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TimedTextSourceUpsertBatch")
            .field("source_kind", &self.source.source_kind)
            .field("language_code", &self.source.language_code)
            .field("source_key", &self.source.source_key)
            .field("stream_index", &self.source.stream_index)
            .field("source_path_hash", &self.source.source_path_hash)
            .field("source_content_hash", &self.source.source_content_hash)
            .field(
                "normalized_content_hash",
                &self.source.normalized_content_hash,
            )
            .field("segment_count", &self.segments.len())
            .field("segment_content_hashes", &self.segment_content_hashes)
            .finish()
    }
}

/// Complete extraction result for one media file.
pub struct TimedTextExtractionOutcome {
    /// Upserts ready to pass to `TranscriptRepository::upsert_source_with_segments`.
    pub sources: Vec<TimedTextSourceUpsertBatch>,
    /// Sources intentionally skipped with safe classifications.
    pub skipped: Vec<TimedTextExtractionSkip>,
    /// Probe, IO, conversion, and parse failures with safe classifications.
    pub failures: Vec<TimedTextExtractionFailure>,
}

impl TimedTextExtractionOutcome {
    fn new() -> Self {
        Self {
            sources: Vec::new(),
            skipped: Vec::new(),
            failures: Vec::new(),
        }
    }

    /// Total redacted transcript segments emitted across sources.
    pub fn segment_count(&self) -> usize {
        self.sources
            .iter()
            .map(|source| source.segments.len())
            .sum()
    }
}

impl fmt::Debug for TimedTextExtractionOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TimedTextExtractionOutcome")
            .field("source_count", &self.sources.len())
            .field("segment_count", &self.segment_count())
            .field("sources", &self.sources)
            .field("skipped", &self.skipped)
            .field("failures", &self.failures)
            .finish()
    }
}

/// Bounded subtitle extractor for sidecar files and embedded text streams.
#[derive(Clone, Debug)]
pub struct TimedTextExtractor {
    config: TimedTextExtractionConfig,
}

impl TimedTextExtractor {
    /// Create an extractor with explicit runtime bounds and binary paths.
    pub fn new(config: TimedTextExtractionConfig) -> Self {
        Self { config }
    }

    /// Extract sidecar and embedded timed text for one playable media file.
    pub async fn extract(
        &self,
        request: TimedTextExtractionRequest,
    ) -> Result<TimedTextExtractionOutcome> {
        let mut outcome = TimedTextExtractionOutcome::new();
        if !is_playable_media(&request.media_id) {
            outcome.skipped.push(TimedTextExtractionSkip {
                source: TimedTextSourceRef::Media,
                kind: TimedTextSkipKind::UnsupportedMediaKind,
                detail:
                    "transcripts are extracted only for movie or episode media"
                        .to_string(),
            });
            return Ok(outcome);
        }

        let scope = match resolve_scope(&request).await {
            Ok(scope) => scope,
            Err(scope_failure) => {
                outcome.skipped.push(scope_failure);
                return Ok(outcome);
            }
        };

        if self.config.extract_sidecars {
            self.extract_sidecars(&request, &scope, &mut outcome).await;
        }
        if self.config.extract_embedded {
            self.extract_embedded(&request, &scope, &mut outcome).await;
        }
        Ok(outcome)
    }

    fn language_allowed(&self, language_code: &str) -> bool {
        self.config.allowed_languages.is_empty()
            || self.config.allowed_languages.iter().any(|allowed| {
                allowed == &normalize_language_code(language_code)
            })
    }

    fn remaining_segments(
        &self,
        outcome: &TimedTextExtractionOutcome,
    ) -> usize {
        self.config
            .max_segments_per_media
            .saturating_sub(outcome.segment_count())
    }

    fn segment_limit_skip(
        source: TimedTextSourceRef,
    ) -> TimedTextExtractionSkip {
        TimedTextExtractionSkip {
            source,
            kind: TimedTextSkipKind::SegmentLimitExceeded,
            detail: "per-media transcript segment cap reached".to_string(),
        }
    }

    async fn extract_sidecars(
        &self,
        request: &TimedTextExtractionRequest,
        scope: &ResolvedExtractionScope,
        outcome: &mut TimedTextExtractionOutcome,
    ) {
        let sidecars = match discover_sidecars(scope).await {
            Ok(sidecars) => sidecars,
            Err(err) => {
                outcome.failures.push(TimedTextExtractionFailure {
                    source: TimedTextSourceRef::Media,
                    kind: TimedTextFailureKind::Io,
                    detail: format!(
                        "sidecar directory scan failed with {:?}",
                        err.kind()
                    ),
                });
                return;
            }
        };

        for candidate in sidecars {
            if outcome.sources.len() >= self.config.max_sources_per_media {
                outcome.skipped.push(TimedTextExtractionSkip {
                    source: TimedTextSourceRef::Sidecar {
                        path_hash: candidate.path_hash,
                    },
                    kind: TimedTextSkipKind::SourceLimitExceeded,
                    detail: "per-media subtitle source cap reached".to_string(),
                });
                continue;
            }

            let source_ref = TimedTextSourceRef::Sidecar {
                path_hash: candidate.path_hash.clone(),
            };
            let language_code =
                normalize_language_code(&candidate.language_code);
            if !self.language_allowed(&language_code) {
                outcome.skipped.push(TimedTextExtractionSkip {
                    source: source_ref,
                    kind: TimedTextSkipKind::LanguageNotAllowed,
                    detail:
                        "subtitle language is not in the configured allow-list"
                            .to_string(),
                });
                continue;
            }
            let remaining_segments = self.remaining_segments(outcome);
            if remaining_segments == 0 {
                outcome.skipped.push(Self::segment_limit_skip(source_ref));
                continue;
            }
            let bytes = match read_file_capped(
                &candidate.path,
                self.config.max_sidecar_bytes,
            )
            .await
            {
                Ok(bytes) => bytes,
                Err(ReadCappedError::TooLarge) => {
                    outcome.skipped.push(TimedTextExtractionSkip {
                        source: source_ref,
                        kind: TimedTextSkipKind::ResourceLimitExceeded,
                        detail: "sidecar exceeded configured byte cap"
                            .to_string(),
                    });
                    continue;
                }
                Err(ReadCappedError::Io(err)) => {
                    outcome.failures.push(TimedTextExtractionFailure {
                        source: source_ref,
                        kind: TimedTextFailureKind::Io,
                        detail: format!(
                            "sidecar read failed with {:?}",
                            err.kind()
                        ),
                    });
                    continue;
                }
            };

            match self.build_source_batch(SourceBuildInput {
                request,
                source_kind: TimedTextSourceKind::Sidecar,
                source_ref,
                language_code,
                source_key: format!("sidecar:{}", candidate.path_hash),
                source_name: Some("Sidecar subtitle".to_string()),
                stream_index: None,
                source_path_hash: Some(candidate.path_hash.clone()),
                source_content_bytes: &bytes,
                subtitle_text: &String::from_utf8_lossy(&bytes),
                format: candidate.format,
                source_locator: json!({
                    "kind": "sidecar_hash",
                    "path_hash": candidate.path_hash,
                    "format": candidate.format.as_str(),
                }),
                extra_metadata: json!({
                    "format": candidate.format.as_str(),
                    "source": "sidecar",
                }),
                source_segment_limit: remaining_segments,
            }) {
                Ok(batch) => outcome.sources.push(batch),
                Err(skip) => outcome.skipped.push(skip),
            }
        }
    }

    async fn extract_embedded(
        &self,
        request: &TimedTextExtractionRequest,
        scope: &ResolvedExtractionScope,
        outcome: &mut TimedTextExtractionOutcome,
    ) {
        let probe_args = vec![
            "-v".to_string(),
            "error".to_string(),
            "-select_streams".to_string(),
            "s".to_string(),
            "-show_entries".to_string(),
            "stream=index,codec_name,codec_type:stream_tags=language,title"
                .to_string(),
            "-of".to_string(),
            "json".to_string(),
            scope.media_path.to_string_lossy().to_string(),
        ];
        let probe = match run_bounded_command(
            &self.config.ffprobe_path,
            &probe_args,
            self.config.probe_timeout,
            self.config.max_probe_json_bytes,
            STDERR_CAP_BYTES,
        )
        .await
        {
            Ok(output) => output,
            Err(BoundedCommandError::TimedOut) => {
                outcome.failures.push(TimedTextExtractionFailure {
                    source: TimedTextSourceRef::Media,
                    kind: TimedTextFailureKind::ProbeTimedOut,
                    detail: "ffprobe exceeded configured timeout".to_string(),
                });
                return;
            }
            Err(BoundedCommandError::Io(kind)) => {
                outcome.failures.push(TimedTextExtractionFailure {
                    source: TimedTextSourceRef::Media,
                    kind: TimedTextFailureKind::Io,
                    detail: format!("ffprobe IO failed with {kind:?}"),
                });
                return;
            }
            Err(BoundedCommandError::Join) => {
                outcome.failures.push(TimedTextExtractionFailure {
                    source: TimedTextSourceRef::Media,
                    kind: TimedTextFailureKind::Io,
                    detail: "ffprobe output reader failed".to_string(),
                });
                return;
            }
        };

        if probe.stdout_truncated {
            outcome.failures.push(TimedTextExtractionFailure {
                source: TimedTextSourceRef::Media,
                kind: TimedTextFailureKind::ProbeOutputTooLarge,
                detail: "ffprobe stdout exceeded configured byte cap"
                    .to_string(),
            });
            return;
        }
        if !probe.status.success() {
            outcome.failures.push(TimedTextExtractionFailure {
                source: TimedTextSourceRef::Media,
                kind: TimedTextFailureKind::ProbeCommandFailed,
                detail: exit_detail("ffprobe", probe.status.code()),
            });
            return;
        }

        let mut streams: FfprobeStreams =
            match serde_json::from_slice(&probe.stdout) {
                Ok(streams) => streams,
                Err(_) => {
                    outcome.failures.push(TimedTextExtractionFailure {
                        source: TimedTextSourceRef::Media,
                        kind: TimedTextFailureKind::ProbeJsonInvalid,
                        detail: "ffprobe returned invalid stream JSON"
                            .to_string(),
                    });
                    return;
                }
            };
        streams.streams.sort_by_key(|stream| stream.index);

        for stream in streams.streams {
            let Some(codec_name) = stream.codec_name.as_deref() else {
                continue;
            };
            if !matches!(stream.codec_type.as_deref(), Some("subtitle") | None)
            {
                continue;
            }
            let stream_ref = TimedTextSourceRef::Embedded {
                stream_index: stream.index,
                codec_name: codec_name.to_ascii_lowercase(),
            };

            match classify_embedded_codec(codec_name) {
                EmbeddedCodecClass::Text => {}
                EmbeddedCodecClass::Unsupported => {
                    outcome.skipped.push(TimedTextExtractionSkip {
                        source: stream_ref,
                        kind: TimedTextSkipKind::UnsupportedSubtitleCodec,
                        detail:
                            "embedded subtitle codec is not text-convertible"
                                .to_string(),
                    });
                    continue;
                }
            }

            let language_code = stream_language(&stream.tags);
            if !self.language_allowed(&language_code) {
                outcome.skipped.push(TimedTextExtractionSkip {
                    source: stream_ref,
                    kind: TimedTextSkipKind::LanguageNotAllowed,
                    detail:
                        "subtitle language is not in the configured allow-list"
                            .to_string(),
                });
                continue;
            }

            if outcome.sources.len() >= self.config.max_sources_per_media {
                outcome.skipped.push(TimedTextExtractionSkip {
                    source: stream_ref,
                    kind: TimedTextSkipKind::SourceLimitExceeded,
                    detail: "per-media subtitle source cap reached".to_string(),
                });
                continue;
            }

            let remaining_segments = self.remaining_segments(outcome);
            if remaining_segments == 0 {
                outcome.skipped.push(Self::segment_limit_skip(stream_ref));
                continue;
            }

            let convert_args = vec![
                "-v".to_string(),
                "error".to_string(),
                "-nostdin".to_string(),
                "-i".to_string(),
                scope.media_path.to_string_lossy().to_string(),
                "-map".to_string(),
                format!("0:{}", stream.index),
                "-f".to_string(),
                "srt".to_string(),
                "-".to_string(),
            ];
            let converted = match run_bounded_command(
                &self.config.ffmpeg_path,
                &convert_args,
                self.config.convert_timeout,
                self.config.max_converted_subtitle_bytes,
                STDERR_CAP_BYTES,
            )
            .await
            {
                Ok(output) => output,
                Err(BoundedCommandError::TimedOut) => {
                    outcome.failures.push(TimedTextExtractionFailure {
                        source: stream_ref,
                        kind: TimedTextFailureKind::FfmpegTimedOut,
                        detail: "ffmpeg exceeded configured timeout"
                            .to_string(),
                    });
                    continue;
                }
                Err(BoundedCommandError::Io(kind)) => {
                    outcome.failures.push(TimedTextExtractionFailure {
                        source: stream_ref,
                        kind: TimedTextFailureKind::Io,
                        detail: format!("ffmpeg IO failed with {kind:?}"),
                    });
                    continue;
                }
                Err(BoundedCommandError::Join) => {
                    outcome.failures.push(TimedTextExtractionFailure {
                        source: stream_ref,
                        kind: TimedTextFailureKind::Io,
                        detail: "ffmpeg output reader failed".to_string(),
                    });
                    continue;
                }
            };

            if converted.stdout_truncated {
                outcome.failures.push(TimedTextExtractionFailure {
                    source: stream_ref,
                    kind: TimedTextFailureKind::FfmpegOutputTooLarge,
                    detail:
                        "converted subtitle stdout exceeded configured byte cap"
                            .to_string(),
                });
                continue;
            }
            if !converted.status.success() {
                outcome.failures.push(TimedTextExtractionFailure {
                    source: stream_ref,
                    kind: TimedTextFailureKind::FfmpegCommandFailed,
                    detail: exit_detail("ffmpeg", converted.status.code()),
                });
                continue;
            }

            match self.build_source_batch(SourceBuildInput {
                request,
                source_kind: TimedTextSourceKind::Embedded,
                source_ref: stream_ref,
                language_code,
                source_key: format!("embedded:{}", stream.index),
                source_name: Some(format!(
                    "Embedded subtitle stream {}",
                    stream.index
                )),
                stream_index: Some(stream.index),
                source_path_hash: None,
                source_content_bytes: &converted.stdout,
                subtitle_text: &String::from_utf8_lossy(&converted.stdout),
                format: SubtitleFormat::Srt,
                source_locator: json!({
                    "kind": "embedded_stream",
                    "stream_index": stream.index,
                    "codec_name": codec_name.to_ascii_lowercase(),
                }),
                extra_metadata: json!({
                    "format": "srt",
                    "source": "embedded",
                    "codec_name": codec_name.to_ascii_lowercase(),
                    "ffmpeg_format": "srt",
                }),
                source_segment_limit: remaining_segments,
            }) {
                Ok(batch) => outcome.sources.push(batch),
                Err(skip) => outcome.skipped.push(skip),
            }
        }
    }

    fn build_source_batch(
        &self,
        input: SourceBuildInput<'_>,
    ) -> std::result::Result<TimedTextSourceUpsertBatch, TimedTextExtractionSkip>
    {
        let parse = match input.format {
            SubtitleFormat::Srt => parse_srt(input.subtitle_text),
            SubtitleFormat::WebVtt => parse_webvtt(input.subtitle_text),
        };

        if parse.cues.is_empty() {
            let kind = if parse.warnings.is_empty() {
                TimedTextSkipKind::EmptySource
            } else {
                TimedTextSkipKind::MalformedSource
            };
            return Err(TimedTextExtractionSkip {
                source: input.source_ref,
                kind,
                detail: "subtitle source contained no usable cues".to_string(),
            });
        }

        let mut segments = Vec::new();
        let mut segment_hashes = Vec::new();
        let mut normalized_hasher = Sha256::new();
        let mut duration_ms: Option<i64> = None;
        let mut truncated = false;

        let source_segment_limit = self
            .config
            .max_cues_per_source
            .min(input.source_segment_limit)
            .max(1);

        for cue in parse.cues.iter().take(source_segment_limit) {
            let mut redacted = self.config.redactor.redact(&cue.text);
            let (trimmed, was_truncated) = trim_to_chars(
                redacted.as_str(),
                self.config.max_text_chars_per_cue,
            );
            if was_truncated {
                truncated = true;
                redacted = trimmed;
            }
            if redacted.trim().is_empty() {
                continue;
            }
            let cue_index = i32::try_from(segments.len()).unwrap_or(i32::MAX);
            let segment_hash = segment_content_hash(
                cue_index,
                cue.start_ms,
                cue.end_ms,
                &redacted,
            );
            normalized_hasher.update(cue_index.to_string().as_bytes());
            normalized_hasher.update(b"\x1f");
            normalized_hasher.update(cue.start_ms.to_string().as_bytes());
            normalized_hasher.update(b"\x1f");
            normalized_hasher.update(cue.end_ms.to_string().as_bytes());
            normalized_hasher.update(b"\x1f");
            normalized_hasher.update(redacted.as_bytes());
            normalized_hasher.update(b"\x1e");
            duration_ms = Some(
                duration_ms
                    .map_or(cue.end_ms, |current| current.max(cue.end_ms)),
            );
            segments.push(TranscriptSegmentUpsert {
                cue_index,
                start_ms: cue.start_ms,
                end_ms: cue.end_ms,
                text: redacted,
                metadata: json!({
                    "segment_content_hash": segment_hash,
                    "source_cue_index": cue.cue_index,
                }),
            });
            segment_hashes.push(segment_hash);
        }

        if parse.cues.len() > source_segment_limit {
            truncated = true;
        }

        if segments.is_empty() {
            return Err(TimedTextExtractionSkip {
                source: input.source_ref,
                kind: TimedTextSkipKind::EmptySource,
                detail: "redaction removed all usable cue text".to_string(),
            });
        }

        let normalized_content_hash = hex::encode(normalized_hasher.finalize());
        let source_content_hash = sha256_hex(input.source_content_bytes);
        let metadata = merge_metadata(
            input.extra_metadata,
            json!({
                "cue_count": segments.len(),
                "parse_warning_count": parse.warnings.len(),
                "source_cue_count": parse.cues.len(),
                "truncated": truncated,
                "text_redaction": "applied",
            }),
        );

        Ok(TimedTextSourceUpsertBatch {
            source: TranscriptSourceUpsert {
                source_id: None,
                library_id: input.request.library_id,
                media_id: input.request.media_id,
                media_file_id: input.request.media_file_id,
                source_kind: input.source_kind,
                language_code: normalize_language_code(&input.language_code),
                source_key: input.source_key,
                source_name: input.source_name,
                stream_index: input.stream_index,
                source_path_hash: input.source_path_hash,
                source_content_hash,
                normalized_content_hash: Some(normalized_content_hash),
                artifact_id: None,
                duration_ms,
                source_locator: input.source_locator,
                metadata,
            },
            segments,
            segment_content_hashes: segment_hashes,
        })
    }
}

struct SourceBuildInput<'a> {
    request: &'a TimedTextExtractionRequest,
    source_kind: TimedTextSourceKind,
    source_ref: TimedTextSourceRef,
    language_code: String,
    source_key: String,
    source_name: Option<String>,
    stream_index: Option<i32>,
    source_path_hash: Option<String>,
    source_content_bytes: &'a [u8],
    subtitle_text: &'a str,
    format: SubtitleFormat,
    source_locator: serde_json::Value,
    extra_metadata: serde_json::Value,
    source_segment_limit: usize,
}

/// Parse SubRip subtitle text into normalized cues.
pub fn parse_srt(input: &str) -> SubtitleParseReport {
    parse_subtitle_blocks(input, SubtitleFormat::Srt)
}

/// Parse WebVTT subtitle text into normalized cues.
pub fn parse_webvtt(input: &str) -> SubtitleParseReport {
    parse_subtitle_blocks(input, SubtitleFormat::WebVtt)
}

fn parse_subtitle_blocks(
    input: &str,
    format: SubtitleFormat,
) -> SubtitleParseReport {
    let mut report = SubtitleParseReport::new();
    let mut block = Vec::new();
    let mut ordinal = 0usize;
    let normalized_input = input.replace("\r\n", "\n").replace('\r', "\n");

    for line in normalized_input.trim_start_matches('\u{feff}').lines() {
        if line.trim().is_empty() {
            if !block.is_empty() {
                ordinal += 1;
                parse_block(&block, ordinal, format, &mut report);
                block.clear();
            }
        } else {
            block.push(line.to_string());
        }
    }
    if !block.is_empty() {
        ordinal += 1;
        parse_block(&block, ordinal, format, &mut report);
    }

    for (idx, cue) in report.cues.iter_mut().enumerate() {
        cue.cue_index = i32::try_from(idx).unwrap_or(i32::MAX);
    }

    report
}

fn parse_block(
    block: &[String],
    ordinal: usize,
    format: SubtitleFormat,
    report: &mut SubtitleParseReport,
) {
    if block.is_empty() {
        return;
    }

    if format == SubtitleFormat::WebVtt && is_webvtt_control_block(block) {
        return;
    }

    let timing_line_index = block.iter().position(|line| line.contains("-->"));
    let Some(timing_line_index) = timing_line_index else {
        if !(format == SubtitleFormat::WebVtt
            && ordinal == 1
            && block[0]
                .trim_start_matches('\u{feff}')
                .starts_with("WEBVTT"))
        {
            report
                .push_warning(ordinal, SubtitleParseWarningKind::MissingTiming);
        }
        return;
    };

    let Some((start_ms, end_ms)) = parse_timing_line(&block[timing_line_index])
    else {
        report.push_warning(ordinal, SubtitleParseWarningKind::InvalidTiming);
        return;
    };
    if end_ms <= start_ms {
        report.push_warning(ordinal, SubtitleParseWarningKind::InvalidTiming);
        return;
    }

    let text =
        normalize_subtitle_text(&block[timing_line_index + 1..].join("\n"));
    if text.is_empty() {
        report.push_warning(ordinal, SubtitleParseWarningKind::EmptyText);
        return;
    }

    report.cues.push(ParsedSubtitleCue {
        cue_index: i32::try_from(report.cues.len()).unwrap_or(i32::MAX),
        start_ms,
        end_ms,
        text,
    });
}

fn is_webvtt_control_block(block: &[String]) -> bool {
    let first = block[0].trim_start_matches('\u{feff}').trim();
    first.starts_with("WEBVTT")
        || first.starts_with("NOTE")
        || first == "STYLE"
        || first == "REGION"
}

fn parse_timing_line(line: &str) -> Option<(i64, i64)> {
    let (start, rest) = line.split_once("-->")?;
    let end = rest.split_whitespace().next()?;
    Some((
        parse_timestamp_ms(start.trim())?,
        parse_timestamp_ms(end.trim())?,
    ))
}

fn parse_timestamp_ms(value: &str) -> Option<i64> {
    let value = value.trim().replace(',', ".");
    let (clock, fraction) = value.split_once('.')?;
    let mut parts = clock.split(':').collect::<Vec<_>>();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }

    let seconds = parts.pop()?.parse::<i64>().ok()?;
    let minutes = parts.pop()?.parse::<i64>().ok()?;
    let hours = if let Some(hours) = parts.pop() {
        hours.parse::<i64>().ok()?
    } else {
        0
    };
    if minutes >= 60 || seconds >= 60 || minutes < 0 || seconds < 0 || hours < 0
    {
        return None;
    }

    let millis = parse_millis_fraction(fraction)?;
    Some((((hours * 60) + minutes) * 60 + seconds) * 1000 + millis)
}

fn parse_millis_fraction(fraction: &str) -> Option<i64> {
    let digits = fraction
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    let mut millis = String::with_capacity(3);
    for ch in digits.chars().take(3) {
        millis.push(ch);
    }
    while millis.len() < 3 {
        millis.push('0');
    }
    millis.parse::<i64>().ok()
}

/// Normalize cue text by removing common subtitle markup, decoding common HTML
/// entities, trimming empty lines, and collapsing intra-line whitespace.
pub fn normalize_subtitle_text(input: &str) -> String {
    let without_ass = strip_ass_overrides(input);
    let without_tags = strip_angle_tags(&without_ass);
    let decoded = decode_basic_entities(&without_tags);
    decoded
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(collapse_whitespace)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_ass_overrides(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut depth = 0usize;
    for ch in input.chars() {
        match ch {
            '{' => depth += 1,
            '}' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out
}

fn strip_angle_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

fn decode_basic_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

fn collapse_whitespace(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug)]
struct ResolvedExtractionScope {
    media_path: PathBuf,
    media_stem: String,
    media_parent: PathBuf,
    roots: Vec<PathBuf>,
}

async fn resolve_scope(
    request: &TimedTextExtractionRequest,
) -> std::result::Result<ResolvedExtractionScope, TimedTextExtractionSkip> {
    let media_path =
        fs::canonicalize(&request.media_path).await.map_err(|_| {
            TimedTextExtractionSkip {
                source: TimedTextSourceRef::Media,
                kind: TimedTextSkipKind::OutsideLibraryRoot,
                detail:
                    "media path could not be resolved under configured roots"
                        .to_string(),
            }
        })?;

    let mut roots = Vec::new();
    for root in &request.library_roots {
        if let Ok(root) = fs::canonicalize(root).await {
            roots.push(root);
        }
    }
    roots.sort();
    roots.dedup();

    if roots.is_empty()
        || !roots.iter().any(|root| media_path.starts_with(root))
    {
        return Err(TimedTextExtractionSkip {
            source: TimedTextSourceRef::Media,
            kind: TimedTextSkipKind::OutsideLibraryRoot,
            detail: "media path is outside configured library roots"
                .to_string(),
        });
    }

    let Some(media_parent) = media_path.parent().map(Path::to_path_buf) else {
        return Err(TimedTextExtractionSkip {
            source: TimedTextSourceRef::Media,
            kind: TimedTextSkipKind::OutsideLibraryRoot,
            detail: "media path has no parent under configured roots"
                .to_string(),
        });
    };
    if !roots.iter().any(|root| media_parent.starts_with(root)) {
        return Err(TimedTextExtractionSkip {
            source: TimedTextSourceRef::Media,
            kind: TimedTextSkipKind::OutsideLibraryRoot,
            detail: "media parent is outside configured library roots"
                .to_string(),
        });
    }

    let media_stem = media_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
        .to_string();

    Ok(ResolvedExtractionScope {
        media_path,
        media_stem,
        media_parent,
        roots,
    })
}

#[derive(Debug)]
struct SidecarCandidate {
    path: PathBuf,
    path_hash: String,
    language_code: String,
    format: SubtitleFormat,
}

async fn discover_sidecars(
    scope: &ResolvedExtractionScope,
) -> io::Result<Vec<SidecarCandidate>> {
    let mut entries = fs::read_dir(&scope.media_parent).await?;
    let mut sidecars = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let Some(format) = SubtitleFormat::from_path(&path) else {
            continue;
        };
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if !matches_sidecar_stem(&scope.media_stem, stem) {
            continue;
        }
        let Ok(canonical) = fs::canonicalize(&path).await else {
            continue;
        };
        if !scope.roots.iter().any(|root| canonical.starts_with(root)) {
            continue;
        }
        let path_hash = path_hash_under_roots(&canonical, &scope.roots);
        let language_code = sidecar_language(&scope.media_stem, stem);
        sidecars.push(SidecarCandidate {
            path: canonical,
            path_hash,
            language_code,
            format,
        });
    }
    sidecars.sort_by(|left, right| left.path_hash.cmp(&right.path_hash));
    Ok(sidecars)
}

fn matches_sidecar_stem(media_stem: &str, sidecar_stem: &str) -> bool {
    sidecar_stem == media_stem
        || sidecar_stem.strip_prefix(media_stem).is_some_and(|suffix| {
            suffix.starts_with('.')
                || suffix.starts_with('-')
                || suffix.starts_with('_')
        })
}

fn sidecar_language(media_stem: &str, sidecar_stem: &str) -> String {
    let suffix = sidecar_stem
        .strip_prefix(media_stem)
        .unwrap_or_default()
        .trim_start_matches(['.', '-', '_']);
    for token in suffix.split(['.', '-', '_']) {
        let normalized = normalize_language_code(token);
        if normalized != DEFAULT_LANGUAGE && !is_sidecar_modifier(token) {
            return normalized;
        }
    }
    DEFAULT_LANGUAGE.to_string()
}

fn is_sidecar_modifier(token: &str) -> bool {
    matches!(
        token.to_ascii_lowercase().as_str(),
        "forced" | "sdh" | "cc" | "hi" | "default"
    )
}

fn path_hash_under_roots(path: &Path, roots: &[PathBuf]) -> String {
    let root = roots
        .iter()
        .filter(|root| path.starts_with(root))
        .max_by_key(|root| root.components().count());
    let relative = root
        .and_then(|root| path.strip_prefix(root).ok())
        .unwrap_or(path);
    sha256_hex(relative.to_string_lossy().as_bytes())
}

fn is_playable_media(media_id: &MediaID) -> bool {
    matches!(media_id, MediaID::Movie(_) | MediaID::Episode(_))
}

fn normalize_language_allow_list<'a>(
    languages: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let mut normalized = languages
        .into_iter()
        .filter_map(|language| {
            let trimmed = language.trim();
            (!trimmed.is_empty()).then(|| normalize_language_code(trimmed))
        })
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn normalize_language_code(value: &str) -> String {
    let value = value.trim().to_ascii_lowercase();
    let mapped = match value.as_str() {
        "eng" | "english" => "en",
        "fre" | "fra" | "french" => "fr",
        "ger" | "deu" | "german" => "de",
        "spa" | "spanish" => "es",
        "ita" | "italian" => "it",
        "jpn" | "japanese" => "ja",
        "kor" | "korean" => "ko",
        "por" | "portuguese" => "pt",
        "und" | "unknown" | "" => DEFAULT_LANGUAGE,
        _ => value.as_str(),
    };
    let sanitized = mapped
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .take(32)
        .collect::<String>();
    if sanitized.is_empty() {
        DEFAULT_LANGUAGE.to_string()
    } else {
        sanitized
    }
}

fn stream_language(tags: &Option<HashMap<String, String>>) -> String {
    tags.as_ref()
        .and_then(|tags| {
            tags.get("language")
                .or_else(|| tags.get("LANGUAGE"))
                .or_else(|| tags.get("Language"))
        })
        .map(|value| normalize_language_code(value))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_LANGUAGE.to_string())
}

#[derive(Debug, Deserialize)]
struct FfprobeStreams {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    index: i32,
    codec_name: Option<String>,
    codec_type: Option<String>,
    tags: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmbeddedCodecClass {
    Text,
    Unsupported,
}

fn classify_embedded_codec(codec_name: &str) -> EmbeddedCodecClass {
    match codec_name.to_ascii_lowercase().as_str() {
        "subrip" | "srt" | "ass" | "ssa" | "webvtt" | "mov_text" | "text"
        | "text_subtitle" => EmbeddedCodecClass::Text,
        "hdmv_pgs_subtitle" | "pgssub" | "dvd_subtitle" | "dvdsub"
        | "dvb_subtitle" | "xsub" | "eia_608" | "dvb_teletext" => {
            EmbeddedCodecClass::Unsupported
        }
        _ => EmbeddedCodecClass::Unsupported,
    }
}

#[derive(Debug)]
struct CappedBytes {
    bytes: Vec<u8>,
    truncated: bool,
}

#[derive(Debug)]
enum ReadCappedError {
    TooLarge,
    Io(io::Error),
}

async fn read_file_capped(
    path: &Path,
    cap: usize,
) -> std::result::Result<Vec<u8>, ReadCappedError> {
    if fs::metadata(path).await.map_err(ReadCappedError::Io)?.len() > cap as u64
    {
        return Err(ReadCappedError::TooLarge);
    }
    let file = fs::File::open(path).await.map_err(ReadCappedError::Io)?;
    let capped = read_capped(file, cap).await.map_err(ReadCappedError::Io)?;
    if capped.truncated {
        Err(ReadCappedError::TooLarge)
    } else {
        Ok(capped.bytes)
    }
}

async fn read_capped<R: AsyncRead + Unpin>(
    mut reader: R,
    cap: usize,
) -> io::Result<CappedBytes> {
    let mut bytes = Vec::with_capacity(cap.min(8192));
    let mut truncated = false;
    let mut chunk = [0u8; 8192];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        if bytes.len() < cap {
            let remaining = cap - bytes.len();
            let keep = remaining.min(read);
            bytes.extend_from_slice(&chunk[..keep]);
            if keep < read {
                truncated = true;
            }
        } else {
            truncated = true;
        }
    }
    Ok(CappedBytes { bytes, truncated })
}

#[derive(Debug)]
struct BoundedCommandOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stdout_truncated: bool,
}

#[derive(Debug)]
enum BoundedCommandError {
    TimedOut,
    Io(io::ErrorKind),
    Join,
}

async fn run_bounded_command(
    program: &Path,
    args: &[String],
    timeout: Duration,
    stdout_cap: usize,
    stderr_cap: usize,
) -> std::result::Result<BoundedCommandOutput, BoundedCommandError> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|err| BoundedCommandError::Io(err.kind()))?;

    let stdout = child
        .stdout
        .take()
        .ok_or(BoundedCommandError::Io(io::ErrorKind::Other))?;
    let stderr = child
        .stderr
        .take()
        .ok_or(BoundedCommandError::Io(io::ErrorKind::Other))?;

    let stdout_task = tokio::spawn(read_capped(stdout, stdout_cap));
    let stderr_task = tokio::spawn(read_capped(stderr, stderr_cap));

    let status = match time::timeout(timeout, child.wait()).await {
        Ok(status) => {
            status.map_err(|err| BoundedCommandError::Io(err.kind()))?
        }
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            let _ = stdout_task.await;
            let _ = stderr_task.await;
            return Err(BoundedCommandError::TimedOut);
        }
    };

    let stdout = stdout_task
        .await
        .map_err(|_| BoundedCommandError::Join)?
        .map_err(|err| BoundedCommandError::Io(err.kind()))?;
    let _stderr = stderr_task
        .await
        .map_err(|_| BoundedCommandError::Join)?
        .map_err(|err| BoundedCommandError::Io(err.kind()))?;

    Ok(BoundedCommandOutput {
        status,
        stdout: stdout.bytes,
        stdout_truncated: stdout.truncated,
    })
}

fn exit_detail(command: &str, code: Option<i32>) -> String {
    match code {
        Some(code) => format!("{command} exited with status {code}"),
        None => format!("{command} exited unsuccessfully"),
    }
}

fn segment_content_hash(
    cue_index: i32,
    start_ms: i64,
    end_ms: i64,
    text: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(cue_index.to_string().as_bytes());
    hasher.update(b"\x1f");
    hasher.update(start_ms.to_string().as_bytes());
    hasher.update(b"\x1f");
    hasher.update(end_ms.to_string().as_bytes());
    hasher.update(b"\x1f");
    hasher.update(text.as_bytes());
    hex::encode(hasher.finalize())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn trim_to_chars(value: &str, max_chars: usize) -> (String, bool) {
    if value.chars().count() <= max_chars {
        return (value.to_string(), false);
    }
    (value.chars().take(max_chars).collect(), true)
}

fn merge_metadata(
    mut base: serde_json::Value,
    extra: serde_json::Value,
) -> serde_json::Value {
    if let (Some(base), Some(extra)) = (base.as_object_mut(), extra.as_object())
    {
        for (key, value) in extra {
            base.insert(key.clone(), value.clone());
        }
    }
    base
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrex_model::{MovieID, SeriesID};
    use tempfile::TempDir;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn movie_id() -> MediaID {
        MediaID::Movie(MovieID(Uuid::from_u128(1)))
    }

    fn library_id() -> LibraryId {
        LibraryId(Uuid::from_u128(2))
    }

    fn request(root: &Path, media_path: &Path) -> TimedTextExtractionRequest {
        TimedTextExtractionRequest {
            library_id: library_id(),
            media_id: movie_id(),
            media_file_id: Uuid::from_u128(3),
            media_path: media_path.to_path_buf(),
            library_roots: vec![root.to_path_buf()],
        }
    }

    #[cfg(unix)]
    fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
        path
    }

    #[cfg(unix)]
    fn no_streams_probe(dir: &Path) -> PathBuf {
        write_script(
            dir,
            "ffprobe-empty.sh",
            "#!/bin/sh\nprintf '{\"streams\":[]}'\n",
        )
    }

    #[cfg(unix)]
    fn noop_ffmpeg(dir: &Path) -> PathBuf {
        write_script(dir, "ffmpeg-empty.sh", "#!/bin/sh\nexit 0\n")
    }

    #[cfg(unix)]
    fn config(
        ffprobe_path: PathBuf,
        ffmpeg_path: PathBuf,
    ) -> TimedTextExtractionConfig {
        TimedTextExtractionConfig {
            ffprobe_path,
            ffmpeg_path,
            extract_embedded: true,
            extract_sidecars: true,
            allowed_languages: Vec::new(),
            probe_timeout: Duration::from_secs(2),
            convert_timeout: Duration::from_secs(2),
            max_sidecar_bytes: DEFAULT_MAX_SIDECAR_BYTES,
            max_probe_json_bytes: DEFAULT_MAX_PROBE_BYTES,
            max_converted_subtitle_bytes: DEFAULT_MAX_CONVERTED_BYTES,
            max_cues_per_source: DEFAULT_MAX_CUES_PER_SOURCE,
            max_segments_per_media: DEFAULT_MAX_CUES_PER_SOURCE,
            max_sources_per_media: DEFAULT_MAX_SOURCES_PER_MEDIA,
            max_text_chars_per_cue: DEFAULT_MAX_TEXT_CHARS_PER_CUE,
            redactor: Arc::new(NoopTimedTextRedactor),
        }
    }

    #[test]
    fn srt_parser_handles_numbering_settings_multiline_tags_and_malformed() {
        let input = "\
1\n\
00:00:01,000 --> 00:00:03,500 X1:0 X2:10\n\
<i>Hello</i>   world\n\
Second   line\n\n\
not a cue\n\n\
3\n\
00:00:04.250 --> 00:00:05.000\n\
{\\an8}Bye &amp; thanks\n";

        let report = parse_srt(input);

        assert_eq!(report.cues.len(), 2);
        assert_eq!(report.cues[0].start_ms, 1000);
        assert_eq!(report.cues[0].end_ms, 3500);
        assert_eq!(report.cues[0].text, "Hello world\nSecond line");
        assert_eq!(report.cues[1].start_ms, 4250);
        assert_eq!(report.cues[1].text, "Bye & thanks");
        assert_eq!(report.skipped_count(), 1);
        assert_eq!(
            report.warnings[0].kind,
            SubtitleParseWarningKind::MissingTiming
        );
    }

    #[test]
    fn webvtt_parser_handles_header_ids_settings_tags_and_malformed() {
        let input = "\
WEBVTT - captions\n\n\
NOTE this block is ignored\n\
with more text\n\n\
c1\n\
00:00:01.500 --> 00:00:03.000 align:start position:0%\n\
<v Roger>Hello &amp; welcome</v>\n\
<c.red>home</c>\n\n\
00:00:04.000 --> 00:00:04.500\n\
second cue\n\n\
broken\n\
00:bad --> 00:00:05.000\n\
nope\n";

        let report = parse_webvtt(input);

        assert_eq!(report.cues.len(), 2);
        assert_eq!(report.cues[0].start_ms, 1500);
        assert_eq!(report.cues[0].end_ms, 3000);
        assert_eq!(report.cues[0].text, "Hello & welcome\nhome");
        assert_eq!(report.cues[1].text, "second cue");
        assert_eq!(report.skipped_count(), 1);
        assert_eq!(
            report.warnings[0].kind,
            SubtitleParseWarningKind::InvalidTiming
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn extractor_matches_sidecars_under_library_root_and_builds_upserts()
    {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("library");
        let scripts = temp.path().join("scripts");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&scripts).unwrap();
        let media = root.join("Arrival.mkv");
        std::fs::write(&media, b"movie").unwrap();
        std::fs::write(
            root.join("Arrival.en.srt"),
            "1\n00:00:01,000 --> 00:00:02,000\nHello\n",
        )
        .unwrap();
        std::fs::write(
            root.join("Arrival.fr.vtt"),
            "WEBVTT\n\n00:00:02.000 --> 00:00:03.000\nBonjour\n",
        )
        .unwrap();
        std::fs::write(
            root.join("Unrelated.en.srt"),
            "1\n00:00:01,000 --> 00:00:02,000\nIgnore\n",
        )
        .unwrap();

        let extractor = TimedTextExtractor::new(config(
            no_streams_probe(&scripts),
            noop_ffmpeg(&scripts),
        ));

        let outcome = extractor.extract(request(&root, &media)).await.unwrap();

        assert_eq!(outcome.sources.len(), 2);
        assert_eq!(outcome.segment_count(), 2);
        let mut languages = outcome
            .sources
            .iter()
            .map(|source| source.source.language_code.as_str())
            .collect::<Vec<_>>();
        languages.sort_unstable();
        assert_eq!(languages, vec!["en", "fr"]);
        assert!(outcome.sources.iter().all(|source| {
            source.source.source_kind == TimedTextSourceKind::Sidecar
                && source.source.source_key.starts_with("sidecar:")
                && source
                    .source
                    .source_path_hash
                    .as_deref()
                    .is_some_and(|hash| hash.len() == 64)
                && !source.source.source_locator.to_string().contains("Arrival")
        }));
        assert!(outcome.skipped.is_empty());
        assert!(outcome.failures.is_empty());
    }

    struct DigitRedactor;

    impl TimedTextRedactor for DigitRedactor {
        fn redact(&self, normalized_text: &str) -> String {
            normalized_text
                .chars()
                .map(|ch| if ch.is_ascii_digit() { 'X' } else { ch })
                .collect()
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn extractor_redacts_text_and_emits_deterministic_hashes() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("library");
        let scripts = temp.path().join("scripts");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&scripts).unwrap();
        let media = root.join("Movie.mkv");
        std::fs::write(&media, b"movie").unwrap();
        std::fs::write(
            root.join("Movie.en.srt"),
            "1\n00:00:01,000 --> 00:00:02,000\nCall 555-1212 now\n",
        )
        .unwrap();
        let mut cfg = config(no_streams_probe(&scripts), noop_ffmpeg(&scripts));
        cfg.redactor = Arc::new(DigitRedactor);

        let outcome = TimedTextExtractor::new(cfg)
            .extract(request(&root, &media))
            .await
            .unwrap();

        let source = &outcome.sources[0];
        assert_eq!(source.segments[0].text, "Call XXX-XXXX now");
        assert!(!source.segments[0].text.contains("555"));
        assert_eq!(source.source.source_content_hash.len(), 64);
        assert_eq!(
            source
                .source
                .normalized_content_hash
                .as_ref()
                .unwrap()
                .len(),
            64
        );
        assert_eq!(source.segment_content_hashes.len(), 1);
        assert_eq!(source.segment_content_hashes[0].len(), 64);
        assert_eq!(
            source.segments[0].metadata["segment_content_hash"],
            source.segment_content_hashes[0]
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn extractor_applies_privacy_redaction_before_upsert() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("library");
        let scripts = temp.path().join("scripts");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&scripts).unwrap();
        let media = root.join("Secrets.mkv");
        std::fs::write(&media, b"movie").unwrap();
        std::fs::write(
            root.join("Secrets.en.srt"),
            "1\n00:00:01,000 --> 00:00:02,000\nEmail me@example.com or call +1 (555) 867-5309\n\n2\n00:00:03,000 --> 00:00:04,000\nOpen https://x.test/watch?token=abc123secret and use Bearer abcdef123456\n",
        )
        .unwrap();
        let mut cfg = config(no_streams_probe(&scripts), noop_ffmpeg(&scripts));
        cfg.redactor = Arc::new(PrivacyTimedTextRedactor::default());

        let outcome = TimedTextExtractor::new(cfg)
            .extract(request(&root, &media))
            .await
            .unwrap();

        let joined = outcome.sources[0]
            .segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(joined.contains("[REDACTED:email]"));
        assert!(joined.contains("[REDACTED:phone]"));
        assert!(joined.contains("token=[REDACTED:url_secret]"));
        assert!(joined.contains("Bearer [REDACTED:token]"));
        assert!(!joined.contains("me@example.com"));
        assert!(!joined.contains("867-5309"));
        assert!(!joined.contains("abc123secret"));
        assert!(!joined.contains("abcdef123456"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn extractor_honors_source_language_and_segment_settings() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("library");
        let scripts = temp.path().join("scripts");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&scripts).unwrap();
        let media = root.join("Feature.mkv");
        std::fs::write(&media, b"movie").unwrap();
        std::fs::write(
            root.join("Feature.en.srt"),
            "1\n00:00:01,000 --> 00:00:02,000\nOne\n\n2\n00:00:03,000 --> 00:00:04,000\nTwo\n",
        )
        .unwrap();
        std::fs::write(
            root.join("Feature.fr.srt"),
            "1\n00:00:01,000 --> 00:00:02,000\nBonjour\n",
        )
        .unwrap();
        let mut cfg = config(no_streams_probe(&scripts), noop_ffmpeg(&scripts));
        cfg.allowed_languages = vec!["en".to_string()];
        cfg.max_segments_per_media = 1;
        cfg.max_cues_per_source = 1;

        let outcome = TimedTextExtractor::new(cfg)
            .extract(request(&root, &media))
            .await
            .unwrap();

        assert_eq!(outcome.segment_count(), 1);
        assert_eq!(outcome.sources.len(), 1);
        assert_eq!(outcome.sources[0].source.language_code, "en");
        assert!(
            outcome
                .skipped
                .iter()
                .any(|skip| skip.kind == TimedTextSkipKind::LanguageNotAllowed)
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn extractor_disabled_sources_prevent_indexing() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("library");
        let scripts = temp.path().join("scripts");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&scripts).unwrap();
        let media = root.join("Silent.mkv");
        std::fs::write(&media, b"movie").unwrap();
        std::fs::write(
            root.join("Silent.en.srt"),
            "1\n00:00:01,000 --> 00:00:02,000\nHidden\n",
        )
        .unwrap();
        let mut cfg = config(no_streams_probe(&scripts), noop_ffmpeg(&scripts));
        cfg.extract_sidecars = false;
        cfg.extract_embedded = false;

        let outcome = TimedTextExtractor::new(cfg)
            .extract(request(&root, &media))
            .await
            .unwrap();

        assert!(outcome.sources.is_empty());
        assert!(outcome.skipped.is_empty());
        assert!(outcome.failures.is_empty());
    }

    #[test]
    fn indexing_config_language_allow_list_preserves_unknown_language() {
        let mut indexing = crate::types::scan::orchestration::config::TranscriptIndexingConfig::default();
        indexing.allowed_languages = vec![
            "English".to_string(),
            "eng".to_string(),
            "UND".to_string(),
            " ".to_string(),
            "fr".to_string(),
        ];

        let config = TimedTextExtractionConfig::from_indexing_config(&indexing)
            .expect("indexing config maps to extraction config");

        assert_eq!(config.allowed_languages, vec!["en", "fr", "und"]);
    }

    #[test]
    fn custom_redaction_regexes_are_compiled_from_config() {
        let mut redaction = crate::types::scan::orchestration::config::TranscriptRedactionConfig::default();
        redaction.custom_regexes = vec!["classified-[0-9]+".to_string()];
        let redactor = PrivacyTimedTextRedactor::from_config(&redaction)
            .expect("custom regex compiles");

        assert_eq!(
            redactor.redact("classified-123 stays private"),
            "[REDACTED:custom] stays private"
        );

        redaction.custom_regexes = vec!["(".to_string()];
        assert!(PrivacyTimedTextRedactor::from_config(&redaction).is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn extractor_applies_byte_and_cue_caps() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("library");
        let scripts = temp.path().join("scripts");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&scripts).unwrap();
        let media = root.join("Feature.mkv");
        std::fs::write(&media, b"movie").unwrap();
        std::fs::write(root.join("Feature.en.srt"), "x".repeat(128)).unwrap();
        let mut cfg = config(no_streams_probe(&scripts), noop_ffmpeg(&scripts));
        cfg.max_sidecar_bytes = 8;

        let outcome = TimedTextExtractor::new(cfg)
            .extract(request(&root, &media))
            .await
            .unwrap();
        assert!(outcome.sources.is_empty());
        assert_eq!(outcome.skipped.len(), 1);
        assert_eq!(
            outcome.skipped[0].kind,
            TimedTextSkipKind::ResourceLimitExceeded
        );

        std::fs::write(
            root.join("Feature.en.srt"),
            "1\n00:00:01,000 --> 00:00:02,000\nOne\n\n2\n00:00:03,000 --> 00:00:04,000\nTwo\n",
        )
        .unwrap();
        let mut cfg = config(no_streams_probe(&scripts), noop_ffmpeg(&scripts));
        cfg.max_cues_per_source = 1;
        let outcome = TimedTextExtractor::new(cfg)
            .extract(request(&root, &media))
            .await
            .unwrap();
        assert_eq!(outcome.sources[0].segments.len(), 1);
        assert_eq!(outcome.sources[0].source.metadata["truncated"], true);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn embedded_streams_convert_text_and_skip_bitmap_subtitles() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("library");
        let scripts = temp.path().join("scripts");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&scripts).unwrap();
        let media = root.join("Episode.mkv");
        std::fs::write(&media, b"movie").unwrap();
        let ffprobe = write_script(
            &scripts,
            "ffprobe-streams.sh",
            "#!/bin/sh\nprintf '{\"streams\":[{\"index\":2,\"codec_name\":\"ass\",\"codec_type\":\"subtitle\",\"tags\":{\"language\":\"eng\"}},{\"index\":3,\"codec_name\":\"hdmv_pgs_subtitle\",\"codec_type\":\"subtitle\"}]}'\n",
        );
        let ffmpeg = write_script(
            &scripts,
            "ffmpeg-srt.sh",
            "#!/bin/sh\nprintf '1\n00:00:05,000 --> 00:00:06,000\nConverted text\n'\n",
        );

        let outcome = TimedTextExtractor::new(config(ffprobe, ffmpeg))
            .extract(request(&root, &media))
            .await
            .unwrap();

        assert_eq!(outcome.sources.len(), 1);
        assert_eq!(
            outcome.sources[0].source.source_kind,
            TimedTextSourceKind::Embedded
        );
        assert_eq!(outcome.sources[0].source.stream_index, Some(2));
        assert_eq!(outcome.sources[0].source.language_code, "en");
        assert_eq!(outcome.sources[0].segments[0].text, "Converted text");
        assert_eq!(outcome.skipped.len(), 1);
        assert_eq!(
            outcome.skipped[0].kind,
            TimedTextSkipKind::UnsupportedSubtitleCodec
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn command_failures_and_timeouts_are_classified_without_paths() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("library");
        let scripts = temp.path().join("scripts");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&scripts).unwrap();
        let media = root.join("PrivateMovie.mkv");
        std::fs::write(&media, b"movie").unwrap();
        let failing_probe = write_script(
            &scripts,
            "ffprobe-fail.sh",
            "#!/bin/sh\nprintf 'raw stderr path leak' >&2\nexit 42\n",
        );

        let outcome = TimedTextExtractor::new(config(
            failing_probe,
            noop_ffmpeg(&scripts),
        ))
        .extract(request(&root, &media))
        .await
        .unwrap();
        assert_eq!(outcome.failures.len(), 1);
        assert_eq!(
            outcome.failures[0].kind,
            TimedTextFailureKind::ProbeCommandFailed
        );
        let debug = format!("{outcome:?}");
        assert!(!debug.contains("PrivateMovie"));
        assert!(!debug.contains("raw stderr"));

        let timeout_probe = write_script(
            &scripts,
            "ffprobe-sleep.sh",
            "#!/bin/sh\nsleep 2\nprintf '{\"streams\":[]}'\n",
        );
        let mut cfg = config(timeout_probe, noop_ffmpeg(&scripts));
        cfg.probe_timeout = Duration::from_millis(50);
        let outcome = TimedTextExtractor::new(cfg)
            .extract(request(&root, &media))
            .await
            .unwrap();
        assert_eq!(outcome.failures.len(), 1);
        assert_eq!(
            outcome.failures[0].kind,
            TimedTextFailureKind::ProbeTimedOut
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn converted_output_cap_is_classified() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("library");
        let scripts = temp.path().join("scripts");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&scripts).unwrap();
        let media = root.join("Movie.mkv");
        std::fs::write(&media, b"movie").unwrap();
        let ffprobe = write_script(
            &scripts,
            "ffprobe-one.sh",
            "#!/bin/sh\nprintf '{\"streams\":[{\"index\":0,\"codec_name\":\"subrip\",\"codec_type\":\"subtitle\"}]}'\n",
        );
        let ffmpeg = write_script(
            &scripts,
            "ffmpeg-large.sh",
            "#!/bin/sh\nyes X | head -c 2048\n",
        );
        let mut cfg = config(ffprobe, ffmpeg);
        cfg.max_converted_subtitle_bytes = 32;

        let outcome = TimedTextExtractor::new(cfg)
            .extract(request(&root, &media))
            .await
            .unwrap();

        assert!(outcome.sources.is_empty());
        assert_eq!(outcome.failures.len(), 1);
        assert_eq!(
            outcome.failures[0].kind,
            TimedTextFailureKind::FfmpegOutputTooLarge
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn debug_output_does_not_expose_raw_text_or_paths() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("library");
        let scripts = temp.path().join("scripts");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&scripts).unwrap();
        let media = root.join("SecretMovie.mkv");
        std::fs::write(&media, b"movie").unwrap();
        std::fs::write(
            root.join("SecretMovie.en.srt"),
            "1\n00:00:01,000 --> 00:00:02,000\nmy secret phrase\n",
        )
        .unwrap();

        let outcome = TimedTextExtractor::new(config(
            no_streams_probe(&scripts),
            noop_ffmpeg(&scripts),
        ))
        .extract(request(&root, &media))
        .await
        .unwrap();

        let debug = format!("{outcome:?}");
        assert!(!debug.contains("SecretMovie"));
        assert!(!debug.contains("my secret phrase"));
        assert!(
            !format!("{:?}", outcome.sources[0]).contains("my secret phrase")
        );
    }

    #[tokio::test]
    async fn outside_library_root_is_skipped_before_probe_or_sidecar_scan() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("library");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let media = outside.join("Movie.mkv");
        std::fs::write(&media, b"movie").unwrap();

        let outcome =
            TimedTextExtractor::new(TimedTextExtractionConfig::default())
                .extract(request(&root, &media))
                .await
                .unwrap();

        assert!(outcome.sources.is_empty());
        assert!(outcome.failures.is_empty());
        assert_eq!(outcome.skipped.len(), 1);
        assert_eq!(
            outcome.skipped[0].kind,
            TimedTextSkipKind::OutsideLibraryRoot
        );
    }

    #[tokio::test]
    async fn unsupported_media_kind_is_skipped_without_scanning() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("library");
        std::fs::create_dir_all(&root).unwrap();
        let media = root.join("Series.mkv");
        std::fs::write(&media, b"movie").unwrap();
        let mut req = request(&root, &media);
        req.media_id = MediaID::Series(SeriesID(Uuid::from_u128(4)));

        let outcome =
            TimedTextExtractor::new(TimedTextExtractionConfig::default())
                .extract(req)
                .await
                .unwrap();

        assert!(outcome.sources.is_empty());
        assert_eq!(outcome.skipped.len(), 1);
        assert_eq!(
            outcome.skipped[0].kind,
            TimedTextSkipKind::UnsupportedMediaKind
        );
    }
}
