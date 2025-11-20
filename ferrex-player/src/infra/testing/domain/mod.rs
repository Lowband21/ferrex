//! Domain testing framework modules
//!
//! Provides abstractions for testing domains in isolation.

pub mod boundary;
pub mod context;

pub use context::{
    DomainContextBuilder, DomainTestContext, GenericDomainContext,
};

pub use boundary::{
    DomainBoundary, EventBus, InMemoryEventBus, ServiceDependency,
};
