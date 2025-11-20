//! Domain testing framework modules
//!
//! Provides abstractions for testing domains in isolation.

pub mod context;
pub mod harness;
pub mod boundary;

pub use context::{
    DomainTestContext,
    GenericDomainContext,
    DomainContextBuilder,
};

pub use harness::{
    TestHarness,
    HarnessConfig,
    TestResult,
};

pub use boundary::{
    DomainBoundary,
    ServiceDependency,
    EventBus,
    InMemoryEventBus,
};