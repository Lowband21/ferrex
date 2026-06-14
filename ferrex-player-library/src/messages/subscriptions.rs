use crate::LibrariesLoadState;
use ferrex_player_api::services::api::ApiService;
use std::{collections::HashSet, sync::Arc};
use uuid::Uuid;

/// Explicit data needed by library stream planning without importing an app root state.
#[derive(Clone, Debug)]
pub struct LibrarySubscriptionInputs {
    pub load_state: LibrariesLoadState,
    pub active_scan_ids: HashSet<Uuid>,
    pub server_url: String,
    pub api_service: Arc<dyn ApiService>,
}

/// Dependency-light stream descriptors emitted by the library domain.
#[derive(Clone, Debug)]
pub enum LibraryStreamPlan {
    /// Subscribe to global media change events for the active server.
    MediaEvents {
        server_url: String,
        api_service: Arc<dyn ApiService>,
    },
    /// Subscribe to progress events for a specific scan.
    ScanProgress {
        server_url: String,
        api_service: Arc<dyn ApiService>,
        scan_id: Uuid,
    },
}

/// Build stream descriptors from explicit ports.
pub fn stream_plan(
    inputs: LibrarySubscriptionInputs,
) -> Vec<LibraryStreamPlan> {
    if !matches!(inputs.load_state, LibrariesLoadState::Succeeded { .. }) {
        return Vec::new();
    }

    let mut plans = Vec::new();
    plans.push(LibraryStreamPlan::MediaEvents {
        server_url: inputs.server_url.clone(),
        api_service: Arc::clone(&inputs.api_service),
    });

    for scan_id in inputs.active_scan_ids {
        plans.push(LibraryStreamPlan::ScanProgress {
            server_url: inputs.server_url.clone(),
            api_service: Arc::clone(&inputs.api_service),
            scan_id,
        });
    }

    plans
}
