use chrono::{DateTime, Utc};
use std::fmt;
use uuid::Uuid;

use crate::api::scan::IncrementalScanStatusView;
use crate::types::ids::LibraryId;
use ferrex_model::{ScanPathReasonDetail, ScanProgressEvent};

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

/// Lifecycle state of a background scan job.
///
/// Manifest-grade reconciliation uses `domain::scan::manifest::ManifestRunStatus`
/// for root/partition runs; this DTO remains the API snapshot status for active
/// scan dashboards and SSE updates.
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
#[derive(Clone, serde::Serialize, PartialEq)]
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
    pub validated_items: u64,
    pub known_unchanged_items: u64,
    pub skipped_items: u64,
    pub failed_items: u64,
    pub needs_attention_items: u64,
    pub retrying_items: u64,
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reason_details: Vec<ScanPathReasonDetail>,
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
            .field("validated_items", &self.validated_items)
            .field("known_unchanged_items", &self.known_unchanged_items)
            .field("skipped_items", &self.skipped_items)
            .field("failed_items", &self.failed_items)
            .field("needs_attention_items", &self.needs_attention_items)
            .field("retrying_items", &self.retrying_items)
            .field("current_path", &self.current_path)
            .field("started_at", &self.started_at)
            .field("terminal_at", &self.terminal_at)
            .field("sequence", &self.sequence)
            .field("correlation_id", &self.correlation_id)
            .field("idempotency_key", &self.idempotency_key)
            .field("run_key", &self.run_key)
            .field("disposition", &self.disposition)
            .field("reason_details", &self.reason_details)
            .finish()
    }
}

impl<'de> serde::Deserialize<'de> for ScanSnapshotDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct Wire {
            scan_id: Uuid,
            library_id: LibraryId,
            status: ScanLifecycleStatus,
            #[serde(default)]
            mode: ScanRunMode,
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
            correlation_id: Uuid,
            idempotency_key: String,
            #[serde(default)]
            run_key: String,
            #[serde(default)]
            disposition: Option<ScanStartDisposition>,
            #[serde(default)]
            current_path: Option<String>,
            started_at: DateTime<Utc>,
            #[serde(default)]
            terminal_at: Option<DateTime<Utc>>,
            sequence: u64,
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
            scan_id: wire.scan_id,
            library_id: wire.library_id,
            status: wire.status,
            mode: wire.mode,
            completed_items: wire.completed_items,
            total_items: wire.total_items,
            validated_items,
            known_unchanged_items: wire.known_unchanged_items,
            skipped_items: wire.skipped_items,
            failed_items: attention_items,
            needs_attention_items: attention_items,
            retrying_items: wire.retrying_items,
            correlation_id: wire.correlation_id,
            idempotency_key: wire.idempotency_key,
            run_key: wire.run_key,
            disposition: wire.disposition,
            current_path: wire.current_path,
            started_at: wire.started_at,
            terminal_at: wire.terminal_at,
            sequence: wire.sequence,
            reason_details: wire.reason_details,
        })
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
    pub use ferrex_model::{
        MediaEvent, ScanEventMetadata, ScanPathReasonCategory,
        ScanPathReasonDetail, ScanProgressEvent, ScanStageLatencySummary,
    };
}

#[cfg(test)]
mod snapshot_serde_tests {
    use super::*;
    use ferrex_model::{ScanPathReasonCategory, ScanPathReasonDetail};

    fn fixed_time() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_700_000_000, 0)
            .expect("valid fixed timestamp")
    }

    fn sample_snapshot() -> ScanSnapshotDto {
        let scan_id = Uuid::now_v7();
        let library_id = LibraryId::new();
        ScanSnapshotDto {
            scan_id,
            library_id,
            status: ScanLifecycleStatus::Running,
            mode: ScanRunMode::Manual,
            completed_items: 7,
            total_items: 10,
            validated_items: 4,
            known_unchanged_items: 2,
            skipped_items: 1,
            failed_items: 2,
            needs_attention_items: 2,
            retrying_items: 1,
            correlation_id: scan_id,
            idempotency_key: "scan:test:7".to_string(),
            run_key: ScanRunMode::Manual.run_key(library_id),
            disposition: Some(ScanStartDisposition::Created),
            current_path: Some("/library/movie".to_string()),
            started_at: fixed_time(),
            terminal_at: Some(fixed_time()),
            sequence: 7,
            reason_details: vec![ScanPathReasonDetail {
                category: ScanPathReasonCategory::NeedsAttention,
                reason_code: "permission_denied".to_string(),
                message: Some(
                    "Review this path and rescan when it is ready".to_string(),
                ),
                path: Some("/library/movie".to_string()),
                path_key: None,
                retryable: false,
                action_hint: Some("rescan_library".to_string()),
            }],
        }
    }

    #[test]
    fn snapshot_json_uses_safe_fields_without_legacy_alias() {
        let snapshot = sample_snapshot();
        let json = serde_json::to_string(&snapshot).expect("encode snapshot");

        assert!(json.contains("needs_attention_items"));
        assert!(json.contains("reason_details"));
        assert!(!json.contains("dead_lettered_items"));

        let decoded: ScanSnapshotDto =
            serde_json::from_str(&json).expect("decode snapshot");
        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn snapshot_legacy_json_maps_attention_alias() {
        let snapshot = sample_snapshot();
        let completed_items = snapshot.completed_items;
        let mut value = serde_json::to_value(snapshot).expect("snapshot json");
        let object = value.as_object_mut().expect("snapshot object");
        object.remove("validated_items");
        object.remove("known_unchanged_items");
        object.remove("skipped_items");
        object.remove("failed_items");
        object.remove("needs_attention_items");
        object.remove("reason_details");
        object.insert("dead_lettered_items".to_string(), serde_json::json!(6));

        let decoded: ScanSnapshotDto =
            serde_json::from_value(value).expect("decode legacy snapshot");
        assert_eq!(decoded.validated_items, completed_items);
        assert_eq!(decoded.failed_items, 6);
        assert_eq!(decoded.needs_attention_items, 6);
    }
}
