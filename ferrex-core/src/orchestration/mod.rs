#![cfg(feature = "scan-runtime")]

//! Temporary compatibility shim for the scan orchestrator modules.
//!
//! The orchestrator has been relocated under `crate::scan::orchestration`. Existing call sites
//! should migrate to the new namespace once the restructuring is complete.

pub use crate::scan::orchestration::*;
