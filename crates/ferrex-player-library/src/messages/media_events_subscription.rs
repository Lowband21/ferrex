use crate::{
    messages::LibraryMessage, scan_dashboard::ScanDashboardRefreshReason,
};
use base64::{
    Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD,
};
use ferrex_core::{
    api::routes::v1, player_prelude::MediaEvent,
    types::events::MediaSseEventType,
};
use ferrex_player_api::{
    api_types::{Media, MediaID},
    services::api::ApiService,
};
use futures::StreamExt;
use futures::stream::{self, BoxStream};
use rkyv::{from_bytes, rancor::Error as RkyvError};
use tokio::sync::mpsc;

use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::sync::Arc;

/// Stable identifier for the media-events stream.
#[derive(Debug, Clone)]
pub struct MediaEventsId {
    server_url: String,
    api: Arc<dyn ApiService>,
}

impl MediaEventsId {
    /// Build a media-events stream identifier.
    pub fn new(server_url: String, api: Arc<dyn ApiService>) -> Self {
        Self { server_url, api }
    }
}

impl PartialEq for MediaEventsId {
    fn eq(&self, other: &Self) -> bool {
        self.server_url == other.server_url
            && Arc::ptr_eq(&self.api, &other.api)
    }
}

impl Eq for MediaEventsId {}

impl Hash for MediaEventsId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.server_url.hash(state);
        Arc::as_ptr(&self.api).hash(state);
    }
}

/// Creates a message stream for server-sent library media changes.
pub fn media_events_stream(
    id: &MediaEventsId,
) -> BoxStream<'static, LibraryMessage> {
    let server_url = id.server_url.clone();
    let api = Arc::clone(&id.api);
    Box::pin(stream::unfold(
        MediaEventState::new(server_url.to_owned(), api),
        |mut state| async move {
            state.next_event().await.map(|message| (message, state))
        },
    ))
}

/// Internal event type for channel communication
#[derive(Debug)]
enum MediaSseEvent {
    Open,
    Message(eventsource_stream::Event),
    Error(String),
    Closed,
}

/// State machine for media events SSE subscription
struct MediaEventState {
    server_url: String,
    event_receiver: Option<mpsc::UnboundedReceiver<MediaSseEvent>>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
    retry_count: u32,
    max_retries: u32,
    api_service: Arc<dyn ApiService>,
}

impl MediaEventState {
    fn new(server_url: String, api_service: Arc<dyn ApiService>) -> Self {
        Self {
            server_url,
            event_receiver: None,
            task_handle: None,
            retry_count: 0,
            max_retries: 10,
            api_service,
        }
    }

    async fn next_event(&mut self) -> Option<LibraryMessage> {
        loop {
            // Create event source if needed
            if self.event_receiver.is_none() {
                self.create_event_source().await;
            }

            // Try to get next event from channel
            if let Some(receiver) = &mut self.event_receiver {
                match receiver.recv().await {
                    Some(MediaSseEvent::Open) => {
                        log::info!(
                            "Library media events SSE connection opened"
                        );
                        self.retry_count = 0;
                        // Continue to next event
                        continue;
                    }

                    Some(MediaSseEvent::Message(msg)) => {
                        if let Some(message) = self.handle_sse_message(msg) {
                            return Some(message);
                        }
                        // If no message, continue to next event
                        continue;
                    }

                    Some(MediaSseEvent::Error(e)) => {
                        log::error!("Library media events SSE error: {}", e);
                        if self.handle_connection_error() {
                            // Max retries exceeded, stop subscription
                            return None;
                        }
                        // Otherwise, continue to retry
                        continue;
                    }

                    Some(MediaSseEvent::Closed) | None => {
                        log::warn!("Library media events SSE stream ended");
                        // Clean up task handle
                        if let Some(handle) = self.task_handle.take() {
                            handle.abort();
                        }
                        if self.handle_connection_error() {
                            // Max retries exceeded, stop subscription
                            return None;
                        }
                        // Otherwise, continue to retry
                        continue;
                    }
                }
            } else {
                // Failed to create event source after all retries
                return None;
            }
        }
    }

