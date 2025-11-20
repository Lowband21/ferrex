use super::Message;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use ferrex_core::player_prelude::ScanProgressEvent;
use iced::Subscription;
use rkyv::{from_bytes, rancor::Error as RkyvError};
use tokio::sync::mpsc;
use uuid::Uuid;

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::infrastructure::services::api::ApiService;

use futures::stream::BoxStream;

#[derive(Debug, Clone)]
struct ScanProgressId {
    server_url: String,
    scan_id: Uuid,
    api: Arc<dyn ApiService>,
}

impl PartialEq for ScanProgressId {
    fn eq(&self, other: &Self) -> bool {
        self.scan_id == other.scan_id
            && self.server_url == other.server_url
            && Arc::ptr_eq(&self.api, &other.api)
    }
}

impl Eq for ScanProgressId {}

impl Hash for ScanProgressId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.server_url.hash(state);
        self.scan_id.hash(state);
        Arc::as_ptr(&self.api).hash(state);
    }
}

/// Creates a subscription to monitor library scan progress via Server-Sent Events (SSE)
    pub fn scan_progress(
        server_url: String,
        api_service: Arc<dyn ApiService>,
        scan_id: Uuid,
    ) -> Subscription<Message> {
    Subscription::run_with(
        ScanProgressId {
            server_url: server_url.clone(),
            scan_id,
            api: Arc::clone(&api_service),
        },
        build_scan_subscription_stream,
    )
}

fn build_scan_subscription_stream(id: &ScanProgressId) -> BoxStream<'static, Message> {
    let server_url = id.server_url.clone();
    let scan_id = id.scan_id;
    let api = Arc::clone(&id.api);
    Box::pin(futures::stream::unfold(
        ScanState::new(server_url, scan_id, api),
        |mut state| async move { state.next_event().await.map(|message| (message, state)) },
    ))
}

#[derive(Debug)]
enum ScanEvent {
    Open,
    Message(eventsource_stream::Event),
    Error(String),
    Closed,
}

struct ScanState {
    server_url: String,
    scan_id: Uuid,
    event_receiver: Option<mpsc::UnboundedReceiver<ScanEvent>>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
    api_service: Arc<dyn ApiService>,
}

impl ScanState {
    fn new(server_url: String, scan_id: Uuid, api_service: Arc<dyn ApiService>) -> Self {
        Self {
            server_url,
            scan_id,
            event_receiver: None,
            task_handle: None,
            api_service,
        }
    }

    async fn next_event(&mut self) -> Option<Message> {
        loop {
            if self.event_receiver.is_none() {
                self.spawn_event_source();
            }

            if let Some(receiver) = &mut self.event_receiver {
                match receiver.recv().await {
                    Some(ScanEvent::Open) => {
                        log::info!("scan SSE opened for {}", self.scan_id);
                        continue;
                    }
                    Some(ScanEvent::Message(event)) => {
                        if let Some(message) = self.handle_sse_message(event) {
                            return Some(message);
                        }
                        continue;
                    }
                    Some(ScanEvent::Error(err)) => {
                        log::warn!("scan SSE error for {}: {}", self.scan_id, err);
                        self.reset_stream();
                        return None;
                    }
                    Some(ScanEvent::Closed) | None => {
                        log::info!("scan SSE closed for {}", self.scan_id);
                        self.reset_stream();
                        return None;
                    }
                }
            } else {
                return None;
            }
        }
    }

    fn spawn_event_source(&mut self) {
        let base = self.server_url.trim_end_matches('/');
        let url = format!("{}/api/v1/scan/{}/progress", base, self.scan_id);
        let (tx, rx) = mpsc::unbounded_channel();
        self.event_receiver = Some(rx);

        let api = Arc::clone(&self.api_service);
        log::info!(
            "Opening scan progress stream for {} at {}",
            self.scan_id,
            url
        );
        let handle = tokio::spawn(async move {
            use futures::StreamExt;
            let client = reqwest::Client::new();
            let mut request = client.get(&url);
            if let Some(token) = api.get_token().await {
                request = request.bearer_auth(token.access_token);
            }
            match reqwest_eventsource::EventSource::new(request) {
                Ok(mut event_source) => {
                    while let Some(event) = event_source.next().await {
                        let scan_event = match event {
                            Ok(reqwest_eventsource::Event::Open) => ScanEvent::Open,
                            Ok(reqwest_eventsource::Event::Message(msg)) => ScanEvent::Message(msg),
                            Err(e) => ScanEvent::Error(e.to_string()),
                        };

                        if tx.send(scan_event).is_err() {
                            log::warn!(
                                "Scan SSE channel closed before event could be delivered for {}",
                                url
                            );
                            break;
                        }
                    }

                    let _ = tx.send(ScanEvent::Closed);
                }
                Err(err) => {
                    let _ = tx.send(ScanEvent::Error(err.to_string()));
                }
            }
        });

        self.task_handle = Some(handle);
    }

