//! FlatBuffers serialization foundation for Ferrex mobile clients.
//!
//! The crate contains generated Rust bindings for the shared `.fbs` schemas and
//! focused conversion helpers for server-side payloads used by Android, Android
//! TV, and other clients that consume the mobile FlatBuffers contract.
//!
//! Use the `fb` re-export when direct generated table access is required and the
//! `conversions` module when translating server/core model values into wire
//! payloads.

#![allow(
    // The generated schema bindings intentionally expose FlatBuffers-created
    // names and fields without hand-written docs. Hand-authored conversion and
    // helper modules carry the public documentation surface for this crate.
    missing_docs,
    missing_debug_implementations,
    dead_code,
    unused_imports,
    clippy::all,
    unsafe_op_in_unsafe_fn
)]

/// Hand-written conversions from Ferrex model/API values into FlatBuffers payloads.
pub mod conversions;
/// Generated Rust modules for the Ferrex FlatBuffers namespace tree.
pub mod generated;
/// UUID conversion helpers shared by generated-table builders.
pub mod uuid_helpers;

/// Re-export the generated Ferrex FlatBuffers namespace tree.
pub use generated::ferrex as fb;

/// MIME type used for FlatBuffers responses.
pub const FLATBUFFERS_MIME: &str = "application/x-flatbuffers";
