use super::{LibraryId, Media, MediaID, MovieBatchId, MovieReference, Series};

use crate::{
    SeriesID, SubjectKey,
    chrono::{DateTime, Utc},
};

use std::fmt;
use uuid::Uuid;

#[cfg(feature = "rkyv")]
use crate::rkyv_wrappers::DateTimeWrapper;

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct ScanStageLatencySummary {
    pub scan: u64,
    pub analyze: u64,
    pub index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
/// User-facing classification for path-level scan progress reasons.
pub enum ScanPathReasonCategory {
    KnownUnchanged,
    Skipped,
    Retrying,
    NeedsAttention,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
/// User-facing detail for one path that was unchanged, skipped, retried, or needs attention.
pub struct ScanPathReasonDetail {
    pub category: ScanPathReasonCategory,
    pub reason_code: String,
    #[cfg_attr(
        feature = "serde",
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub message: Option<String>,
    #[cfg_attr(
        feature = "serde",
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub path: Option<String>,
    #[cfg_attr(
        feature = "serde",
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub path_key: Option<SubjectKey>,
    pub retryable: bool,
    #[cfg_attr(
        feature = "serde",
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub action_hint: Option<String>,
}

#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq)))]
pub struct ScanProgressEvent {
    pub version: String,
    pub scan_id: Uuid,
    pub library_id: LibraryId,
    pub status: String,
    pub completed_items: u64,
    pub total_items: u64,
    pub validated_items: u64,
    pub known_unchanged_items: u64,
    pub skipped_items: u64,
    pub failed_items: u64,
    pub needs_attention_items: u64,
    pub retrying_items: u64,
    pub sequence: u64,
    #[cfg_attr(
        feature = "serde",
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub current_path: Option<String>,
    #[cfg_attr(
        feature = "serde",
        serde(skip_serializing_if = "Option::is_none")
    )]
    pub path_key: Option<SubjectKey>,
    pub p95_stage_latencies_ms: ScanStageLatencySummary,
    pub correlation_id: Uuid,
    pub idempotency_key: String,
    #[cfg_attr(feature = "rkyv", rkyv(with = DateTimeWrapper))]
    pub emitted_at: DateTime<Utc>,
    #[cfg_attr(
        feature = "serde",
        serde(skip_serializing_if = "Option::is_none")
    )]
    #[cfg_attr(feature = "rkyv", rkyv(with = crate::rkyv_wrappers::OptionDateTime))]
    pub terminal_at: Option<DateTime<Utc>>,
    #[cfg_attr(
        feature = "serde",
        serde(skip_serializing_if = "Vec::is_empty")
    )]
    pub reason_details: Vec<ScanPathReasonDetail>,
}

