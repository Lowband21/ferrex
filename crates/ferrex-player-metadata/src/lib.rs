//! Metadata data-domain crate for Ferrex player clients.
//!
//! UI-bound image loading, Iced image handles, and presentation update plumbing
//! live in `ferrex-player-ui`. This crate intentionally remains free of Iced and
//! view dependencies so it can host future metadata contracts or services shared
//! by non-UI clients.

pub mod constants {
    //! Metadata and image-cache constants shared by runtime adapters.

    pub mod image {
        use std::time::Duration;

        pub const IMAGE_MAX_RETRY_ATTEMPTS: u8 = 15;
        pub const IMAGE_PENDING_RETRY_DELAY: Duration =
            Duration::from_millis(750);
        pub const IMAGE_RETRY_THROTTLE: Duration = Duration::from_millis(750);
    }

    pub mod memory_usage {
        const GIB: u64 = 1_073_741_824;
        pub const MAX_RAM_BYTES: u64 = GIB;
        pub const MAX_IMAGE_CACHE_BYTES: u64 = 5 * GIB;
    }
}
