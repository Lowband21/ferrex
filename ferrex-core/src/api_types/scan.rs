use chrono::{DateTime, Utc};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

use crate::types::ids::LibraryID;
use crate::types::media_events::ScanProgressEvent;

/// Lifecycle state of a background scan job
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[serde(rename_all = "snake_case")]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub enum ScanLifecycleStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
    Canceled,
}

/// Snapshot of a scan job used for dashboards and SSE updates
#[derive(Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
#[rkyv(derive(Debug, PartialEq))]
pub struct ScanSnapshotDto {
    pub scan_id: Uuid,
    pub library_id: LibraryID,
    pub status: ScanLifecycleStatus,
    pub completed_items: u64,
    pub total_items: u64,
    pub retrying_items: u64,
    pub dead_lettered_items: u64,
    pub correlation_id: Uuid,
    pub idempotency_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_path: Option<String>,
    #[rkyv(with = crate::rkyv_wrappers::DateTimeWrapper)]
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[rkyv(with = crate::rkyv_wrappers::OptionDateTime)]
    pub terminal_at: Option<DateTime<Utc>>,
    pub sequence: u64,
}

impl fmt::Debug for ScanSnapshotDto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScanSnapshotDto")
            .field("scan_id", &self.scan_id)
            .field("library_id", &self.library_id)
            .field("status", &self.status)
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
            .finish()
    }
}

/// Response for `/active-scans` endpoints including total count
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveScansResponse {
    pub scans: Vec<ScanSnapshotDto>,
    pub count: usize,
}

/// Response for `/scans/latest-progress` endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestProgressResponse {
    pub scan_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<ScanProgressEvent>,
}

/// Request body for triggering a scan start
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartScanRequest {
    #[serde(default)]
    pub correlation_id: Option<Uuid>,
}

/// Request body for scan commands (pause/resume/cancel)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanCommandRequest {
    pub scan_id: Uuid,
}

/// Acknowledge scan command operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanCommandAcceptedResponse {
    pub scan_id: Uuid,
    pub correlation_id: Uuid,
}

/// Re-export media scan SSE payloads for downstream clients
pub mod events {
    pub use crate::types::media_events::{
        MediaEvent, ScanEventMetadata, ScanProgressEvent, ScanStageLatencySummary,
    };
}
