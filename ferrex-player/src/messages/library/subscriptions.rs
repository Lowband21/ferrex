use crate::messages::DomainMessage;
use crate::state::State;
use iced::Subscription;

/// Creates all library-related subscriptions
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    let mut subscriptions = vec![];

    // Subscribe to scan progress if we have an active scan
    if let Some(scan_id) = &state.active_scan_id {
        //log::info!("Creating scan progress subscription for scan ID: {}", scan_id);
        subscriptions.push(
            super::scan_subscription::scan_progress(state.server_url.clone(), scan_id.clone())
                .map(DomainMessage::Library),
        );
    } else {
        //log::debug!("No active scan ID, not creating scan progress subscription");
    }

    // Subscribe to media events SSE stream
    if !state.server_url.is_empty() {
        subscriptions.push(
            super::media_events_subscription::media_events(state.server_url.clone())
                .map(DomainMessage::Library),
        );
    }

    Subscription::batch(subscriptions)
}
