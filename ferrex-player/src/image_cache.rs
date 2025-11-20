use iced::widget::image;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub enum ImageState {
    Loading,
    Loaded(image::Handle),
    Failed,
}

/// A generic image cache that can load images from various sources
#[derive(Debug, Clone)]
pub struct ImageCache {
    cache: Arc<Mutex<HashMap<String, ImageState>>>,
}

impl Default for ImageCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageCache {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn get(&self, key: &str) -> Option<ImageState> {
        self.cache.lock().unwrap().get(key).cloned()
    }

    pub fn set_loading(&self, key: String) {
        self.cache.lock().unwrap().insert(key, ImageState::Loading);
    }

    pub fn set_loaded(&self, key: String, handle: image::Handle) {
        self.cache
            .lock()
            .unwrap()
            .insert(key, ImageState::Loaded(handle));
    }

    pub fn set_failed(&self, key: String) {
        self.cache.lock().unwrap().insert(key, ImageState::Failed);
    }
}

/// Fetch an image from a URL
pub async fn fetch_image_from_url(url: String) -> Result<Vec<u8>, anyhow::Error> {
    log::info!("Fetching image from URL: {}", url);

    let response = reqwest::get(&url).await?;

    if !response.status().is_success() {
        log::warn!("Failed to fetch image: {} - {}", url, response.status());
        return Err(anyhow::anyhow!(
            "Failed to fetch image: {}",
            response.status()
        ));
    }

    let bytes = response.bytes().await?;
    Ok(bytes.to_vec())
}

/// Fetch a poster from the server
pub async fn fetch_poster(server_url: String, media_id: String) -> Result<Vec<u8>, anyhow::Error> {
    let url = format!("{}/poster/{}", server_url, media_id);
    fetch_image_from_url(url).await
}

/// Fetch a thumbnail from the server
pub async fn fetch_thumbnail(
    server_url: String,
    media_id: String,
) -> Result<Vec<u8>, anyhow::Error> {
    let url = format!("{}/thumbnail/{}", server_url, media_id);
    fetch_image_from_url(url).await
}

/// Image source specification
#[derive(Debug, Clone)]
pub enum ImageSource {
    /// Direct URL (e.g., TMDB URL)
    Url(String),
    /// Server poster endpoint
    ServerPoster {
        server_url: String,
        media_id: String,
    },
    /// Server thumbnail endpoint
    ServerThumbnail {
        server_url: String,
        media_id: String,
    },
}

impl ImageSource {
    /// Get a unique cache key for this image source
    pub fn cache_key(&self) -> String {
        match self {
            ImageSource::Url(url) => url.clone(),
            ImageSource::ServerPoster {
                server_url: _,
                media_id,
            } => format!("poster:{}", media_id),
            ImageSource::ServerThumbnail {
                server_url: _,
                media_id,
            } => format!("thumbnail:{}", media_id),
        }
    }
}

/// Fetch an image from any source
pub async fn fetch_image(source: ImageSource) -> Result<Vec<u8>, anyhow::Error> {
    match source {
        ImageSource::Url(url) => fetch_image_from_url(url).await,
        ImageSource::ServerPoster {
            server_url,
            media_id,
        } => fetch_poster(server_url, media_id).await,
        ImageSource::ServerThumbnail {
            server_url,
            media_id,
        } => fetch_thumbnail(server_url, media_id).await,
    }
}

/// Fetch an image and return it with its cache key
pub async fn fetch_image_with_key(source: ImageSource) -> (String, Result<Vec<u8>, String>) {
    let key = source.cache_key();
    let result = fetch_image(source).await.map_err(|e| e.to_string());
    (key, result)
}
