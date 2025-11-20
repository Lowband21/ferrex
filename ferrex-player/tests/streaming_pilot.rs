use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use ferrex_player::infrastructure::api_client::ApiClient;
use ferrex_player::infrastructure::services::streaming::{
    StreamingApiAdapter, StreamingApiService, TranscodingStatus as PilotStatus,
};

#[async_trait]
trait Dummy: Send + Sync {}

struct MockStreamingService;

#[async_trait]
impl StreamingApiService for MockStreamingService {
    async fn start_transcoding(&self, _media_id: &str, _profile: &str) -> Result<String> {
        Ok("job123".to_string())
    }
    async fn check_transcoding_status(&self, _job_id: &str) -> Result<PilotStatus> {
        Ok(PilotStatus {
            job_id: "job123".into(),
            state: "processing".into(),
            progress: Some(0.5),
            message: None,
        })
    }
    async fn get_master_playlist(&self, _media_id: &str) -> Result<String> {
        Ok("#EXTM3U".to_string())
    }
}

#[test]
fn streaming_service_trait_object_smoke() {
    let svc: Arc<dyn StreamingApiService> = Arc::new(MockStreamingService);
    // Ensure it can be used as trait object
    let _ = svc.clone();
}

#[test]
fn adapter_constructs() {
    // Constructing the adapter should be possible with an ApiClient
    let client = ApiClient::new("http://localhost:8000".to_string());
    let _adapter = StreamingApiAdapter::new(Arc::new(client));
}
