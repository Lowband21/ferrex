// Streaming service trait and adapter for the RUS-136 pilot

use anyhow::Result;
use async_trait::async_trait;
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
        #[derive(serde::Serialize)]
        struct StartReq<'a> { media_id: &'a str, profile: &'a str }
        #[derive(serde::Deserialize)]
        struct StartRes { job_id: String }
        let res: StartRes = self.client.post("/api/transcoding/start", &StartReq { media_id, profile }).await?;
        Ok(res.job_id)
    }

    async fn check_transcoding_status(&self, job_id: &str) -> Result<TranscodingStatus> {
        #[derive(serde::Deserialize)]
        struct StatusRes { job_id: String, state: String, progress: Option<f32>, message: Option<String> }
        let res: StatusRes = self.client.get(&format!("/api/transcoding/status/{}", job_id)).await?;
        Ok(TranscodingStatus { job_id: res.job_id, state: res.state, progress: res.progress, message: res.message })
    }

    async fn get_master_playlist(&self, media_id: &str) -> Result<String> {
        // Some endpoints in the codebase use non-versioned paths; mirror existing behavior
        // If server returns JSON-wrapped data, adjust accordingly later
        let playlist: String = self.client.get(&format!("/hls/{}/master.m3u8", media_id)).await?;
        Ok(playlist)
    }
}

