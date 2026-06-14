use crate::domains::metadata::messages::MetadataMessage;
use crate::infra::services::api::ApiService;
use base64::{
    Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD,
};
use ferrex_core::{
    api::routes::v1,
    player_prelude::ImageRequest,
    types::{ImageReadyEvent, events::ImageSseEventType},
};
use futures::StreamExt;
use futures::stream::{self, BoxStream};
use iced::Subscription;
use rkyv::{from_bytes, rancor::Error as RkyvError};
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
struct ImageEventsId {
    server_url: String,
    api: Arc<dyn ApiService>,
}

impl PartialEq for ImageEventsId {
    fn eq(&self, other: &Self) -> bool {
        self.server_url == other.server_url
            && Arc::ptr_eq(&self.api, &other.api)
    }
}

impl Eq for ImageEventsId {}

impl Hash for ImageEventsId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.server_url.hash(state);
        Arc::as_ptr(&self.api).hash(state);
    }
}

/// Subscribe to image readiness notifications (SSE).
pub fn image_events(
    server_url: String,
    api_service: Arc<dyn ApiService>,
) -> Subscription<MetadataMessage> {
    Subscription::run_with(
        ImageEventsId {
            server_url: server_url.clone(),
            api: Arc::clone(&api_service),
        },
        build_image_events_stream,
    )
}

fn build_image_events_stream(
    id: &ImageEventsId,
) -> BoxStream<'static, MetadataMessage> {
    let server_url = id.server_url.clone();
    let api = Arc::clone(&id.api);
    Box::pin(stream::unfold(
        ImageEventState::new(server_url.to_owned(), api),
        |mut state| async move {
            state.next_event().await.map(|message| (message, state))
        },
    ))
}

#[derive(Debug)]
enum ImageSseEvent {
    Open,
    Message(eventsource_stream::Event),
    Error(String),
    Closed,
}

struct ImageEventState {
    server_url: String,
    event_receiver: Option<mpsc::UnboundedReceiver<ImageSseEvent>>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
    retry_count: u32,
    max_retries: u32,
    api_service: Arc<dyn ApiService>,
}

impl ImageEventState {
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

    async fn next_event(&mut self) -> Option<MetadataMessage> {
        loop {
            if self.event_receiver.is_none() {
                self.create_event_source().await;
            }

            if let Some(receiver) = &mut self.event_receiver {
                match receiver.recv().await {
                    Some(ImageSseEvent::Open) => {
                        log::info!("Image events SSE connection opened");
                        self.retry_count = 0;
                        continue;
                    }
                    Some(ImageSseEvent::Message(msg)) => {
                        if let Some(message) = self.handle_sse_message(msg) {
                            return Some(message);
                        }
                        continue;
                    }
                    Some(ImageSseEvent::Error(e)) => {
                        log::error!("Image events SSE error: {}", e);
                        if self.handle_connection_error() {
                            return None;
                        }
                        continue;
                    }
                    Some(ImageSseEvent::Closed) | None => {
                        log::warn!("Image events SSE stream ended");
                        if let Some(handle) = self.task_handle.take() {
                            handle.abort();
                        }
                        if self.handle_connection_error() {
                            return None;
                        }
                        continue;
                    }
                }
            } else {
                return None;
            }
        }
    }

    async fn create_event_source(&mut self) {
        if self.retry_count > 0 {
            let delay_secs = std::cmp::min(30, 2u64.pow(self.retry_count - 1));
            tokio::time::sleep(std::time::Duration::from_secs(delay_secs))
                .await;
        }

        let url = format!("{}{}", self.server_url, v1::images::EVENTS);
        log::info!("Creating image events SSE connection to: {}", url);

        let (tx, rx) = mpsc::unbounded_channel();
        self.event_receiver = Some(rx);

        let api = Arc::clone(&self.api_service);
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
                                ImageSseEvent::Open
                            }
                            Ok(reqwest_eventsource::Event::Message(msg)) => {
                                ImageSseEvent::Message(msg)
                            }
                            Err(e) => ImageSseEvent::Error(e.to_string()),
                        };
                        if tx.send(sse_event).is_err() {
                            break;
                        }
                    }
                    let _ = tx.send(ImageSseEvent::Closed);
                }
                Err(err) => {
                    let _ = tx.send(ImageSseEvent::Error(err.to_string()));
                }
            }
        });

        self.task_handle = Some(task_handle);
    }

    fn handle_connection_error(&mut self) -> bool {
        self.retry_count += 1;
        if self.retry_count >= self.max_retries {
            log::error!("Max retries exceeded for image events SSE");
            return true;
        }
        self.event_receiver = None;
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
        false
    }

    fn handle_sse_message(
        &mut self,
        msg: eventsource_stream::Event,
    ) -> Option<MetadataMessage> {
        if matches!(msg.data.as_str(), "keepalive" | "keep-alive")
            || msg.data.is_empty()
        {
            return None;
        }

        let declared_event =
            ImageSseEventType::from_str(msg.event.as_str()).ok()?;
        if declared_event != ImageSseEventType::Ready {
            return None;
        }

        let event = decode_image_ready_event(&msg.data).ok()?;
        let request = ImageRequest::new(event.iid, event.imz);
        Some(MetadataMessage::ImageBlobReady(request, event.token))
    }
}

fn decode_image_ready_event(payload: &str) -> Result<ImageReadyEvent, String> {
    let decoded = BASE64_STANDARD
        .decode(payload.trim())
        .map_err(|e| format!("base64 decode: {e}"))?;
    from_bytes::<ImageReadyEvent, RkyvError>(&decoded)
        .map_err(|e| format!("rkyv decode: {e}"))
}
