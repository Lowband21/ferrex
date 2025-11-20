//! Image loader implementation with HTTP client and retry logic

use super::{ImageLoader, ImagePipelineError, LoadProgress, LoadStage, Result};
use reqwest::Client;
use std::time::Duration;

/// HTTP-based image loader with connection pooling and retry logic
pub struct HttpImageLoader {
    client: Client,
    max_retries: u32,
    timeout: Duration,
}

impl HttpImageLoader {
    /// Create a new HTTP image loader
    pub fn new() -> Self {
        let client = Client::builder()
            .pool_max_idle_per_host(10)
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            max_retries: 3,
            timeout: Duration::from_secs(30),
        }
    }

    /// Create with custom configuration
    pub fn with_config(max_retries: u32, timeout: Duration) -> Self {
        let client = Client::builder()
            .pool_max_idle_per_host(10)
            .timeout(timeout)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            max_retries,
            timeout,
        }
    }
}

#[async_trait::async_trait]
impl ImageLoader for HttpImageLoader {
    async fn load(&self, url: &str) -> Result<Vec<u8>> {
        self.load_with_progress(url, |_| {}).await
    }

    async fn load_with_progress<F>(&self, url: &str, progress: F) -> Result<Vec<u8>>
    where
        F: Fn(LoadProgress) + Send + Sync,
    {
        let mut last_error = None;

        for attempt in 0..self.max_retries {
            if attempt > 0 {
                // Exponential backoff
                let delay = Duration::from_millis(100 * 2u64.pow(attempt));
                tokio::time::sleep(delay).await;
            }

            progress(LoadProgress {
                downloaded: 0,
                total: None,
                stage: LoadStage::Starting,
            });

            match self.load_internal(url, &progress).await {
                Ok(data) => {
                    progress(LoadProgress {
                        downloaded: data.len() as u64,
                        total: Some(data.len() as u64),
                        stage: LoadStage::Complete,
                    });
                    return Ok(data);
                }
                Err(e) => {
                    log::warn!("Image load attempt {} failed: {}", attempt + 1, e);
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| ImagePipelineError::Network("Unknown error".to_string())))
    }

    fn supports_url(&self, url: &str) -> bool {
        url.starts_with("http://") || url.starts_with("https://")
    }
}

impl HttpImageLoader {
    async fn load_internal<F>(&self, url: &str, progress: &F) -> Result<Vec<u8>>
    where
        F: Fn(LoadProgress) + Send + Sync,
    {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| ImagePipelineError::Network(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ImagePipelineError::Network(format!(
                "HTTP {}: {}",
                response.status(),
                url
            )));
        }

        let total_size = response.content_length();

        progress(LoadProgress {
            downloaded: 0,
            total: total_size,
            stage: LoadStage::Downloading,
        });

        let bytes = response
            .bytes()
            .await
            .map_err(|e| ImagePipelineError::Network(e.to_string()))?;

        Ok(bytes.to_vec())
    }
}
