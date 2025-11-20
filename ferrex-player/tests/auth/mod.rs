//! Authentication domain TDD tests organized by concern
//! 
//! Following proper Test-Driven Development:
//! - RED: Write failing tests for requirements
//! - GREEN: Implement minimal code to pass
//! - REFACTOR: Improve implementation while keeping tests green

pub mod test_helper;

// Test modules organized by functionality
pub mod device_trust_tests;
pub mod first_run_tests;

// Tests not yet fully implemented (placeholders exist)
pub mod session_management_tests;
pub mod brute_force_tests;
pub mod auto_login_tests;
pub mod user_deletion_tests;

// Re-export the test helper for all test modules
pub use test_helper::TestHelper;