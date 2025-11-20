#![cfg(feature = "scan-runtime")]

//! Compatibility wrapper for filesystem watch components.
//!
//! The active implementation now lives under `crate::scan::fs_watch`. This shim re-exports the new
//! location until callers are updated to use the `scan` namespace directly.

pub use crate::scan::fs_watch::*;
