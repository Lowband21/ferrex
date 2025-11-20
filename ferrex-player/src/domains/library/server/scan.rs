use std::sync::Arc;

use anyhow::anyhow;
use ferrex_core::{LibraryID, ScanProgress, ScanStatus};
use uuid::Uuid;

use crate::{
    infrastructure::{adapters::ApiClientAdapter, services::api::ApiService},
    state_refactored::State,
};

pub async fn start_scan_all_libraries(
    client: Arc<ApiClientAdapter>,
    force_rescan: bool,
) -> Result<Uuid, anyhow::Error> {
    match client.scan_all_libraries(false).await {
        Ok(scan_response) => match (scan_response.status, scan_response.scan_id) {
            (ScanStatus::Scanning, Some(scan_id)) => Ok(scan_id),
            (ScanStatus::Pending, Some(scan_id)) => Ok(scan_id),
            (ScanStatus::Completed, _) => Err(anyhow!("Scan already completed")),
            (ScanStatus::Failed, _) => Err(anyhow!("Scan failed")),
            (ScanStatus::Cancelled, _) => Err(anyhow!("Scan cancelled")),
            (_, _) => Err(anyhow!(
                "Scan ID not found for Scanning or Pending scan status"
            )),
        },
        Err(e) => Err(anyhow!(e.to_string())),
    }
}

// Library-specific scan function
pub async fn start_scan_library(
    client: Arc<ApiClientAdapter>,
    library_id: LibraryID,
    force_rescan: bool,
) -> Result<Uuid, anyhow::Error> {
    log::info!("Starting library scan library_id: {}", library_id,);
    match client.scan_library(library_id, false).await {
        Ok(scan_response) => match (scan_response.status, scan_response.scan_id) {
            (ScanStatus::Scanning, Some(scan_id)) => Ok(scan_id),
            (ScanStatus::Pending, Some(scan_id)) => Ok(scan_id),
            (ScanStatus::Completed, _) => Err(anyhow!("Scan already completed")),
            (ScanStatus::Failed, _) => Err(anyhow!("Scan failed")),
            (ScanStatus::Cancelled, _) => Err(anyhow!("Scan cancelled")),
            (_, _) => Err(anyhow!(
                "Scan ID not found for Scanning or Pending scan status"
            )),
        },
        Err(e) => Err(anyhow!(e.to_string())),
    }
}

pub async fn check_active_scans(server_url: String) -> Vec<ScanProgress> {
    match reqwest::get(format!("{}/scan/active", server_url)).await {
        Ok(response) => match response.json::<serde_json::Value>().await {
            Ok(json) => {
                if let Some(scans) = json.get("scans").and_then(|s| s.as_array()) {
                    scans
                        .iter()
                        .filter_map(|scan| {
                            serde_json::from_value::<ScanProgress>(scan.clone()).ok()
                        })
                        .collect()
                } else {
                    vec![]
                }
            }
            Err(e) => {
                log::error!("Failed to parse active scans response: {}", e);
                vec![]
            }
        },
        Err(e) => {
            log::error!("Failed to check active scans: {}", e);
            vec![]
        }
    }
}