    async fn create_event_source(&mut self) {
        // Add exponential backoff delay for retries
        if self.retry_count > 0 {
            let delay_secs = std::cmp::min(30, 2u64.pow(self.retry_count - 1));
            log::info!(
                "Retrying media events connection after {} seconds (attempt #{})",
                delay_secs,
                self.retry_count + 1
            );
            tokio::time::sleep(std::time::Duration::from_secs(delay_secs))
                .await;
        }

        let url = format!("{}{}", self.server_url, v1::events::MEDIA);
        log::info!("Creating media events SSE connection to: {}", url);

        // Create channel for communication
        let (tx, rx) = mpsc::unbounded_channel();
        self.event_receiver = Some(rx);

        let api = Arc::clone(&self.api_service);
        // Spawn task to handle EventSource
        let task_handle = tokio::spawn(async move {
            let client = reqwest::Client::new();
            let mut request = client.get(&url);
            if let Some(token) = api.get_token().await {
                request = request.bearer_auth(token.access_token);
            }

            match reqwest_eventsource::EventSource::new(request) {
                Ok(mut event_source) => {
                    while let Some(event) = event_source.next().await {
                        let sse_event = match event {
                            Ok(reqwest_eventsource::Event::Open) => {
                                MediaSseEvent::Open
                            }
                            Ok(reqwest_eventsource::Event::Message(msg)) => {
                                MediaSseEvent::Message(msg)
                            }
                            Err(e) => MediaSseEvent::Error(e.to_string()),
                        };

                        if tx.send(sse_event).is_err() {
                            break;
                        }
                    }

                    let _ = tx.send(MediaSseEvent::Closed);
                }
                Err(err) => {
                    let _ = tx.send(MediaSseEvent::Error(err.to_string()));
                }
            }
        });

        self.task_handle = Some(task_handle);
    }

    fn handle_sse_message(
        &mut self,
        msg: eventsource_stream::Event,
    ) -> Option<LibraryMessage> {
        // Skip keepalive messages silently
        if matches!(msg.data.as_str(), "keepalive" | "keep-alive")
            || msg.data.is_empty()
        {
            log::debug!("Received media event keepalive");
            return None;
        }

        let declared_event =
            match MediaSseEventType::from_str(msg.event.as_str()) {
                Ok(event_type) => event_type,
                Err(err) => {
                    log::debug!(
                        "Unknown media event type: {} with data: {} ({})",
                        msg.event,
                        msg.data,
                        err
                    );
                    return None;
                }
            };

        log::debug!(
            "Received media event '{}' with payload of {} bytes",
            declared_event.event_name(),
            msg.data.len()
        );

        match decode_media_event(&msg.data) {
            Ok(event) => {
                let actual_type = event.sse_event_type();
                if actual_type != declared_event {
                    log::warn!(
                        "Media event type mismatch: declared {:?}, payload {:?}",
                        declared_event,
                        actual_type
                    );
                }
                self.convert_media_event(event)
            }
            Err(err) => {
                log::error!(
                    "Failed to decode media event {}: {}",
                    msg.event,
                    err
                );
                None
            }
        }
    }

    fn handle_connection_error(&mut self) -> bool {
        self.event_receiver = None;
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
        self.retry_count += 1;

        if self.retry_count > self.max_retries {
            log::error!("Max retries exceeded for media events connection");
            // Return true to indicate we should stop
            return true;
        }

        // Return false to indicate we should continue retrying
        false
    }

    fn convert_media_event(&self, event: MediaEvent) -> Option<LibraryMessage> {
        match event {
            // These events indicate we should refresh our library data
            MediaEvent::MovieAdded { movie } => {
                log::info!("Movie added: {}", movie.title.as_str());
                Some(LibraryMessage::MediaDiscovered(vec![Media::Movie(
                    Box::new(movie),
                )]))
            }
            MediaEvent::MovieBatchFinalized {
                library_id,
                batch_id,
            } => {
                log::info!(
                    "Movie batch finalized: library {} batch {}",
                    library_id,
                    batch_id
                );
                Some(LibraryMessage::FetchMovieBatch {
                    library_id,
                    batch_id,
                })
            }
            MediaEvent::SeriesAdded { series } => {
                log::info!(
                    "Discarding empty series match: {}",
                    series.title.as_str()
                );
                None
            }
            MediaEvent::SeriesBundleFinalized {
                library_id,
                series_id,
            } => {
                log::info!(
                    "Series Bundle finalized: library {} series id {}",
                    library_id,
                    series_id
                );
                Some(LibraryMessage::FetchSeriesBundle {
                    library_id,
                    series_id,
                })
            }
            // Updates require refreshing existing data
            MediaEvent::MovieUpdated { movie } => {
                log::info!(
                    "Movie updated MediaEvent received, no action taken: {}",
                    movie.title.as_str()
                );
                // Some(LibraryMessage::MediaUpdated(Media::Movie(Box::new(
                //     movie,
                // ))))
                None
            }
            MediaEvent::SeriesUpdated { series } => {
                log::info!(
                    "Series updated MediaEvent received, no action taken: {}",
                    series.title.as_str()
                );
                // Some(LibraryMessage::MediaUpdated(Media::Series(Box::new(
                //     series,
                // ))))
                None
            }

            // Deletion events
            MediaEvent::MediaDeleted { id } => {
                log::info!("Media deleted: {:?}", id);
                Some(LibraryMessage::MediaDeleted(id))
            }

            // Global scan media events refresh durable dashboard state. Per-run
            // scan SSE subscriptions still drive live progress frames.
            MediaEvent::ScanStarted { scan_id, .. } => {
                log::debug!(
                    "ScanStarted event {} refreshing durable dashboard",
                    scan_id
                );
                Some(LibraryMessage::RefreshScanDashboard(
                    ScanDashboardRefreshReason::MediaScanEvent,
                ))
            }
            MediaEvent::ScanCompleted { scan_id, .. } => {
                log::debug!(
                    "ScanCompleted event {} refreshing durable dashboard",
                    scan_id
                );
                Some(LibraryMessage::RefreshScanDashboard(
                    ScanDashboardRefreshReason::MediaScanEvent,
                ))
            }
            MediaEvent::ScanProgress { scan_id, .. } => {
                log::debug!(
                    "ScanProgress event {} refreshing durable dashboard metadata",
                    scan_id
                );
                Some(LibraryMessage::RefreshScanDashboard(
                    ScanDashboardRefreshReason::MediaScanEvent,
                ))
            }
            MediaEvent::ScanFailed { scan_id, error, .. } => {
                log::error!("Scan {} failed: {}", scan_id, error);
                Some(LibraryMessage::RefreshScanDashboard(
                    ScanDashboardRefreshReason::MediaScanEvent,
                ))
            }
        }
    }
}

