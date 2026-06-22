//! Server scan facades and route adapters.
//!
//! Public map:
//! - `scan_manager` exposes `ScanControlPlane`, the single server facade for
//!   scan commands, scan progress history, media-event history, and catalog
//!   projection fan-out.
//! - `folder_inventory` contains route-facing read adapters for folder progress
//!   diagnostics.
//!
//! Internal projection helpers and the media-event bus are intentionally kept
//! behind `scan_manager` so production code has one enqueue/progress projection
//! path instead of multiple independently callable publishers.

mod catalog_event_projection;
pub mod folder_inventory;
mod media_event_bus;
mod movie_batch_notifier;
pub mod scan_manager;
