use std::collections::HashMap;

use crate::{
    media_library::MediaFile,
    models::TvShow,
    state::{SortBy, SortOrder},
};

pub fn sort_media(
    movies: &mut Vec<MediaFile>,
    _tv_shows: &mut HashMap<String, TvShow>,
    sort_by: SortBy,
    sort_order: SortOrder,
) {
    // Sort movies
    movies.sort_by(|a, b| {
        let cmp = match sort_by {
            SortBy::DateAdded => a.created_at.cmp(&b.created_at),
            SortBy::Title => {
                let title_a = a
                    .metadata
                    .as_ref()
                    .and_then(|m| m.parsed_info.as_ref())
                    .map(|p| &p.title)
                    .unwrap_or(&a.filename);
                let title_b = b
                    .metadata
                    .as_ref()
                    .and_then(|m| m.parsed_info.as_ref())
                    .map(|p| &p.title)
                    .unwrap_or(&b.filename);
                title_a.cmp(title_b)
            }
            SortBy::Year => {
                let year_a = a
                    .metadata
                    .as_ref()
                    .and_then(|m| m.parsed_info.as_ref())
                    .and_then(|p| p.year);
                let year_b = b
                    .metadata
                    .as_ref()
                    .and_then(|m| m.parsed_info.as_ref())
                    .and_then(|p| p.year);
                year_a.cmp(&year_b)
            }
            SortBy::Rating => {
                let rating_a = a
                    .metadata
                    .as_ref()
                    .and_then(|m| m.external_info.as_ref())
                    .and_then(|e| e.rating);
                let rating_b = b
                    .metadata
                    .as_ref()
                    .and_then(|m| m.external_info.as_ref())
                    .and_then(|e| e.rating);
                rating_a
                    .partial_cmp(&rating_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }
        };

        match sort_order {
            SortOrder::Ascending => cmp,
            SortOrder::Descending => cmp.reverse(),
        }
    });

    // For TV shows, we'll sort the values when displaying them
    // since HashMap doesn't maintain order
}

pub async fn trigger_metadata_fetch(
    server_url: String,
    media_ids: Vec<String>,
) -> Result<(), String> {
    log::info!("Triggering metadata fetch for {} items", media_ids.len());

    // Use a connection pool with limited connections
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(2)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    // Process in smaller batches to avoid overwhelming the server
    for (i, media_id) in media_ids.iter().enumerate() {
        // Extract just the ID part if it's in "media:id" format
        let clean_id = if media_id.starts_with("media:") {
            media_id.strip_prefix("media:").unwrap_or(media_id)
        } else {
            media_id
        };

        let url = format!("{}/metadata/fetch/{}", server_url, clean_id);
        log::debug!("Fetching metadata from: {}", url);

        match client.post(&url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    log::info!(
                        "Metadata fetch triggered successfully for {} ({}/{})",
                        clean_id,
                        i + 1,
                        media_ids.len()
                    );
                } else {
                    log::warn!(
                        "Metadata fetch failed for {}: {}",
                        clean_id,
                        response.status()
                    );
                }
            }
            Err(e) => {
                log::error!("Failed to trigger metadata fetch for {}: {}", clean_id, e);
            }
        }

        // Small delay between requests to avoid overwhelming the server
        if i < media_ids.len() - 1 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    log::info!("Completed metadata fetch batch");
    Ok(())
}
