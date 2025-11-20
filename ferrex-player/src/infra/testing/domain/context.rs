//! Domain test context for isolated domain testing
//!
//! Provides a simplified interface for testing any domain in isolation.

use crate::infra::testing::{
    executor::TestExecutor, mocks::MockRegistry, recorder::TestRecorder,
    time::TimeProvider,
};
use std::future::Future;
use std::pin::Pin;

/// Core trait for testing a domain in isolation
pub trait DomainTestContext: Send {
    /// Get the test executor for this context
    fn executor(&self) -> &TestExecutor;

    /// Get the time provider for this context
    fn time_provider(&self) -> &dyn TimeProvider;

    /// Get the mock registry for this context
    fn mock_registry(&self) -> &MockRegistry;

    /// Get the test recorder for debugging
    fn recorder(&self) -> &TestRecorder;

    /// Execute a command
    fn execute_command(
        &mut self,
        command: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>>;

    /// Execute a query
    fn execute_query(
        &self,
        query: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>>;

    /// Get events emitted since last clear
    fn events(&self) -> Vec<String>;

    /// Clear recorded events
    fn clear_events(&mut self);

    /// Reset the domain to initial state
    fn reset(&mut self);
}

/// Generic implementation of DomainTestContext for any domain
pub struct GenericDomainContext<S> {
    pub state: S,
    pub events: Vec<String>,
    pub executor: TestExecutor,
    pub time_provider: Box<dyn TimeProvider>,
    pub mock_registry: MockRegistry,
    pub recorder: TestRecorder,
}

impl<S> GenericDomainContext<S>
where
    S: Default + Send,
{
    /// Create a new domain test context
    pub fn new(time_provider: Box<dyn TimeProvider>) -> Self {
        Self {
            state: S::default(),
            events: Vec::new(),
            executor: TestExecutor::new(),
            time_provider,
            mock_registry: MockRegistry::new(),
            recorder: TestRecorder::new(),
        }
    }

    /// Create with a specific initial state
    pub fn with_state(state: S, time_provider: Box<dyn TimeProvider>) -> Self {
        Self {
            state,
            events: Vec::new(),
            executor: TestExecutor::new(),
            time_provider,
            mock_registry: MockRegistry::new(),
            recorder: TestRecorder::new(),
        }
    }

    /// Record an event
    pub fn emit_event(&mut self, event: impl Into<String>) {
        let event_str = event.into();
        self.events.push(event_str.clone());
        self.recorder.record_event(event_str);
    }

    /// Get current state
    pub fn state(&self) -> &S {
        &self.state
    }

    /// Get mutable state
    pub fn state_mut(&mut self) -> &mut S {
        &mut self.state
    }
}

/// Builder for setting up domain test contexts
pub struct DomainContextBuilder<S> {
    state: Option<S>,
    time_provider: Option<Box<dyn TimeProvider>>,
    executor: Option<TestExecutor>,
    mock_registry: Option<MockRegistry>,
    recorder: Option<TestRecorder>,
}

impl<S> DomainContextBuilder<S>
where
    S: Default + Send,
{
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            state: None,
            time_provider: None,
            executor: None,
            mock_registry: None,
            recorder: None,
        }
    }

    /// Set the initial state
    pub fn with_state(mut self, state: S) -> Self {
        self.state = Some(state);
        self
    }

    /// Set the time provider
    pub fn with_time_provider(
        mut self,
        provider: Box<dyn TimeProvider>,
    ) -> Self {
        self.time_provider = Some(provider);
        self
    }

    /// Set the executor
    pub fn with_executor(mut self, executor: TestExecutor) -> Self {
        self.executor = Some(executor);
        self
    }

    /// Set the mock registry
    pub fn with_mocks(mut self, registry: MockRegistry) -> Self {
        self.mock_registry = Some(registry);
        self
    }

    /// Set the recorder
    pub fn with_recorder(mut self, recorder: TestRecorder) -> Self {
        self.recorder = Some(recorder);
        self
    }

    /// Build the domain context
    pub fn build(self) -> GenericDomainContext<S> {
        use crate::infra::testing::time::SystemTimeProvider;

        GenericDomainContext {
            state: self.state.unwrap_or_default(),
            events: Vec::new(),
            executor: self.executor.unwrap_or_default(),
            time_provider: self
                .time_provider
                .unwrap_or_else(|| Box::new(SystemTimeProvider)),
            mock_registry: self.mock_registry.unwrap_or_default(),
            recorder: self.recorder.unwrap_or_default(),
        }
    }
}

impl<S> Default for DomainContextBuilder<S>
where
    S: Default + Send,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Simple test context for basic domain testing
