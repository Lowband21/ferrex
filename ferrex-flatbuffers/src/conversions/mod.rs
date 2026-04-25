//! Conversions from `ferrex-model` types to FlatBuffers builders.
//!
//! Each submodule mirrors a `.fbs` schema namespace and provides
//! `build_*` functions that take a `FlatBufferBuilder` and a model type,
//! returning a `WIPOffset` suitable for insertion into a parent table.

pub mod auth;
pub mod batch_data;
pub mod batch_sync;
pub mod common;
pub mod details;
pub mod files;
pub mod library;
pub mod media;
pub mod media_query;
pub mod watch;

// Re-export the timestamp helper since it's used everywhere.
pub use common::timestamp_to_fb;
