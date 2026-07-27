// Streaming service trait and adapter for the RUS-136 pilot

use anyhow::Result;
use async_trait::async_trait;
use ferrex_core::api::routes::{utils, v1};
pub use ferrex_model::TranscodeQualityProfile;
use ferrex_model::{
    StartTranscodeRequest, TranscodeJobState, TranscodeJobStatusResponse,
};
use std::{fmt, sync::Arc};
use zeroize::Zeroizing;

use crate::ApiClient;

/// Credential-bearing stream source returned by the streaming API boundary.
///
/// The URI is required to be credential-free. The bearer header is zeroized
/// on drop and intentionally omitted from `Debug`, so HLS/direct-stream
/// callers do not need to reconstruct query-token URLs.
#[derive(Clone, PartialEq, Eq)]
pub struct StreamingPlaybackSource {
    uri: reqwest::Url,
    authorization: Zeroizing<String>,
}

impl StreamingPlaybackSource {
    /// Construct a credential-free HTTP(S) source with a bearer token.
    ///
    /// The token is moved immediately into zeroizing storage. Invalid input is
    /// rejected without including the source or token in the error.
    pub fn with_bearer_token(uri: String, token: String) -> Result<Self> {
        let token = Zeroizing::new(token);
        if token.trim().is_empty()
            || token.bytes().any(|byte| byte.is_ascii_control())
        {
            anyhow::bail!("playback ticket response was invalid");
        }

        let uri = reqwest::Url::parse(&uri)
            .map_err(|_| anyhow::anyhow!("stream URL was invalid"))?;
        if !matches!(uri.scheme(), "http" | "https")
            || !uri.username().is_empty()
            || uri.password().is_some()
            || uri.query().is_some()
            || uri.fragment().is_some()
        {
            anyhow::bail!("stream URL must be credential-free HTTP(S)");
        }

        Ok(Self {
            uri,
            authorization: Zeroizing::new(format!("Bearer {}", token.as_str())),
        })
    }

    pub fn uri(&self) -> &reqwest::Url {
        &self.uri
    }

    /// Deliberately expose the authorization value to an in-process playback
    /// transport. Callers must not log or persist the returned value.
    pub fn authorization_header(&self) -> (&'static str, &str) {
        ("Authorization", self.authorization.as_str())
    }
}