pub struct SimpleTestContext {
    state: String,
    events: Vec<String>,
    executor: TestExecutor,
    time_provider: Box<dyn TimeProvider>,
    mock_registry: MockRegistry,
    recorder: TestRecorder,
}

impl SimpleTestContext {
    pub fn new(time_provider: Box<dyn TimeProvider>) -> Self {
        Self {
            state: String::new(),
            events: Vec::new(),
            executor: TestExecutor::new(),
            time_provider,
            mock_registry: MockRegistry::new(),
            recorder: TestRecorder::new(),
        }
    }
}

impl DomainTestContext for SimpleTestContext {
    fn executor(&self) -> &TestExecutor {
        &self.executor
    }

    fn time_provider(&self) -> &dyn TimeProvider {
        &*self.time_provider
    }

    fn mock_registry(&self) -> &MockRegistry {
        &self.mock_registry
    }

    fn recorder(&self) -> &TestRecorder {
        &self.recorder
    }

    fn execute_command(
        &mut self,
        command: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        self.recorder.record_command(command.to_string());
        self.state = format!("Executed: {}", command);
        Box::pin(async { Ok(()) })
    }

    fn execute_query(
        &self,
        query: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>> {
        self.recorder.record_query(query.to_string());
        let query_owned = query.to_string();
        Box::pin(async move { Ok(format!("Result for: {}", query_owned)) })
    }

    fn events(&self) -> Vec<String> {
        self.events.clone()
    }

    fn clear_events(&mut self) {
        self.events.clear();
    }

    fn reset(&mut self) {
        self.state.clear();
        self.events.clear();
        self.executor.reset();
        self.mock_registry.reset_all();
        self.recorder.clear();
    }
}

/// Macro to implement DomainTestContext for specific domains
#[macro_export]
macro_rules! impl_domain_test_context {
    ($context:ty) => {
        impl $crate::infra::testing::domain::DomainTestContext for $context {
            fn executor(&self) -> &$crate::infra::testing::TestExecutor {
                &self.executor
            }

            fn time_provider(
                &self,
            ) -> &dyn $crate::infra::testing::TimeProvider {
                &*self.time_provider
            }

            fn mock_registry(&self) -> &$crate::infra::testing::MockRegistry {
                &self.mock_registry
            }

            fn recorder(&self) -> &$crate::infra::testing::TestRecorder {
                &self.recorder
            }

            fn execute_command(
                &mut self,
                command: &str,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<(), String>>
                        + Send
                        + '_,
                >,
            > {
                self.recorder.record_command(command.to_string());
                Box::pin(async { Ok(()) })
            }

            fn execute_query(
                &self,
                query: &str,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<String, String>>
                        + Send
                        + '_,
                >,
            > {
                self.recorder.record_query(query.to_string());
                Box::pin(async move { Ok(String::new()) })
            }

            fn events(&self) -> Vec<String> {
                self.events.clone()
            }

            fn clear_events(&mut self) {
                self.events.clear()
            }

            fn reset(&mut self) {
                self.executor.reset();
                self.mock_registry.reset_all();
                self.recorder.clear();
                self.events.clear();
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::testing::time::VirtualTimeProvider;

    #[tokio::test]
    async fn test_simple_context() {
        let time_provider = Box::new(VirtualTimeProvider::new());
        let mut ctx = SimpleTestContext::new(time_provider);

        // Test command execution
        let result = ctx.execute_command("test_command").await;
        assert!(result.is_ok());

        // Test query execution
        let result = ctx.execute_query("test_query").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Result for: test_query");

        // Test reset
        ctx.reset();
        assert_eq!(ctx.events().len(), 0);
    }

    #[test]
    fn test_generic_context() {
        #[derive(Default)]
        struct TestState {
            value: i32,
        }

        let time_provider = Box::new(VirtualTimeProvider::new());
        let mut ctx = GenericDomainContext::<TestState>::new(time_provider);

        // Test state access
        assert_eq!(ctx.state().value, 0);
        ctx.state_mut().value = 42;
        assert_eq!(ctx.state().value, 42);

        // Test event emission
        ctx.emit_event("test_event");
        assert_eq!(ctx.events.len(), 1);
    }

    #[test]
    fn test_context_builder() {
        #[derive(Default)]
        struct TestState {
            name: String,
        }

        let ctx = DomainContextBuilder::<TestState>::new()
            .with_state(TestState {
                name: "test".to_string(),
            })
            .with_executor(TestExecutor::new())
            .build();

        assert_eq!(ctx.state.name, "test");
    }
}
