//! Shared scanner defaults.
//!
//! These defaults are consumed by both the scan runtime and configuration tooling, so the
//! canonical values live in `ferrex-model` and are re-exported here for compatibility.

pub use crate::types::scan::scanner::settings::{
    DEFAULT_VIDEO_FILE_EXTENSIONS, default_video_file_extensions_vec,
};
