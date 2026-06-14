use crate::common::messages::DomainMessage;
use crate::state::State;
use ferrex_player_library::messages::subscriptions::{
    LibraryStreamPlan, LibrarySubscriptionInputs, stream_plan,
};
use iced::Subscription;
use std::collections::HashSet;
use std::sync::Arc;

/// Creates all library-related subscriptions.
///
/// The library data crate plans dependency-light stream descriptors and this UI
/// adapter turns those descriptors into concrete Iced subscriptions.
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    if state.server_url.is_empty() {
        return Subscription::none();
    }

    let mut scan_ids: HashSet<_> = state
        .domains
        .library
        .state
        .active_scans
        .keys()
        .copied()
        .collect();
    scan_ids
        .extend(state.domains.library.state.latest_progress.keys().copied());

    let plans = stream_plan(LibrarySubscriptionInputs {
        load_state: state.domains.library.state.load_state.clone(),
        server_url: state.server_url.clone(),
        active_scan_ids: scan_ids,
        api_service: Arc::clone(&state.api_service),
    });

    Subscription::batch(plans.into_iter().map(|plan| {
        match plan {
            LibraryStreamPlan::MediaEvents {
                server_url,
                api_service,
            } => super::media_events_subscription::media_events(
                server_url,
                api_service,
            )
            .map(DomainMessage::Library),
            LibraryStreamPlan::ScanProgress {
                server_url,
                api_service,
                scan_id,
            } => super::scan_subscription::scan_progress(
                server_url,
                api_service,
                scan_id,
            )
            .map(DomainMessage::Library),
        }
    }))
}
