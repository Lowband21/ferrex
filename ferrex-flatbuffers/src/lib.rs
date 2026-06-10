//! FlatBuffers serialization foundation for Ferrex mobile clients.
//!
//! The crate contains generated Rust bindings for the shared `.fbs` schemas and
//! focused conversion helpers for the server-side payloads used by the Android
//! and Android TV clients.

#![allow(
    missing_docs,
    missing_debug_implementations,
    dead_code,
    unused_imports,
    clippy::all,
    unsafe_op_in_unsafe_fn
)]

pub mod conversions;
pub mod generated;
pub mod uuid_helpers;

/// Re-export the generated Ferrex FlatBuffers namespace tree.
pub use generated::ferrex as fb;

/// MIME type used for FlatBuffers responses.
pub const FLATBUFFERS_MIME: &str = "application/x-flatbuffers";
