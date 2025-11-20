use super::Message;
use ferrex_core::api_types::ScanProgressEvent;
use iced::Subscription;
use tokio::sync::mpsc;
use uuid::Uuid;

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::infrastructure::{adapters::ApiClientAdapter, services::api::ApiService};

use futures::stream::BoxStream;

#[derive(Debug, Clone)]
struct ScanProgressId {
    server_url: String,
    scan_id: Uuid,
    api: Arc<ApiClientAdapter>,
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
    api_service: Arc<ApiClientAdapter>,
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
        |mut state| async move {
            match state.next_event().await {
                Some(message) => Some((message, state)),
                None => None,
            }
        },
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
    api_service: Arc<ApiClientAdapter>,
}

impl ScanState {
    fn new(server_url: String, scan_id: Uuid, api_service: Arc<ApiClientAdapter>) -> Self {
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
        if msg.data.is_empty() || msg.data == "keep-alive" {
            return None;
        }

        match serde_json::from_str::<ScanProgressEvent>(&msg.data) {
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
                    "Failed to parse scan progress event for {}: {} (payload={})",
                    self.scan_id,
                    err,
                    msg.data
                );
                None
            }
        }
    }
}
