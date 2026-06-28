//! HTTP handler modules for Ferrex server routes.
//!
//! Handlers are grouped by product/API area and should stay thin: parse inputs,
//! call application/core services, and convert results into HTTP responses.

/// Administrative handlers for demo controls, media roots, scan history, and developer utilities.
pub mod admin;
/// Collection browsing, rule preview, and manual-editing handlers.
pub mod collections;
/// Local-network discovery handler for client bootstrap.
pub mod discovery;
/// Websocket upgrade handler for realtime events.
pub mod handle_websocket;
/// Bounded intelligence read-model, artifact, candidate-search, and audit-summary handlers.
pub mod intelligence;
/// Library, media, search, image, and streaming-adjacent media handlers.
pub mod media;
/// Scan-control and scan-status handlers.
pub mod scan;
/// Streaming and synchronized playback handlers.
pub mod stream;
/// User, role, setup, and authentication handlers.
pub mod users;
