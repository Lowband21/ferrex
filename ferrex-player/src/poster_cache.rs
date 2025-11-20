use crate::performance_config;
use ::image::DynamicImage;
use iced::widget::image;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub enum PosterState {
    Loading,
    Loaded {
        thumbnail: image::Handle, // 200x300 for grid view
        full_size: image::Handle, // Original size for detail view
        opacity: f32,             // Opacity for animations (0.0 to 1.0)
    },
    Failed,
}

#[derive(Debug, Clone)]
pub struct PosterCache {
    cache: Arc<Mutex<HashMap<String, PosterState>>>,
}

impl Default for PosterCache {
    fn default() -> Self {
        Self::new()
    }
}

impl PosterCache {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn get(&self, media_id: &str) -> Option<PosterState> {
        self.cache.lock().unwrap().get(media_id).cloned()
    }

    pub fn set_loading(&self, media_id: String) {
        self.cache
            .lock()
            .unwrap()
            .insert(media_id, PosterState::Loading);
    }

    pub fn set_loaded(&self, media_id: String, thumbnail: image::Handle, full_size: image::Handle) {
        self.cache.lock().unwrap().insert(
            media_id,
            PosterState::Loaded {
                thumbnail,
                full_size,
                opacity: 1.0,
            },
        );
    }

    pub fn update_opacity(&self, media_id: &str, new_opacity: f32) {
        let mut cache = self.cache.lock().unwrap();
        if let Some(state) = cache.get(media_id).cloned() {
            if let PosterState::Loaded {
                thumbnail,
                full_size,
                ..
            } = state
            {
                cache.insert(
                    media_id.to_string(),
                    PosterState::Loaded {
                        thumbnail,
                        full_size,
                        opacity: new_opacity,
                    },
                );
            }
        }
    }

    pub fn set_failed(&self, media_id: String) {
        self.cache
            .lock()
            .unwrap()
            .insert(media_id, PosterState::Failed);
    }

    pub fn remove(&self, media_id: &str) {
        self.cache.lock().unwrap().remove(media_id);
    }

    pub fn get_failed_ids(&self) -> Vec<String> {
        self.cache
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(id, state)| match state {
                PosterState::Failed => Some(id.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn get_loading_ids(&self) -> Vec<String> {
        self.cache
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(id, state)| match state {
                PosterState::Loading => Some(id.clone()),
                _ => None,
            })
            .collect()
    }
    
    pub fn clear(&self) {
        self.cache.lock().unwrap().clear();
    }
}

pub async fn fetch_poster(server_url: String, media_id: String) -> Result<Vec<u8>, anyhow::Error> {
    // Add cache-busting parameter to force server to regenerate posters
    let cache_version = "v4"; // Increment this when poster processing changes
    let url = format!(
        "{}/poster/{}?version={}",
        server_url, media_id, cache_version
    );
    log::debug!("Fetching poster from: {}", url);

    // Use a client with timeout to prevent hanging
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let response = client.get(&url).send().await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to fetch poster: {}",
            response.status()
        ));
    }

    let bytes = response.bytes().await?;
    Ok(bytes.to_vec())
}

pub async fn fetch_poster_with_retry(
    server_url: String,
    media_id: String,
    max_retries: u32,
    retry_delay: std::time::Duration,
) -> Result<Vec<u8>, anyhow::Error> {
    let mut attempts = 0;

    loop {
        match fetch_poster(server_url.clone(), media_id.clone()).await {
            Ok(data) => return Ok(data),
            Err(e) => {
                attempts += 1;
                if attempts >= max_retries {
                    log::error!(
                        "Failed to fetch poster for {} after {} attempts: {}",
                        media_id,
                        attempts,
                        e
                    );
                    return Err(e);
                }

                log::warn!(
                    "Failed to fetch poster for {} (attempt {}/{}): {}, retrying in {:?}",
                    media_id,
                    attempts,
                    max_retries,
                    e,
                    retry_delay
                );

                // Sleep before next retry
                tokio::time::sleep(retry_delay).await;
            }
        }
    }
}

