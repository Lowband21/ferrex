//! Infrastructure adapters grouped by domain.

#[cfg(feature = "rkyv")]
pub mod archive;
pub mod media;
#[cfg(feature = "scan-runtime")]
pub mod scan;
pub mod watch;

// Re-export submodules so existing imports can transition gradually.
#[cfg(feature = "rkyv")]
pub use archive::*;
pub use media::*;
#[cfg(feature = "scan-runtime")]
pub use scan::*;
pub use watch::*;
