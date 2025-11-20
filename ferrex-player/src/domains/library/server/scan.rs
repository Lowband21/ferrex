use crate::infrastructure::api_types::ScanProgress;
use ferrex_core::LibraryID;

pub async fn start_media_scan(
    server_url: String,
    force_rescan: bool,
    use_streaming: bool,
) -> Result<String, anyhow::Error> {
    log::info!(
        "Starting scan for all libraries (force_rescan: {}, use_streaming: {})",
        force_rescan,
        use_streaming
    );

    // Use the new /scan/all endpoint that scans all enabled libraries
    let client = reqwest::Client::new();
    let mut url = format!("{}/scan/all", server_url);

    // Add query parameters
    let mut params = vec![];
    if force_rescan {
        params.push("force=true");
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }

    let response = client.post(&url).send().await?;

    if !response.status().is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(anyhow::anyhow!("Server returned error: {}", error_text));
    }

    let json: serde_json::Value = response.json().await?;

    // The /scan/all endpoint returns multiple scan IDs, so we'll return a summary
    if json.get("status").and_then(|s| s.as_str()) == Some("success") {
        if let Some(scans) = json.get("scans").and_then(|s| s.as_array()) {
            if !scans.is_empty() {
                // Return the first scan ID for tracking purposes
                if let Some(first_scan) = scans.first() {
                    if let Some(scan_id) = first_scan.get("scan_id").and_then(|id| id.as_str()) {
                        log::info!(
                            "Started {} scan(s), tracking scan ID: {}",
                            scans.len(),
                            scan_id
                        );
                        return Ok(scan_id.to_string());
                    }
                }
            }
        }
    }

    if let Some(error) = json.get("error").and_then(|e| e.as_str()) {
        Err(anyhow::anyhow!("Scan error: {}", error))
    } else if let Some(message) = json.get("message").and_then(|m| m.as_str()) {
        log::info!("Scan response: {}", message);
        // Return a placeholder scan ID since we're scanning multiple libraries
        Ok("all-libraries-scan".to_string())
    } else {
        Err(anyhow::anyhow!("Invalid response from server"))
    }
}

// Library-specific scan function
pub async fn start_library_scan(
    server_url: String,
    library_id: LibraryID,
    streaming: bool,
) -> Result<String, anyhow::Error> {
    log::info!(
        "Starting library scan (library_id: {}, streaming: {})",
        library_id,
        streaming
    );

    //crate::domains::media::library::scan_library(server_url, library_id, streaming).await
    Err(anyhow::anyhow!("Not implemented"))
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
