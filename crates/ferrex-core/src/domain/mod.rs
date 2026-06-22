//! Domain layer entry modules.

#[cfg(feature = "database")]
pub mod cache;
/// Demo-mode helpers for quickly seeding fake media libraries.
#[cfg(feature = "demo")]
pub mod demo;
/// Provider-neutral intelligence runtime contracts.
pub mod intelligence;
pub mod media;
/// Scan domain entrypoint bundling orchestrator, filesystem watch, and helper modules.
#[cfg(feature = "scan-runtime")]
#[cfg_attr(docsrs, doc(cfg(feature = "scan-runtime")))]
pub mod scan;
/// First-run setup flows (claim codes, binding)
#[cfg(feature = "database")]
pub mod setup;
pub mod theater_plate;
pub mod users;
pub mod watch;
