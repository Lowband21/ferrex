//! Ferrex HTTP server crate.
//!
//! `ferrex-server` wires the Rust core domain crates into a runnable Axum
//! service. It owns server-only concerns such as HTTP routing, request/response
//! handlers, application facades, startup/runtime configuration, database URL
//! safety checks, websocket delivery, thumbnail serving, and background scan
//! orchestration adapters.
//!
//! The crate intentionally keeps most business rules in `ferrex-core` and
//! configuration defaults in `ferrexctl`; this crate composes those reusable
//! surfaces with transport and process lifecycle code.

/// Application-service facades used by request handlers.
pub mod application;
/// Database URL validation and demo-database derivation helpers.
pub mod db;
/// Demo-mode startup and scan helpers enabled by the `demo` feature.
#[cfg(feature = "demo")]
pub mod demo;
/// Axum request handlers grouped by API area.
pub mod handlers;
/// Server infrastructure adapters, middleware, caches, and runtime state.
pub mod infra;
/// Versioned API router composition.
pub mod routes;
