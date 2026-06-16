// Streaming service trait and adapter for the RUS-136 pilot

use anyhow::Result;
use async_trait::async_trait;
use ferrex_core::api::routes::{utils, v1};
use std::sync::Arc;

use crate::ApiClient;

#[derive(Debug, Clone)]
pub struct TranscodingStatus {
    pub job_id: String,
    pub state: String, // e.g., "pending", "running", "completed", "failed"
    pub progress: Option<f32>,
    pub message: Option<String>,
}

#[async_trait]
pub trait StreamingApiService: Send + Sync {
    async fn start_transcoding(
        &self,
        media_id: &str,
        profile: &str,
    ) -> Result<String>;
    async fn check_transcoding_status(
        &self,
        job_id: &str,
    ) -> Result<TranscodingStatus>;
    async fn get_master_playlist(&self, media_id: &str) -> Result<String>;
}

#[derive(Clone, Debug)]
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
    async fn start_transcoding(
        &self,
        media_id: &str,
        profile: &str,
    ) -> Result<String> {
        // Transcoding provider is temporarily unavailable; signal cached job
        let _ = profile; // profile selection is ignored for direct streaming
        Ok(format!("cached_{}", media_id))
    }

    async fn check_transcoding_status(
        &self,
        job_id: &str,
    ) -> Result<TranscodingStatus> {
        Ok(TranscodingStatus {
            job_id: job_id.to_string(),
            state: "completed".to_string(),
            progress: Some(1.0),
            message: Some("Direct streaming available".to_string()),
        })
    }

    async fn get_master_playlist(&self, media_id: &str) -> Result<String> {
        let stream_path =
            utils::replace_param(v1::stream::PLAY, "{id}", media_id);
        let base = self.client.build_url(&stream_path);
        // Attach a short-lived playback ticket for HLS clients. Failing open
        // to the protected stream URL would bypass the desktop auth contract.
        #[derive(serde::Deserialize)]
        struct PlaybackTicketResponse {
            access_token: String,
            #[allow(dead_code)]
            expires_in: i64,
        }
        let ticket_path =
            utils::replace_param(v1::stream::PLAYBACK_TICKET, "{id}", media_id);
        let resp = self
            .client
            .get::<PlaybackTicketResponse>(&ticket_path)
            .await
            .map_err(|error| {
                anyhow::anyhow!(
                    "failed to resolve playback ticket for stream URL: {}",
                    error
                )
            })?;

        if resp.access_token.trim().is_empty() {
            anyhow::bail!(
                "playback ticket response did not include an access token"
            );
        }

        Ok(format!(
            "{}?access_token={}",
            base,
            urlencoding::encode(&resp.access_token)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::Duration,
    };

    fn serve_once(status: &str, body: &str) -> String {
        let listener =
            TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().expect("test server address");
        let status = status.to_string();
        let body = body.to_string();

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request);

            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
        });

        format!("http://{addr}")
    }

    #[tokio::test]
    async fn get_master_playlist_returns_ticketed_url() {
        let base_url = serve_once(
            "200 OK",
            r#"{"status":"success","data":{"access_token":"ticket secret","expires_in":60}}"#,
        );
        let adapter = StreamingApiAdapter::new(Arc::new(ApiClient::new(
            base_url.clone(),
        )));

        let playlist_url = adapter
            .get_master_playlist("media-1")
            .await
            .expect("ticket request succeeds");

        assert_eq!(
            playlist_url,
            format!(
                "{}/api/v1/stream/media-1?access_token=ticket%20secret",
                base_url
            )
        );
    }

    #[tokio::test]
    async fn get_master_playlist_fails_closed_when_ticket_request_fails() {
        let base_url = serve_once(
            "401 Unauthorized",
            r#"{"status":"error","error":"expired"}"#,
        );
        let adapter = StreamingApiAdapter::new(Arc::new(ApiClient::new(
            base_url.clone(),
        )));

        let error = adapter
            .get_master_playlist("media-1")
            .await
            .expect_err("ticket failure must not return the protected URL");

        let error = error.to_string();
        assert!(error.contains("failed to resolve playback ticket"));
        assert!(!error.contains("/api/v1/stream/media-1?"));
    }
}
