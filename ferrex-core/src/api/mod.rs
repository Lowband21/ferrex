//! API boundary for Ferrex Core.
//!
//! Groups versioned routes, scan-facing DTOs, and general API data structures
//! so consumers can depend on a single namespace instead of dozens of root
//! modules.

pub mod routes;
pub mod scan;
pub mod types;

// Curated re-exports for callers that previously imported from root modules.
pub use routes::v1;
pub use scan::{
    OrchestratorConfigView, ScanConfig, ScanMetrics, ScanQueueDepths,
};
pub use types::*;
