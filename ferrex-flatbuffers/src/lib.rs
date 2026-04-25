//! FlatBuffers serialization layer for ferrex mobile clients.
//!
//! This crate provides:
//! - Generated FlatBuffers types from `.fbs` schemas (in `generated/`)
//! - Conversion helpers from `ferrex-model` types to FlatBuffers builders
//! - UUID helper functions for the `[ubyte:16]` representation
//!
//! # Architecture
//!
//! The server uses this crate to serialize responses when the client sends
//! `Accept: application/x-flatbuffers`. Mobile clients use the Swift/Kotlin
//! generated code directly — this Rust crate is server-side only.

#![allow(unused_imports, dead_code, clippy::all, missing_docs)]

pub mod conversions;
pub mod generated;
pub mod uuid_helpers;

// Re-export the generated namespace tree for ergonomic access.
pub use generated::ferrex as fb;

/// MIME type for FlatBuffers content negotiation.
pub const FLATBUFFERS_MIME: &str = "application/x-flatbuffers";