pub async fn fetch_poster_with_id(
    server_url: String,
    media_id: String,
) -> (String, Result<Vec<u8>, String>) {
    let id = media_id.clone();
    let result = fetch_poster(server_url, media_id)
        .await
        .map_err(|e| e.to_string());
    (id, result)
}
pub async fn fetch_poster_with_id_retry(
    server_url: String,
    media_id: String,
) -> (String, Result<Vec<u8>, String>) {
    let id = media_id.clone();
    let result = fetch_poster_with_retry(
        server_url,
        media_id,
        10,                                // 10 retries
        std::time::Duration::from_secs(5), // 1 second delay
    )
    .await
    .map_err(|e| e.to_string());
    (id, result)
}

/// Apply rounded corners to an image
fn apply_rounded_corners(img: &DynamicImage, corner_radius: u32) -> DynamicImage {
    let width = img.width();
    let height = img.height();

    // Ensure corner radius is reasonable for the image size
    let effective_radius = corner_radius.min(width / 2).min(height / 2);

    // Convert to RGBA if not already
    let mut rgba_img = img.to_rgba8();

    // Apply rounded corners by making pixels transparent
    for y in 0..height {
        for x in 0..width {
            let mut should_be_transparent = false;

            // Top-left corner
            if x < effective_radius && y < effective_radius {
                let dx = effective_radius as f32 - 0.5 - x as f32;
                let dy = effective_radius as f32 - 0.5 - y as f32;
                let distance = (dx * dx + dy * dy).sqrt();
                should_be_transparent = distance > effective_radius as f32;
            }
            // Top-right corner
            else if x >= width - effective_radius && y < effective_radius {
                let dx = x as f32 + 0.5 - (width - effective_radius) as f32;
                let dy = effective_radius as f32 - 0.5 - y as f32;
                let distance = (dx * dx + dy * dy).sqrt();
                should_be_transparent = distance > effective_radius as f32;
            }
            // Bottom-left corner
            else if x < effective_radius && y >= height - effective_radius {
                let dx = effective_radius as f32 - 0.5 - x as f32;
                let dy = y as f32 + 0.5 - (height - effective_radius) as f32;
                let distance = (dx * dx + dy * dy).sqrt();
                should_be_transparent = distance > effective_radius as f32;
            }
            // Bottom-right corner
            else if x >= width - effective_radius && y >= height - effective_radius {
                let dx = x as f32 + 0.5 - (width - effective_radius) as f32;
                let dy = y as f32 + 0.5 - (height - effective_radius) as f32;
                let distance = (dx * dx + dy * dy).sqrt();
                should_be_transparent = distance > effective_radius as f32;
            }

            if should_be_transparent {
                // Set alpha to 0 (transparent)
                let pixel = rgba_img.get_pixel_mut(x, y);
                pixel[3] = 0;
            } else {
                // Anti-aliasing: for pixels near the edge, calculate partial transparency
                let mut edge_distance = f32::MAX;

                // Check all four corners for edge proximity
                if x < effective_radius && y < effective_radius {
                    let dx = effective_radius as f32 - 0.5 - x as f32;
                    let dy = effective_radius as f32 - 0.5 - y as f32;
                    let distance = (dx * dx + dy * dy).sqrt();
                    edge_distance = edge_distance.min((effective_radius as f32 - distance).abs());
                } else if x >= width - effective_radius && y < effective_radius {
                    let dx = x as f32 + 0.5 - (width - effective_radius) as f32;
                    let dy = effective_radius as f32 - 0.5 - y as f32;
                    let distance = (dx * dx + dy * dy).sqrt();
                    edge_distance = edge_distance.min((effective_radius as f32 - distance).abs());
                } else if x < effective_radius && y >= height - effective_radius {
                    let dx = effective_radius as f32 - 0.5 - x as f32;
                    let dy = y as f32 + 0.5 - (height - effective_radius) as f32;
                    let distance = (dx * dx + dy * dy).sqrt();
                    edge_distance = edge_distance.min((effective_radius as f32 - distance).abs());
                } else if x >= width - effective_radius && y >= height - effective_radius {
                    let dx = x as f32 + 0.5 - (width - effective_radius) as f32;
                    let dy = y as f32 + 0.5 - (height - effective_radius) as f32;
                    let distance = (dx * dx + dy * dy).sqrt();
                    edge_distance = edge_distance.min((effective_radius as f32 - distance).abs());
                }

                // Apply anti-aliasing for pixels within 1 pixel of the edge
                if edge_distance < 1.0 {
                    let pixel = rgba_img.get_pixel_mut(x, y);
                    let alpha = (edge_distance * 255.0) as u8;
                    pixel[3] = pixel[3].min(alpha);
                }
            }
        }
    }

    DynamicImage::ImageRgba8(rgba_img)
}

