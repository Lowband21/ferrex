//! Type-safe mock registry for testing
//!
//! Provides a simplified mock registry using TypeId for type safety.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Trait for mock services that can be stored in the registry
pub trait MockService: Send + Sync {
    /// Reset the mock to initial state
    fn reset(&mut self);

    /// Verify expectations were met
    fn verify(&self) -> Result<(), String>;

    /// Get number of times this mock was called
    fn call_count(&self) -> usize;

    /// Convert to Any for downcasting
    fn as_any(&self) -> &dyn Any;

    /// Convert to mutable Any for downcasting
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// Simple mock implementation
pub struct SimpleMock {
    call_count: usize,
    expected_calls: Option<usize>,
    operations: Vec<String>,
}

impl SimpleMock {
    pub fn new() -> Self {
        Self {
            call_count: 0,
            expected_calls: None,
            operations: Vec::new(),
        }
    }

    /// Set expected number of calls
    pub fn expect_calls(mut self, count: usize) -> Self {
        self.expected_calls = Some(count);
        self
    }

    /// Record an operation
    pub fn record_operation(&mut self, operation: String) {
        self.operations.push(operation);
        self.call_count += 1;
    }

    /// Get recorded operations
    pub fn operations(&self) -> &[String] {
        &self.operations
    }
}

impl MockService for SimpleMock {
    fn reset(&mut self) {
        self.call_count = 0;
        self.operations.clear();
    }

    fn verify(&self) -> Result<(), String> {
        if let Some(expected) = self.expected_calls
            && self.call_count != expected
        {
            return Err(format!(
                "Expected {} calls, got {}",
                expected, self.call_count
            ));
        }
        Ok(())
    }

