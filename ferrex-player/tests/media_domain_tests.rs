//! Media Domain Integration Tests
//! 
//! Test runner for all media domain tests following TDD principles.
//! Tests verify actual behavior, not mock implementations.

#[cfg(test)]
mod media_store {
    include!("media/media_store_tests.rs");
}

#[cfg(test)]
mod domain_isolation {
    include!("media/domain_isolation_tests.rs");
}

#[cfg(test)]
mod sorting {
    include!("media/sorting_tests.rs");
}

// Future test modules to be added:
// - batch_metadata_tests.rs - Test batch metadata fetching
// - cross_domain_events_tests.rs - Test event-based communication
// - media_lifecycle_tests.rs - Test media reference lifecycle