impl fmt::Debug for StreamingPlaybackSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let authority = self
            .uri
            .host_str()
            .map(|host| {
                let port = self
                    .uri
                    .port()
                    .map(|port| format!(":{port}"))
                    .unwrap_or_default();
                format!("{}://{host}{port}/<redacted>", self.uri.scheme())
            })
            .unwrap_or_else(|| "<redacted>".to_string());
        formatter
            .debug_struct("StreamingPlaybackSource")
            .field("uri", &authority)
            .field("authorization", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct TranscodingStatus {
    pub job_id: String,
    pub profile: TranscodeQualityProfile,
    pub state: String, // e.g., "pending", "running", "completed", "failed"
    pub progress: Option<f32>,
    pub message: Option<String>,
    pub playback_source: Option<StreamingPlaybackSource>,
}

#[async_trait]
pub trait StreamingApiService: Send + Sync {
    async fn start_transcoding(
        &self,
        media_id: &str,
        profile: TranscodeQualityProfile,
    ) -> Result<String>;
    async fn check_transcoding_status(
        &self,
        job_id: &str,
    ) -> Result<TranscodingStatus>;
    /// Resolve the authenticated direct source used by the legacy HLS
    /// boundary. Completed transcode jobs return their protected rendition
    /// through [`Self::check_transcoding_status`].
    async fn get_master_playlist(
        &self,
        media_id: &str,
    ) -> Result<StreamingPlaybackSource>;
}

#[derive(Clone, Debug)]
pub struct StreamingApiAdapter {
    client: Arc<ApiClient>,
}

impl StreamingApiAdapter {
    pub fn new(client: Arc<ApiClient>) -> Self {
        Self { client }
    }

    async fn authenticated_playback_source(
        &self,
        media_id: &str,
        playback_path: &str,
    ) -> Result<StreamingPlaybackSource> {
        #[derive(serde::Deserialize)]
        struct PlaybackTicketResponse {
            access_token: String,
            #[allow(dead_code)]
            expires_in: i64,
        }

        let ticket_path =
            utils::replace_param(v1::stream::PLAYBACK_TICKET, "{id}", media_id);
        let ticket = self
            .client
            .get::<PlaybackTicketResponse>(&ticket_path)
            .await
            .map_err(|error| {
                anyhow::anyhow!(
                    "failed to resolve playback ticket for stream URL: {}",
                    error
                )
            })?;
        StreamingPlaybackSource::with_bearer_token(
            self.client.build_url(playback_path),
            ticket.access_token,
        )
    }
}

#[async_trait]
impl StreamingApiService for StreamingApiAdapter {
    async fn start_transcoding(
        &self,
        media_id: &str,
        profile: TranscodeQualityProfile,
    ) -> Result<String> {
        let path = utils::replace_param(v1::transcode::START, "{id}", media_id);
        let response = self
            .client
            .post::<_, TranscodeJobStatusResponse>(
                &path,
                &StartTranscodeRequest { profile },
            )
            .await?;
        Ok(response.job_id)
    }

    async fn check_transcoding_status(
        &self,
        job_id: &str,
    ) -> Result<TranscodingStatus> {
        let path =
            utils::replace_param(v1::transcode::STATUS, "{job_id}", job_id);
        let response =
            self.client.get::<TranscodeJobStatusResponse>(&path).await?;
        let playback_source =
            match (response.state, response.playback_path.as_deref()) {
                (TranscodeJobState::Completed, Some(playback_path)) => Some(
                    self.authenticated_playback_source(
                        &response.media_id,
                        playback_path,
                    )
                    .await?,
                ),
                _ => None,
            };
        let state = match response.state {
            TranscodeJobState::Queued => "queued",
            TranscodeJobState::Running => "running",
            TranscodeJobState::Completed => "completed",
            TranscodeJobState::Failed => "failed",
        };
        Ok(TranscodingStatus {
            job_id: response.job_id,
            profile: response.profile,
            state: state.to_string(),
            progress: response.progress,
            message: response.message,
            playback_source,
        })
    }

    async fn get_master_playlist(
        &self,
        media_id: &str,
    ) -> Result<StreamingPlaybackSource> {
        let stream_path =
            utils::replace_param(v1::stream::PLAY, "{id}", media_id);
        self.authenticated_playback_source(media_id, &stream_path)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
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

    fn serve_responses(
        responses: Vec<(&str, &str)>,
    ) -> (String, mpsc::Receiver<String>) {
        let listener =
            TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().expect("test server address");
        let responses = responses
            .into_iter()
            .map(|(status, body)| (status.to_string(), body.to_string()))
            .collect::<Vec<_>>();
        let (request_tx, request_rx) = mpsc::channel();

        thread::spawn(move || {
            for (status, body) in responses {
                let (mut stream, _) =
                    listener.accept().expect("accept request");
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                let mut request = [0_u8; 8192];
                let read = stream.read(&mut request).unwrap_or_default();
                request_tx
                    .send(
                        String::from_utf8_lossy(&request[..read]).into_owned(),
                    )
                    .expect("capture request");
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write response");
            }
        });

        (format!("http://{addr}"), request_rx)
    }

    #[tokio::test]
    async fn start_transcoding_submits_the_selected_server_profile() {
        let (base_url, requests) = serve_responses(vec![(
            "200 OK",
            r#"{"status":"success","data":{"job_id":"job-7","media_id":"media-1","profile":"720p","state":"queued","progress":0.0,"message":"queued","playback_path":null}}"#,
        )]);
        let adapter =
            StreamingApiAdapter::new(Arc::new(ApiClient::new(base_url)));

        let job_id = adapter
            .start_transcoding("media-1", TranscodeQualityProfile::P720)
            .await
            .expect("start response");
        assert_eq!(job_id, "job-7");

        let request = requests.recv().expect("captured start request");
        assert!(request.starts_with("POST /api/v1/transcode/media-1 "));
        assert!(request.contains(r#"{"profile":"720p"}"#));
    }

    #[tokio::test]
    async fn completed_status_resolves_a_header_authenticated_hls_source() {
        let (base_url, requests) = serve_responses(vec![
            (
                "200 OK",
                r#"{"status":"success","data":{"job_id":"job-7","media_id":"media-1","profile":"480p","state":"completed","progress":1.0,"message":"ready","playback_path":"/api/v1/transcode/media-1/480p/index.m3u8"}}"#,
            ),
            (
                "200 OK",
                r#"{"status":"success","data":{"access_token":"scoped ticket","expires_in":60}}"#,
            ),
        ]);
        let adapter = StreamingApiAdapter::new(Arc::new(ApiClient::new(
            base_url.clone(),
        )));

        let status = adapter
            .check_transcoding_status("job-7")
            .await
            .expect("completed status");
        assert_eq!(status.profile, TranscodeQualityProfile::P480);
        assert_eq!(status.state, "completed");
        let source = status.playback_source.expect("protected HLS source");
        assert_eq!(
            source.uri().as_str(),
            format!("{base_url}/api/v1/transcode/media-1/480p/index.m3u8")
        );
        assert_eq!(
            source.authorization_header(),
            ("Authorization", "Bearer scoped ticket")
        );
        assert!(!format!("{source:?}").contains("scoped ticket"));

        let status_request = requests.recv().expect("captured status request");
        assert!(
            status_request.starts_with("GET /api/v1/transcode/jobs/job-7 ")
        );
        let ticket_request = requests.recv().expect("captured ticket request");
        assert!(
            ticket_request.starts_with("GET /api/v1/stream/media-1/ticket ")
        );
    }

    #[tokio::test]
    async fn get_master_playlist_returns_header_authenticated_source() {
        let base_url = serve_once(
            "200 OK",
            r#"{"status":"success","data":{"access_token":"ticket secret","expires_in":60}}"#,
        );
        let adapter = StreamingApiAdapter::new(Arc::new(ApiClient::new(
            base_url.clone(),
        )));

        let source = adapter
            .get_master_playlist("media-1")
            .await
            .expect("ticket request succeeds");

        assert_eq!(
            source.uri().as_str(),
            format!("{}/api/v1/stream/media-1", base_url)
        );
        assert!(source.uri().query().is_none());
        assert_eq!(
            source.authorization_header(),
            ("Authorization", "Bearer ticket secret")
        );
        let debug = format!("{source:?}");
        assert!(!debug.contains("ticket secret"));
        assert!(!debug.contains("/api/v1/stream/media-1"));
    }

    #[test]
    fn streaming_source_rejects_embedded_credentials_and_header_injection() {
        for (uri, token) in [
            (
                "https://ferrex.example/stream?access_token=url-secret",
                "header-secret",
            ),
            ("https://user@ferrex.example/stream", "header-secret"),
            (
                "https://ferrex.example/stream",
                "header-secret\r\nX-Injected: value",
            ),
        ] {
            let error = StreamingPlaybackSource::with_bearer_token(
                uri.to_string(),
                token.to_string(),
            )
            .expect_err("credential-bearing or injectable source must fail")
            .to_string();
            assert!(!error.contains("url-secret"));
            assert!(!error.contains("header-secret"));
            assert!(!error.contains("X-Injected"));
        }
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