    fn call_count(&self) -> usize {
        self.call_count
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl Default for SimpleMock {
    fn default() -> Self {
        Self::new()
    }
}

/// Type-safe mock registry using TypeId
pub struct MockRegistry {
    mocks: Arc<Mutex<HashMap<TypeId, Box<dyn MockService>>>>,
}

impl MockRegistry {
    /// Create a new mock registry
    pub fn new() -> Self {
        Self {
            mocks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a mock service for a type
    pub fn register<T, M>(&self, mock: M)
    where
        T: 'static,
        M: MockService + 'static,
    {
        let type_id = TypeId::of::<T>();
        self.mocks.lock().unwrap().insert(type_id, Box::new(mock));
    }

    /// Get a mock service by type
    pub fn get<T, M>(&self) -> Option<MockHandle<M>>
    where
        T: 'static,
        M: MockService + 'static,
    {
        let type_id = TypeId::of::<T>();
        let mocks = self.mocks.lock().unwrap();

        if mocks.contains_key(&type_id) {
            Some(MockHandle {
                registry: self.clone(),
                type_id,
                _phantom: std::marker::PhantomData,
            })
        } else {
            None
        }
    }

    /// Execute an operation on a mock
    pub fn with_mock<T, M, F, R>(&self, f: F) -> Option<R>
    where
        T: 'static,
        M: MockService + 'static,
        F: FnOnce(&M) -> R,
    {
        let type_id = TypeId::of::<T>();
        let mocks = self.mocks.lock().unwrap();

        mocks
            .get(&type_id)
            .and_then(|mock| mock.as_any().downcast_ref::<M>())
            .map(f)
    }

    /// Execute a mutable operation on a mock
    pub fn with_mock_mut<T, M, F, R>(&self, f: F) -> Option<R>
    where
        T: 'static,
        M: MockService + 'static,
        F: FnOnce(&mut M) -> R,
    {
        let type_id = TypeId::of::<T>();
        let mut mocks = self.mocks.lock().unwrap();

        mocks
            .get_mut(&type_id)
            .and_then(|mock| mock.as_any_mut().downcast_mut::<M>())
            .map(f)
    }

    /// Remove a mock service
    pub fn remove<T>(&self) -> bool
    where
        T: 'static,
    {
        let type_id = TypeId::of::<T>();
        self.mocks.lock().unwrap().remove(&type_id).is_some()
    }

    /// Reset all mocks to initial state
    pub fn reset_all(&self) {
        for mock in self.mocks.lock().unwrap().values_mut() {
            mock.reset();
        }
    }

    /// Verify all mock expectations
    pub fn verify_all(&self) -> Result<(), Vec<String>> {
        let errors: Vec<String> = self
            .mocks
            .lock()
            .unwrap()
            .values()
            .filter_map(|mock| mock.verify().err())
            .collect();

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Get total number of registered mocks
    pub fn count(&self) -> usize {
        self.mocks.lock().unwrap().len()
    }

    /// Clear all mocks
    pub fn clear(&self) {
        self.mocks.lock().unwrap().clear();
    }
}

impl Clone for MockRegistry {
    fn clone(&self) -> Self {
        Self {
            mocks: Arc::clone(&self.mocks),
        }
    }
}

impl Default for MockRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle to a mock service in the registry
pub struct MockHandle<M> {
    registry: MockRegistry,
    type_id: TypeId,
    _phantom: std::marker::PhantomData<M>,
}

impl<M> MockHandle<M>
where
    M: MockService + 'static,
{
    /// Execute an operation on the mock
    pub fn with<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&M) -> R,
    {
        let mocks = self.registry.mocks.lock().unwrap();

        mocks
            .get(&self.type_id)
            .and_then(|mock| mock.as_any().downcast_ref::<M>())
            .map(f)
            .ok_or_else(|| "Mock not found or wrong type".to_string())
    }

    /// Execute a mutable operation on the mock
    pub fn with_mut<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&mut M) -> R,
    {
        let mut mocks = self.registry.mocks.lock().unwrap();

        mocks
            .get_mut(&self.type_id)
            .and_then(|mock| mock.as_any_mut().downcast_mut::<M>())
            .map(f)
            .ok_or_else(|| "Mock not found or wrong type".to_string())
    }

    /// Reset the mock
    pub fn reset(&self) -> Result<(), String> {
        self.with_mut(|mock| mock.reset())
    }

    /// Verify the mock's expectations
    pub fn verify(&self) -> Result<(), String> {
        self.with(|mock| mock.verify())?
    }

    /// Get call count
    pub fn call_count(&self) -> Result<usize, String> {
        self.with(|mock| mock.call_count())
    }
}

/// Builder for creating mocks with fluent API
pub struct MockBuilder<M> {
    mock: M,
}

impl<M> MockBuilder<M>
where
    M: MockService,
{
    pub fn new(mock: M) -> Self {
        Self { mock }
    }

    pub fn build(self) -> M {
        self.mock
    }
}

/// Example custom mock for specific domain testing
pub struct DomainMock {
    base: SimpleMock,
    responses: HashMap<String, String>,
}

impl DomainMock {
    pub fn new() -> Self {
        Self {
            base: SimpleMock::new(),
            responses: HashMap::new(),
        }
    }

    pub fn with_response(mut self, query: String, response: String) -> Self {
        self.responses.insert(query, response);
        self
    }

    pub fn execute(&mut self, query: &str) -> Option<String> {
        self.base.record_operation(format!("execute: {}", query));
        self.responses.get(query).cloned()
    }
}

impl MockService for DomainMock {
    fn reset(&mut self) {
        self.base.reset();
        self.responses.clear();
    }

    fn verify(&self) -> Result<(), String> {
        self.base.verify()
    }

    fn call_count(&self) -> usize {
        self.base.call_count()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl Default for DomainMock {
    fn default() -> Self {
        Self::new()
    }
}

/// Macro for creating mock services with specific behavior
#[macro_export]
macro_rules! mock_service {
    ($name:ident) => {
        pub struct $name {
            inner: $crate::infra::testing::mocks::SimpleMock,
        }

        impl $name {
            pub fn new() -> Self {
                Self {
                    inner: $crate::infra::testing::mocks::SimpleMock::new(),
                }
            }

            pub fn expect_calls(mut self, count: usize) -> Self {
                self.inner = self.inner.expect_calls(count);
                self
            }

            pub fn record(&mut self, operation: String) {
                self.inner.record_operation(operation);
            }
        }

        impl $crate::infra::testing::mocks::MockService for $name {
            fn reset(&mut self) {
                self.inner.reset()
            }

            fn verify(&self) -> Result<(), String> {
                self.inner.verify()
            }

            fn call_count(&self) -> usize {
                self.inner.call_count()
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }

            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    struct UserService;
    struct AuthService;

    #[test]
    fn test_mock_registry_basic() {
        let registry = MockRegistry::new();

        // Register mocks
        let user_mock = SimpleMock::new().expect_calls(2);
        let auth_mock = SimpleMock::new().expect_calls(1);

        registry.register::<UserService, _>(user_mock);
        registry.register::<AuthService, _>(auth_mock);

        // Use mocks
        registry.with_mock_mut::<UserService, SimpleMock, _, _>(|mock| {
            mock.record_operation("create_user".to_string());
            mock.record_operation("update_user".to_string());
        });

        registry.with_mock_mut::<AuthService, SimpleMock, _, _>(|mock| {
            mock.record_operation("authenticate".to_string());
        });

        // Verify all mocks
        assert!(registry.verify_all().is_ok());
    }

    #[test]
    fn test_mock_handle() {
        let registry = MockRegistry::new();

        let mock = SimpleMock::new().expect_calls(1);
        registry.register::<UserService, _>(mock);

        let handle: MockHandle<SimpleMock> =
            registry.get::<UserService, _>().unwrap();

        handle
            .with_mut(|mock| {
                mock.record_operation("test".to_string());
            })
            .unwrap();

        assert!(handle.verify().is_ok());
        assert_eq!(handle.call_count().unwrap(), 1);
    }

    #[test]
    fn test_domain_mock() {
        let registry = MockRegistry::new();

        let mock = DomainMock::new()
            .with_response("get_user".to_string(), "user123".to_string());

        registry.register::<UserService, _>(mock);

        let result =
            registry.with_mock_mut::<UserService, DomainMock, _, _>(|mock| {
                mock.execute("get_user")
            });

        assert_eq!(result, Some(Some("user123".to_string())));
    }

    #[test]
    fn test_mock_not_found() {
        let registry = MockRegistry::new();

        let handle: Option<MockHandle<SimpleMock>> =
            registry.get::<UserService, _>();
        assert!(handle.is_none());
    }

    #[test]
    fn test_mock_reset() {
        let registry = MockRegistry::new();

        let mock = SimpleMock::new();
        registry.register::<UserService, _>(mock);

        registry.with_mock_mut::<UserService, SimpleMock, _, _>(|mock| {
            mock.record_operation("test".to_string());
        });

        registry.reset_all();

        let count =
            registry.with_mock::<UserService, SimpleMock, _, _>(|mock| {
                mock.call_count()
            });

        assert_eq!(count, Some(0));
    }

    mock_service!(TestMock);

    #[test]
    fn test_macro_generated_mock() {
        let mut mock = TestMock::new().expect_calls(1);
        mock.record("test_operation".to_string());

        assert_eq!(mock.call_count(), 1);
        assert!(mock.verify().is_ok());
    }
}
