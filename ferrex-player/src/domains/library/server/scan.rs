use std::sync::Arc;

use anyhow::anyhow;
use ferrex_core::LibraryID;
use ferrex_core::api_types::{
    ActiveScansResponse, LatestProgressResponse, ScanCommandAcceptedResponse, ScanCommandRequest,
    ScanSnapshotDto, StartScanRequest,
};
use uuid::Uuid;

use crate::infrastructure::{adapters::ApiClientAdapter, services::api::ApiService};

pub async fn start_library_scan(
    client: Arc<ApiClientAdapter>,
    library_id: LibraryID,
    correlation_id: Option<Uuid>,
) -> Result<ScanCommandAcceptedResponse, anyhow::Error> {
    client
        .start_library_scan(library_id, StartScanRequest { correlation_id })
        .await
        .map_err(|e| anyhow!(e.to_string()))
}

pub async fn pause_library_scan(
    client: Arc<ApiClientAdapter>,
    library_id: LibraryID,
    scan_id: Uuid,
) -> Result<ScanCommandAcceptedResponse, anyhow::Error> {
    client
        .pause_library_scan(library_id, ScanCommandRequest { scan_id })
        .await
        .map_err(|e| anyhow!(e.to_string()))
}

pub async fn resume_library_scan(
    client: Arc<ApiClientAdapter>,
    library_id: LibraryID,
    scan_id: Uuid,
) -> Result<ScanCommandAcceptedResponse, anyhow::Error> {
    client
        .resume_library_scan(library_id, ScanCommandRequest { scan_id })
        .await
        .map_err(|e| anyhow!(e.to_string()))
}

pub async fn cancel_library_scan(
    client: Arc<ApiClientAdapter>,
    library_id: LibraryID,
    scan_id: Uuid,
) -> Result<ScanCommandAcceptedResponse, anyhow::Error> {
    client
        .cancel_library_scan(library_id, ScanCommandRequest { scan_id })
        .await
        .map_err(|e| anyhow!(e.to_string()))
}

pub async fn fetch_active_scans(
    client: Arc<ApiClientAdapter>,
) -> Result<Vec<ScanSnapshotDto>, anyhow::Error> {
    let response: ActiveScansResponse = client
        .fetch_active_scans()
        .await
        .map_err(|e| anyhow!(e.to_string()))?;
    Ok(response.scans)
}

pub async fn fetch_latest_scan_progress(
    client: Arc<ApiClientAdapter>,
    scan_id: Uuid,
) -> Result<Option<LatestProgressResponse>, anyhow::Error> {
    let response = client
        .fetch_latest_scan_progress(scan_id)
        .await
        .map_err(|e| anyhow!(e.to_string()))?;
    Ok(Some(response))
}
