//! Domain-agnostic testing infrastructure for Ferrex
//!
//! This module provides a comprehensive testing framework that enables:
//! - Deterministic async execution
//! - Virtual time control
//! - Type-safe mocking without Any
//! - Operation recording and debugging
//! - Domain isolation and testing
//!
//! # Architecture
//!
//! The testing infrastructure is organized into several key components:
//!
//! - **Executor**: Controls async task execution deterministically
//! - **Time**: Virtual time provider for testing time-dependent code
//! - **Mocks**: Type-safe mock registry without using Any
//! - **Recorder**: Records operations for debugging test failures
//! - **Domain**: Framework for testing domains in isolation
//!
//! # Usage Example
//!
//! ```rust
//! use ferrex_player::infrastructure::testing::*;
//! use ferrex_player::infrastructure::testing::time::VirtualTimeProvider;
//! use ferrex_player::infrastructure::testing::domain::*;
//!
//! #[tokio::test]
//! async fn test_domain_behavior() {
//!     // Create test context with virtual time
//!     let time_provider = VirtualTimeProvider::new();
//!     let mut ctx = MyDomainContext::new(time_provider.clone());
//!     
//!     // Execute domain operations
//!     ctx.execute_command(MyCommand::DoSomething).await.unwrap();
//!     
//!     // Advance virtual time
//!     time_provider.advance(Duration::from_secs(10));
//!     
//!     // Verify state and events
//!     assert_eq!(ctx.state().value, expected_value);
//!     assert_eq!(ctx.events().len(), 1);
//! }
//! ```

pub mod domain;
pub mod executor;
pub mod mocks;
pub mod recorder;
pub mod stubs;
pub mod time;
pub mod utils;

// Re-export commonly used types
pub use domain::{
    DomainBoundary, DomainContextBuilder, DomainTestContext, EventBus,
    GenericDomainContext, HarnessConfig, InMemoryEventBus, ServiceDependency,
    TestHarness, TestResult,
};
pub use executor::{ExecutionMode, TaskTestExt, TestExecutor};
pub use mocks::{
    DomainMock, MockBuilder, MockHandle, MockRegistry, MockService, SimpleMock,
};
pub use recorder::{Operation, OperationType, StateSnapshot, TestRecorder};
pub use stubs::{TestApiService, TestAuthService, TestSettingsService};
pub use time::{
    SystemTimeProvider, TimeContext, TimeProvider, VirtualTimeProvider,
};

// The macros are already exported at the crate root via #[macro_export]
// They don't need to be re-exported here

/// Prelude for convenient imports
pub mod prelude {
    pub use super::domain::*;
    pub use super::executor::*;
    pub use super::mocks::*;
    pub use super::recorder::*;
    pub use super::time::*;
    pub use super::utils::*;
}