    fn reset_stream(&mut self) {
        self.event_receiver = None;
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
    }

    fn handle_sse_message(&mut self, msg: eventsource_stream::Event) -> Option<Message> {
        if matches!(msg.data.as_str(), "keepalive" | "keep-alive") || msg.data.is_empty() {
            return None;
        }

        match decode_scan_progress_event(&msg.data, self.scan_id) {
            Ok(event) => {
                log::debug!(
                    "SSE progress event: scan={}, seq={}, status={}, completed={}/{}",
                    event.scan_id,
                    event.sequence,
                    event.status,
                    event.completed_items,
                    event.total_items
                );
                Some(Message::ScanProgressFrame(event))
            }
            Err(err) => {
                log::error!(
                    "Failed to decode scan progress event for {}: {} (payload={})",
                    self.scan_id,
                    err,
                    msg.data
                );
                None
            }
        }
    }
}

fn decode_scan_progress_event(payload: &str, scan_id: Uuid) -> Result<ScanProgressEvent, String> {
    if payload.trim().is_empty() {
        return Err("empty payload".to_string());
    }

    if let Ok(bytes) = BASE64_STANDARD.decode(payload.as_bytes()) {
        match from_bytes::<ScanProgressEvent, RkyvError>(&bytes) {
            Ok(event) => return Ok(event),
            Err(err) => {
                log::warn!(
                    "Failed to decode rkyv scan progress for {}: {}. Falling back to JSON parsing",
                    scan_id,
                    err
                );
            }
        }
    }

    serde_json::from_str::<ScanProgressEvent>(payload).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
    use chrono::Utc;
    use ferrex_core::player_prelude::{LibraryID, ScanStageLatencySummary};
    use rkyv::rancor::Error as RkyvError;
    use rkyv::to_bytes;

    fn sample_progress_event() -> ScanProgressEvent {
        ScanProgressEvent {
            version: "1.0".to_string(),
            scan_id: Uuid::now_v7(),
            library_id: LibraryID::new(),
            status: "running".to_string(),
            completed_items: 10,
            total_items: 100,
            sequence: 42,
            current_path: Some("/path".to_string()),
            path_key: Some("key".to_string()),
            p95_stage_latencies_ms: ScanStageLatencySummary {
                scan: 1,
                analyze: 2,
                index: 3,
            },
            correlation_id: Uuid::now_v7(),
            idempotency_key: "idem".to_string(),
            emitted_at: Utc::now(),
            retrying_items: Some(1),
            dead_lettered_items: Some(2),
        }
    }

    #[test]
    fn decode_scan_progress_rkyv_roundtrip() {
        let event = sample_progress_event();
        let bytes = to_bytes::<RkyvError>(&event).expect("serialize rkyv");
        let encoded = BASE64_STANDARD.encode(bytes.as_slice());

        let decoded =
            decode_scan_progress_event(&encoded, event.scan_id).expect("decode rkyv event");
        assert_eq!(decoded.version, event.version);
        assert_eq!(decoded.scan_id, event.scan_id);
        assert_eq!(decoded.library_id, event.library_id);
        assert_eq!(decoded.status, event.status);
        assert_eq!(decoded.completed_items, event.completed_items);
        assert_eq!(decoded.total_items, event.total_items);
        assert_eq!(decoded.sequence, event.sequence);
        assert_eq!(decoded.current_path, event.current_path);
        assert_eq!(decoded.path_key, event.path_key);
        assert_eq!(decoded.p95_stage_latencies_ms, event.p95_stage_latencies_ms);
        assert_eq!(decoded.correlation_id, event.correlation_id);
        assert_eq!(decoded.idempotency_key, event.idempotency_key);
        assert_eq!(decoded.retrying_items, event.retrying_items);
        assert_eq!(decoded.dead_lettered_items, event.dead_lettered_items);
        assert_eq!(decoded.emitted_at.timestamp(), event.emitted_at.timestamp());
    }

    #[test]
    fn decode_scan_progress_json_fallback() {
        let event = sample_progress_event();
        let json = serde_json::to_string(&event).expect("json encode");

        let decoded = decode_scan_progress_event(&json, event.scan_id).expect("decode json event");
        assert_eq!(decoded, event);
    }
}
