//! Server infrastructure adapters and runtime state.
//!
//! This namespace contains process-local wiring that belongs to the HTTP server:
//! configuration re-exports, caches, middleware, websocket fan-out, scan
//! orchestration, startup helpers, content negotiation, and shared Axum state.

/// Dependency container assembled during startup.
pub mod app_context;
/// Shared Axum application state.
pub mod app_state;
/// Server-side cache adapters for media batch payloads.
pub mod cache;
/// Configuration re-exports sourced from `ferrexctl`.
pub mod config;
/// Server constants used by handlers and infrastructure.
pub mod constants;
/// HTTP content negotiation for JSON and FlatBuffers responses.
pub mod content_negotiation;
/// Demo-mode runtime flags and helpers.
pub mod demo_mode;
/// HTTP-facing error conversion helpers.
pub mod errors;
/// FlatBuffers request body parsing helpers.
pub mod fb_request_parsing;
/// HTTP middleware layers.
pub mod middleware;
/// Durable scan-orchestrator server wiring.
pub mod orchestration;
/// PostgreSQL connection/session tuning helpers.
pub mod postgres_tuning;
/// Scan runtime adapters, event buses, and control-plane helpers.
pub mod scan;
/// Startup and server-lifecycle helpers.
pub mod startup;
/// Thumbnail resolution and serving support.
pub mod thumbnail_service;
/// Bounded FFmpeg HLS generation and cache publication.
pub mod transcode;
/// Websocket connection management and event messages.
pub mod websocket;
