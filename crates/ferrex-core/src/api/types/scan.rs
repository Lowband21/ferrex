use chrono::{DateTime, Utc};
use std::fmt;
use uuid::Uuid;

use crate::api::scan::IncrementalScanStatusView;
use crate::types::ids::LibraryId;
use crate::types::media_events::ScanProgressEvent;

/// Public mode for a durable library scan run.
///
/// The default `manual` mode preserves the existing user-triggered full-library
/// scan behavior, which currently maps to the internal bulk start path.
#[derive(
    Debug,
    Clone,
    Copy,
    serde::Serialize,
    serde::Deserialize,
    PartialEq,
    Eq,
    Hash,
)]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub enum ScanRunMode {
    Manual,
    Maintenance,
    Resume,
}

impl Default for ScanRunMode {
    fn default() -> Self {
        Self::Manual
    }
}

impl ScanRunMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Maintenance => "maintenance",
            Self::Resume => "resume",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "manual" | "manual_bulk" | "bulk" => Some(Self::Manual),
            "maintenance" => Some(Self::Maintenance),
            "resume" => Some(Self::Resume),
            _ => None,
        }
    }

    pub fn run_key(self, library_id: LibraryId) -> String {
        format!("library:{}:mode:{}", library_id.as_uuid(), self.as_str())
    }
}

/// Indicates whether a scan start created a new durable run or reused one.
#[derive(
    Debug,
    Clone,
    Copy,
    serde::Serialize,
    serde::Deserialize,
    PartialEq,
    Eq,
    Hash,
)]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub enum ScanStartDisposition {
    Created,
    Reused,
}

impl Default for ScanStartDisposition {
    fn default() -> Self {
        Self::Created
    }
}

/// Lifecycle state of a background scan job
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub enum ScanLifecycleStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
    Canceled,
}

impl Default for ScanLifecycleStatus {
    fn default() -> Self {
        Self::Pending
    }
}

impl ScanLifecycleStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "paused" => Some(Self::Paused),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "canceled" | "cancelled" => Some(Self::Canceled),
            _ => None,
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self, Self::Pending | Self::Running | Self::Paused)
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Canceled)
    }
}

/// Snapshot of a scan job used for dashboards and SSE updates
#[derive(Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq)))]
pub struct ScanSnapshotDto {
    pub scan_id: Uuid,
    pub library_id: LibraryId,
    pub status: ScanLifecycleStatus,
    #[serde(default)]
    pub mode: ScanRunMode,
    pub completed_items: u64,
    pub total_items: u64,
    pub retrying_items: u64,
    pub dead_lettered_items: u64,
    pub correlation_id: Uuid,
    pub idempotency_key: String,
    #[serde(default)]
    pub run_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disposition: Option<ScanStartDisposition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_path: Option<String>,
    #[cfg_attr(
        feature = "rkyv",
        rkyv(with = crate::rkyv_wrappers::DateTimeWrapper)
    )]
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(
        feature = "rkyv",
        rkyv(with = crate::rkyv_wrappers::OptionDateTime)
    )]
    pub terminal_at: Option<DateTime<Utc>>,
    pub sequence: u64,
}

impl fmt::Debug for ScanSnapshotDto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScanSnapshotDto")
            .field("scan_id", &self.scan_id)
            .field("library_id", &self.library_id)
            .field("status", &self.status)
            .field("mode", &self.mode)
            .field("completed_items", &self.completed_items)
            .field("total_items", &self.total_items)
            .field("retrying_items", &self.retrying_items)
            .field("dead_lettered_items", &self.dead_lettered_items)
            .field("current_path", &self.current_path)
            .field("started_at", &self.started_at)
            .field("terminal_at", &self.terminal_at)
            .field("sequence", &self.sequence)
            .field("correlation_id", &self.correlation_id)
            .field("idempotency_key", &self.idempotency_key)
            .field("run_key", &self.run_key)
            .field("disposition", &self.disposition)
            .finish()
    }
}

/// Response for `/active-scans` endpoints including total count
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ActiveScansResponse {
    pub scans: Vec<ScanSnapshotDto>,
    pub count: usize,
    #[serde(default)]
    pub incremental: IncrementalScanStatusView,
}

/// Response for `/scans/latest-progress` endpoint
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LatestProgressResponse {
    pub scan_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<ScanProgressEvent>,
}

/// Request body for triggering a scan start
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StartScanRequest {
    #[serde(default)]
    pub correlation_id: Option<Uuid>,
    #[serde(default)]
    pub mode: Option<ScanRunMode>,
}

impl StartScanRequest {
    pub fn effective_mode(&self) -> ScanRunMode {
        self.mode.unwrap_or_default()
    }
}

/// Request body for scan commands (pause/resume/cancel)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScanCommandRequest {
    pub scan_id: Uuid,
}

/// Acknowledge scan command operations
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScanCommandAcceptedResponse {
    pub scan_id: Uuid,
    pub correlation_id: Uuid,
    #[serde(default)]
    pub status: ScanLifecycleStatus,
    #[serde(default)]
    pub mode: ScanRunMode,
    #[serde(default)]
    pub idempotency_key: String,
    #[serde(default)]
    pub run_key: String,
    #[serde(default)]
    pub disposition: ScanStartDisposition,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_scan_request_defaults_to_manual_mode() {
        let request: StartScanRequest =
            serde_json::from_str("{}").expect("empty request decodes");

        assert_eq!(request.mode, None);
        assert_eq!(request.effective_mode(), ScanRunMode::Manual);
    }

    #[test]
    fn scan_command_response_deserializes_legacy_payload() {
        let scan_id = Uuid::now_v7();
        let correlation_id = Uuid::now_v7();
        let payload = serde_json::json!({
            "scan_id": scan_id,
            "correlation_id": correlation_id,
        });

        let response: ScanCommandAcceptedResponse =
            serde_json::from_value(payload).expect("legacy response decodes");

        assert_eq!(response.scan_id, scan_id);
        assert_eq!(response.correlation_id, correlation_id);
        assert_eq!(response.status, ScanLifecycleStatus::Pending);
        assert_eq!(response.mode, ScanRunMode::Manual);
        assert_eq!(response.idempotency_key, "");
        assert_eq!(response.run_key, "");
        assert_eq!(response.disposition, ScanStartDisposition::Created);
    }

    #[test]
    fn scan_snapshot_deserializes_legacy_payload() {
        let scan_id = Uuid::now_v7();
        let library_id = LibraryId(Uuid::now_v7());
        let correlation_id = Uuid::now_v7();
        let started_at = Utc::now();
        let payload = serde_json::json!({
            "scan_id": scan_id,
            "library_id": library_id,
            "status": "running",
            "completed_items": 0,
            "total_items": 0,
            "retrying_items": 0,
            "dead_lettered_items": 0,
            "correlation_id": correlation_id,
            "idempotency_key": "scan-event-key",
            "started_at": started_at,
            "sequence": 1,
        });

        let snapshot: ScanSnapshotDto =
            serde_json::from_value(payload).expect("legacy snapshot decodes");

        assert_eq!(snapshot.scan_id, scan_id);
        assert_eq!(snapshot.library_id, library_id);
        assert_eq!(snapshot.mode, ScanRunMode::Manual);
        assert_eq!(snapshot.run_key, "");
        assert_eq!(snapshot.disposition, None);
        assert_eq!(snapshot.idempotency_key, "scan-event-key");
    }
}

/// Re-export media scan SSE payloads for downstream clients
pub mod events {
    pub use crate::types::media_events::{
        MediaEvent, ScanEventMetadata, ScanProgressEvent,
        ScanStageLatencySummary,
    };
}
