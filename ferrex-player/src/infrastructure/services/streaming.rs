// Streaming service trait and adapter for the RUS-136 pilot

use anyhow::Result;
use async_trait::async_trait;
use ferrex_core::api_routes::{utils, v1};
use std::sync::Arc;

use crate::infrastructure::api_client::ApiClient;

#[derive(Debug, Clone)]
pub struct TranscodingStatus {
    pub job_id: String,
    pub state: String, // e.g., "pending", "running", "completed", "failed"
    pub progress: Option<f32>,
    pub message: Option<String>,
}

#[async_trait]
pub trait StreamingApiService: Send + Sync {
    async fn start_transcoding(&self, media_id: &str, profile: &str) -> Result<String>;
    async fn check_transcoding_status(&self, job_id: &str) -> Result<TranscodingStatus>;
    async fn get_master_playlist(&self, media_id: &str) -> Result<String>;
}

#[derive(Clone)]
pub struct StreamingApiAdapter {
    client: Arc<ApiClient>,
}

impl StreamingApiAdapter {
    pub fn new(client: Arc<ApiClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl StreamingApiService for StreamingApiAdapter {
    async fn start_transcoding(&self, media_id: &str, profile: &str) -> Result<String> {
        // Transcoding pipeline is temporarily unavailable; signal cached job
        let _ = profile; // profile selection is ignored for direct streaming
        Ok(format!("cached_{}", media_id))
    }

    async fn check_transcoding_status(&self, job_id: &str) -> Result<TranscodingStatus> {
        Ok(TranscodingStatus {
            job_id: job_id.to_string(),
            state: "completed".to_string(),
            progress: Some(1.0),
            message: Some("Direct streaming available".to_string()),
        })
    }

    async fn get_master_playlist(&self, media_id: &str) -> Result<String> {
        let stream_path = utils::replace_param(v1::stream::PLAY, "{id}", media_id);
        Ok(self.client.build_url(&stream_path, false))
    }
}
