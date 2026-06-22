//! Metadata data-domain crate for Ferrex player clients.
//!
//! UI-bound image loading, Iced image handles, and presentation update plumbing
//! live in `ferrex-player-ui`. This crate intentionally remains free of Iced and
//! view dependencies so it can host future metadata contracts or services shared
//! by non-UI clients.

/// Metadata and image-cache constants shared by runtime adapters.
pub mod constants {
    //! Metadata and image-cache constants shared by runtime adapters.

    /// Image retry and throttling constants.
    pub mod image {
        use std::time::Duration;

        /// Maximum number of attempts for pending image fetch/retry loops.
        pub const IMAGE_MAX_RETRY_ATTEMPTS: u8 = 15;
        /// Delay before retrying an image that is still pending server-side.
        pub const IMAGE_PENDING_RETRY_DELAY: Duration =
            Duration::from_millis(750);
        /// Minimum delay between repeated image retry attempts.
        pub const IMAGE_RETRY_THROTTLE: Duration = Duration::from_millis(750);
    }

    /// Memory budget constants for image and metadata caches.
    pub mod memory_usage {
        const GIB: u64 = 1_073_741_824;
        /// Soft RAM budget for metadata cache usage.
        pub const MAX_RAM_BYTES: u64 = GIB;
        /// Soft image cache budget.
        pub const MAX_IMAGE_CACHE_BYTES: u64 = 5 * GIB;
    }
}