fn decode_media_event(payload: &str) -> Result<MediaEvent, String> {
    if payload.trim().is_empty() {
        return Err("empty payload".to_string());
    }

    if let Ok(bytes) = BASE64_STANDARD.decode(payload.as_bytes()) {
        match from_bytes::<MediaEvent, RkyvError>(&bytes) {
            Ok(event) => return Ok(event),
            Err(err) => {
                log::warn!(
                    "Failed to decode media event from rkyv bytes: {}. Falling back to JSON",
                    err
                );
            }
        }
    }

    serde_json::from_str::<MediaEvent>(payload).map_err(|err| err.to_string())
}
impl Drop for MediaEventState {
    fn drop(&mut self) {
        // Clean up the spawned task when the state is dropped
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
    }
}

// Helper extension to convert Media to legacy MediaFile if needed
impl LibraryMessage {
    /// Create a MediaDiscovered message from media references
    pub fn media_discovered(references: Vec<Media>) -> Self {
        LibraryMessage::MediaDiscovered(references)
    }

    /// Create a MediaUpdated message from a media reference
    pub fn media_updated(reference: Media) -> Self {
        LibraryMessage::MediaUpdated(reference)
    }

    /// Create a MediaDeleted message from a media ID
    pub fn media_deleted(id: MediaID) -> Self {
        LibraryMessage::MediaDeleted(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
    use ferrex_core::player_prelude::{
        LibraryId, MediaID, MovieID, ScanEventMetadata,
    };
    use ferrex_player_api::{
        services::api::ApiService, testing::TestApiService,
    };
    use rkyv::rancor::Error as RkyvError;
    use rkyv::to_bytes;
    use std::sync::Arc;

    fn sample_event() -> MediaEvent {
        MediaEvent::MediaDeleted {
            id: MediaID::Movie(MovieID::new()),
        }
    }

    fn sample_scan_metadata(library_id: LibraryId) -> ScanEventMetadata {
        ScanEventMetadata {
            version: "1".into(),
            correlation_id: uuid::Uuid::now_v7(),
            idempotency_key: "scan-event".into(),
            library_id,
        }
    }

    #[test]
    fn decode_media_event_rkyv_roundtrip() {
        let event = sample_event();
        let bytes = to_bytes::<RkyvError>(&event).expect("serialize rkyv");
        let encoded = BASE64_STANDARD.encode(bytes.as_slice());

        let decoded = decode_media_event(&encoded).expect("decode rkyv");
        assert_eq!(decoded, event);
    }

    #[test]
    fn decode_media_event_json_fallback() {
        let event = sample_event();
        let json = serde_json::to_string(&event).expect("json encode");

        let decoded = decode_media_event(&json).expect("decode json");
        assert_eq!(decoded, event);
    }

    #[test]
    fn scan_media_events_request_dashboard_refresh() {
        let library_id = LibraryId::new();
        let scan_id = uuid::Uuid::now_v7();
        let api: Arc<dyn ApiService> = Arc::new(TestApiService::default());
        let state = MediaEventState::new("http://localhost".into(), api);

        let message = state
            .convert_media_event(MediaEvent::ScanCompleted {
                scan_id,
                metadata: sample_scan_metadata(library_id),
            })
            .expect("scan event maps to dashboard refresh");

        assert!(matches!(
            message,
            LibraryMessage::RefreshScanDashboard(
                ScanDashboardRefreshReason::MediaScanEvent
            )
        ));
    }
}
