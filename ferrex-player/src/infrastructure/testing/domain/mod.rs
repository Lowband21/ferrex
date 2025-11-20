//! Domain testing framework modules
//!
//! Provides abstractions for testing domains in isolation.

pub mod boundary;
pub mod context;
pub mod harness;

pub use context::{
    DomainContextBuilder, DomainTestContext, GenericDomainContext,
};

pub use harness::{HarnessConfig, TestHarness, TestResult};

pub use boundary::{
    DomainBoundary, EventBus, InMemoryEventBus, ServiceDependency,
};