impl fmt::Debug for ScanProgressEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScanProgressEvent")
            .field("scan_id", &self.scan_id)
            .field("library_id", &self.library_id)
            .field("status", &self.status)
            .field("completed_items", &self.completed_items)
            .field("total_items", &self.total_items)
            .field("validated_items", &self.validated_items)
            .field("known_unchanged_items", &self.known_unchanged_items)
            .field("skipped_items", &self.skipped_items)
            .field("failed_items", &self.failed_items)
            .field("needs_attention_items", &self.needs_attention_items)
            .field("retrying_items", &self.retrying_items)
            .field("sequence", &self.sequence)
            .field("current_path", &self.current_path)
            .field("reason_details", &self.reason_details)
            .field("correlation_id", &self.correlation_id)
            .field("idempotency_key", &self.idempotency_key)
            .field("p95_stage_latencies_ms", &self.p95_stage_latencies_ms)
            .field("emitted_at", &self.emitted_at)
            .field("terminal_at", &self.terminal_at)
            .finish()
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for ScanProgressEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct Wire {
            version: String,
            scan_id: Uuid,
            library_id: LibraryId,
            status: String,
            completed_items: u64,
            total_items: u64,
            #[serde(default)]
            validated_items: u64,
            #[serde(default)]
            known_unchanged_items: u64,
            #[serde(default)]
            skipped_items: u64,
            #[serde(default)]
            failed_items: u64,
            #[serde(default)]
            needs_attention_items: u64,
            // Deprecated compatibility input for pre-v2 JSON clients only.
            // Remove this alias after 2026-09-30; new serializers never emit it.
            #[serde(default, rename = "dead_lettered_items")]
            legacy_dead_lettered_items: Option<u64>,
            #[serde(default)]
            retrying_items: u64,
            sequence: u64,
            #[serde(default)]
            current_path: Option<String>,
            #[serde(default)]
            path_key: Option<SubjectKey>,
            p95_stage_latencies_ms: ScanStageLatencySummary,
            correlation_id: Uuid,
            idempotency_key: String,
            emitted_at: DateTime<Utc>,
            #[serde(default)]
            terminal_at: Option<DateTime<Utc>>,
            #[serde(default)]
            reason_details: Vec<ScanPathReasonDetail>,
        }

        let wire = <Wire as serde::Deserialize>::deserialize(deserializer)?;
        let attention_items = wire
            .failed_items
            .max(wire.needs_attention_items)
            .max(wire.legacy_dead_lettered_items.unwrap_or(0));
        let has_breakdown = wire.validated_items > 0
            || wire.known_unchanged_items > 0
            || wire.skipped_items > 0;
        let validated_items = if has_breakdown {
            wire.validated_items
        } else {
            wire.completed_items
        };

        Ok(Self {
            version: wire.version,
            scan_id: wire.scan_id,
            library_id: wire.library_id,
            status: wire.status,
            completed_items: wire.completed_items,
            total_items: wire.total_items,
            validated_items,
            known_unchanged_items: wire.known_unchanged_items,
            skipped_items: wire.skipped_items,
            failed_items: attention_items,
            needs_attention_items: attention_items,
            retrying_items: wire.retrying_items,
            sequence: wire.sequence,
            current_path: wire.current_path,
            path_key: wire.path_key,
            p95_stage_latencies_ms: wire.p95_stage_latencies_ms,
            correlation_id: wire.correlation_id,
            idempotency_key: wire.idempotency_key,
            emitted_at: wire.emitted_at,
            terminal_at: wire.terminal_at,
            reason_details: wire.reason_details,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq)))]
pub struct ScanEventMetadata {
    pub version: String,
    pub correlation_id: Uuid,
    pub idempotency_key: String,
    pub library_id: LibraryId,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "serde", serde(tag = "type", rename_all = "snake_case"))]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq)))]
pub enum MediaEvent {
    MovieAdded {
        movie: MovieReference,
    },
    MovieBatchFinalized {
        library_id: LibraryId,
        batch_id: MovieBatchId,
    },
    SeriesAdded {
        series: Series,
    },
    SeriesBundleFinalized {
        library_id: LibraryId,
        series_id: SeriesID,
    },
    MovieUpdated {
        movie: MovieReference,
    },
    SeriesUpdated {
        series: Series,
    },

    MediaDeleted {
        id: MediaID,
    },

    ScanStarted {
        scan_id: Uuid,
        metadata: ScanEventMetadata,
    },
    ScanProgress {
        scan_id: Uuid,
        progress: ScanProgressEvent,
    },
    ScanCompleted {
        scan_id: Uuid,
        metadata: ScanEventMetadata,
    },
    ScanFailed {
        scan_id: Uuid,
        error: String,
        metadata: ScanEventMetadata,
    },
}

impl MediaEvent {
    pub fn into_media(self) -> Option<Media> {
        match self {
            MediaEvent::MovieAdded { movie }
            | MediaEvent::MovieUpdated { movie } => {
                Some(Media::Movie(Box::new(movie)))
            }
            MediaEvent::MovieBatchFinalized { .. } => None,
            MediaEvent::SeriesBundleFinalized { .. } => None,
            MediaEvent::SeriesAdded { series }
            | MediaEvent::SeriesUpdated { series } => {
                Some(Media::Series(Box::new(series)))
            }
            MediaEvent::MediaDeleted { .. }
            | MediaEvent::ScanStarted { .. }
            | MediaEvent::ScanProgress { .. }
            | MediaEvent::ScanCompleted { .. }
            | MediaEvent::ScanFailed { .. } => None,
        }
    }
}
