use std::sync::Arc;

use anyhow::anyhow;
use ferrex_core::player_prelude::{
    ActiveScansResponse, LatestProgressResponse, LibraryID, ScanCommandAcceptedResponse,
    ScanCommandRequest, ScanSnapshotDto, StartScanRequest,
};
use uuid::Uuid;

use crate::infrastructure::services::api::ApiService;

pub async fn start_library_scan(
    client: Arc<dyn ApiService>,
    library_id: LibraryID,
    correlation_id: Option<Uuid>,
) -> Result<ScanCommandAcceptedResponse, anyhow::Error> {
    client
        .start_library_scan(library_id, StartScanRequest { correlation_id })
        .await
        .map_err(|e| anyhow!(e.to_string()))
}

pub async fn pause_library_scan(
    client: Arc<dyn ApiService>,
    library_id: LibraryID,
    scan_id: Uuid,
) -> Result<ScanCommandAcceptedResponse, anyhow::Error> {
    client
        .pause_library_scan(library_id, ScanCommandRequest { scan_id })
        .await
        .map_err(|e| anyhow!(e.to_string()))
}

pub async fn resume_library_scan(
    client: Arc<dyn ApiService>,
    library_id: LibraryID,
    scan_id: Uuid,
) -> Result<ScanCommandAcceptedResponse, anyhow::Error> {
    client
        .resume_library_scan(library_id, ScanCommandRequest { scan_id })
        .await
        .map_err(|e| anyhow!(e.to_string()))
}

pub async fn cancel_library_scan(
    client: Arc<dyn ApiService>,
    library_id: LibraryID,
    scan_id: Uuid,
) -> Result<ScanCommandAcceptedResponse, anyhow::Error> {
    client
        .cancel_library_scan(library_id, ScanCommandRequest { scan_id })
        .await
        .map_err(|e| anyhow!(e.to_string()))
}

pub async fn fetch_active_scans(
    client: Arc<dyn ApiService>,
) -> Result<Vec<ScanSnapshotDto>, anyhow::Error> {
    let response: ActiveScansResponse = client
        .fetch_active_scans()
        .await
        .map_err(|e| anyhow!(e.to_string()))?;
    Ok(response.scans)
}

pub async fn fetch_latest_scan_progress(
    client: Arc<dyn ApiService>,
    scan_id: Uuid,
) -> Result<Option<LatestProgressResponse>, anyhow::Error> {
    let response = client
        .fetch_latest_scan_progress(scan_id)
        .await
        .map_err(|e| anyhow!(e.to_string()))?;
    Ok(Some(response))
}