/// Resize an image to create a thumbnail while preserving aspect ratio
fn create_thumbnail(
    image_bytes: &[u8],
    target_width: u32,
    target_height: u32,
) -> Result<Vec<u8>, anyhow::Error> {
    // Load the image
    let img = ::image::load_from_memory(image_bytes)?;

    // Calculate the scaling to fit within target dimensions while preserving aspect ratio
    let (orig_width, orig_height) = (img.width(), img.height());
    let width_ratio = target_width as f32 / orig_width as f32;
    let height_ratio = target_height as f32 / orig_height as f32;
    let scale_ratio = width_ratio.min(height_ratio);

    let new_width = (orig_width as f32 * scale_ratio) as u32;
    let new_height = (orig_height as f32 * scale_ratio) as u32;

    // Resize the image - use Triangle filter for faster performance
    let resized = img.resize(
        new_width,
        new_height,
        ::image::imageops::FilterType::Triangle,
    );

    // Apply rounded corners with 8px radius to match the UI theme
    // DISABLED: Testing UI-based rounding instead
    // let rounded = apply_rounded_corners(&resized, 8);

    // Convert back to bytes - use JPEG for better performance unless we need transparency
    let mut output = Vec::new();
    if resized.color().has_alpha() {
        // Only use PNG if we actually have transparency
        resized.write_to(
            &mut std::io::Cursor::new(&mut output),
            ::image::ImageFormat::Png,
        )?;
    } else {
        // Use JPEG with good quality for opaque images
        // Write as JPEG with good quality
        let mut cursor = std::io::Cursor::new(&mut output);
        resized.write_to(&mut cursor, ::image::ImageFormat::Jpeg)?;
    }

    Ok(output)
}

/// Process poster bytes to create both thumbnail and full-size handles
/// This is now async to allow background processing
pub async fn process_poster_bytes_async(
    bytes: Vec<u8>,
) -> Result<(image::Handle, image::Handle), anyhow::Error> {
    // Move heavy processing to blocking thread pool
    tokio::task::spawn_blocking(move || {
        // Create thumbnail (200x300 for grid view) with rounded corners
        let thumbnail_bytes = create_thumbnail(&bytes, 200, 300)?;

        // For full-size, skip re-encoding to save time
        // Just use the original bytes if they're a reasonable size
        let full_bytes =
            if bytes.len() < performance_config::posters::processing::MAX_FULLSIZE_BYTES {
                bytes
            } else {
                // Only re-encode if the image is too large
                let full_img = ::image::load_from_memory(&bytes)?;
                let mut output = Vec::new();
                // For now, use PNG until we figure out JPEG quality syntax
                let mut cursor = std::io::Cursor::new(&mut output);
                full_img.write_to(&mut cursor, ::image::ImageFormat::Png)?;
                output
            };

        // Create handles for both versions
        let thumbnail_handle = image::Handle::from_bytes(thumbnail_bytes);
        let full_size_handle = image::Handle::from_bytes(full_bytes);

        Ok((thumbnail_handle, full_size_handle))
    })
    .await?
}

/// Synchronous version for compatibility (delegates to async version)
pub fn process_poster_bytes(
    bytes: Vec<u8>,
) -> Result<(image::Handle, image::Handle), anyhow::Error> {
    // For now, block on the async version
    // This maintains compatibility but isn't ideal
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(process_poster_bytes_async(bytes))
    })
}
