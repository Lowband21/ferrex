use super::LibraryMessage;
use ferrex_player_api::services::api::ApiService;
use ferrex_player_library::messages::scan_subscription::{
    ScanProgressId, scan_progress_stream,
};
use iced::Subscription;
use std::sync::Arc;
use uuid::Uuid;

/// Creates an Iced subscription to monitor library scan progress via SSE.
pub fn scan_progress(
    server_url: String,
    api_service: Arc<dyn ApiService>,
    scan_id: Uuid,
) -> Subscription<LibraryMessage> {
    Subscription::run_with(
        ScanProgressId::new(server_url, api_service, scan_id),
        scan_progress_stream,
    )
}
