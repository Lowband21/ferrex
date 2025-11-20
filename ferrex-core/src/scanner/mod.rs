//! Transitional re-export for legacy scanner helpers.
//!
//! Utilities that used to live under `crate::scanner` have moved to `crate::scan::scanner`. This
//! module keeps the old path available while we finish reorganizing the scan domain.

pub use crate::scan::scanner::*;
