//! Domain boundary interfaces for cross-domain testing
//!
//! Provides simplified abstractions for domain dependencies and cross-domain communication.

use std::collections::HashMap;
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

/// Represents a service that can handle commands and queries
pub trait ServiceDependency: Send + Sync {
    /// Execute a command on the service
    fn execute_command(
        &mut self,
        command: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>>;

    /// Execute a query on the service
    fn execute_query(
        &self,
        query: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>>;
}

/// Event bus for cross-domain communication
pub trait EventBus: Send + Sync {
    /// Publish an event to the bus
    fn publish(&mut self, event: &str);

    /// Get all published events
    fn events(&self) -> Vec<String>;

    /// Clear all events
    fn clear(&mut self);
}

/// Simple in-memory event bus implementation
#[derive(Clone)]
pub struct InMemoryEventBus {
    events: Arc<Mutex<Vec<String>>>,
}

impl InMemoryEventBus {
    pub fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl EventBus for InMemoryEventBus {
    fn publish(&mut self, event: &str) {
        self.events.lock().unwrap().push(event.to_string());
    }

    fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }

    fn clear(&mut self) {
        self.events.lock().unwrap().clear();
    }
}

impl Default for InMemoryEventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Domain boundary that encapsulates external dependencies
pub struct DomainBoundary {
    services: HashMap<String, Box<dyn ServiceDependency>>,
    event_bus: Box<dyn EventBus>,
}

impl DomainBoundary {
    /// Create a new domain boundary
    pub fn new(event_bus: Box<dyn EventBus>) -> Self {
        Self {
            services: HashMap::new(),
            event_bus,
        }
    }

    /// Create with default event bus
    pub fn with_default_bus() -> Self {
        Self::new(Box::new(InMemoryEventBus::new()))
    }

    /// Register a service dependency
    pub fn register_service(
        &mut self,
        name: impl Into<String>,
        service: Box<dyn ServiceDependency>,
    ) {
        self.services.insert(name.into(), service);
    }

    /// Get a service by name
    pub fn service(&self, name: &str) -> Option<&dyn ServiceDependency> {
        self.services.get(name).map(|s| s.as_ref())
    }

    /// Get a mutable service by name
    pub fn service_mut(&mut self, name: &str) -> Option<&mut dyn ServiceDependency> {
        match self.services.get_mut(name) {
            Some(service) => Some(&mut **service),
            None => None,
        }
    }

    /// Execute a command on a service
    pub async fn execute_command(&mut self, service: &str, command: &str) -> Result<(), String> {
        self.service_mut(service)
            .ok_or_else(|| format!("Service '{}' not found", service))?
            .execute_command(command)
            .await
    }

    /// Execute a query on a service
    pub async fn execute_query(&self, service: &str, query: &str) -> Result<String, String> {
        self.service(service)
            .ok_or_else(|| format!("Service '{}' not found", service))?
            .execute_query(query)
            .await
    }

    /// Publish an event
    pub fn publish_event(&mut self, event: &str) {
        self.event_bus.publish(event);
    }

    /// Get all events
    pub fn events(&self) -> Vec<String> {
        self.event_bus.events()
    }

    /// Clear all events
    pub fn clear_events(&mut self) {
        self.event_bus.clear();
    }
}

impl Default for DomainBoundary {
    fn default() -> Self {
        Self::with_default_bus()
    }
}

/// Mock service implementation for testing
pub struct MockService {
    commands: Arc<Mutex<Vec<String>>>,
    query_responses: Arc<Mutex<HashMap<String, String>>>,
    command_errors: Arc<Mutex<HashMap<String, String>>>,
}

impl MockService {
    pub fn new() -> Self {
        Self {
            commands: Arc::new(Mutex::new(Vec::new())),
            query_responses: Arc::new(Mutex::new(HashMap::new())),
            command_errors: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Set a response for a query
    pub fn set_query_response(&self, query: impl Into<String>, response: impl Into<String>) {
        self.query_responses
            .lock()
            .unwrap()
            .insert(query.into(), response.into());
    }

    /// Set an error for a command
    pub fn set_command_error(&self, command: impl Into<String>, error: impl Into<String>) {
        self.command_errors
            .lock()
            .unwrap()
            .insert(command.into(), error.into());
    }

    /// Get recorded commands
    pub fn commands(&self) -> Vec<String> {
        self.commands.lock().unwrap().clone()
    }
}

impl ServiceDependency for MockService {
    fn execute_command(
        &mut self,
        command: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>> {
        self.commands.lock().unwrap().push(command.to_string());

        let error = self.command_errors.lock().unwrap().get(command).cloned();

        Box::pin(async move {
            if let Some(err) = error {
                Err(err)
            } else {
                Ok(())
            }
        })
    }

    fn execute_query(
        &self,
        query: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>> {
        let response = self.query_responses.lock().unwrap().get(query).cloned();

        let query_owned = query.to_string();
        Box::pin(async move {
            response.ok_or_else(|| format!("No response configured for query: {}", query_owned))
        })
    }
}

impl Default for MockService {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for setting up domain boundaries
pub struct DomainBoundaryBuilder {
    services: HashMap<String, Box<dyn ServiceDependency>>,
    event_bus: Option<Box<dyn EventBus>>,
}

impl DomainBoundaryBuilder {
    pub fn new() -> Self {
        Self {
            services: HashMap::new(),
            event_bus: None,
        }
    }

    /// Add a service dependency
    pub fn with_service(
        mut self,
        name: impl Into<String>,
        service: Box<dyn ServiceDependency>,
    ) -> Self {
        self.services.insert(name.into(), service);
        self
    }

    /// Set the event bus
    pub fn with_event_bus(mut self, event_bus: Box<dyn EventBus>) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    /// Build the domain boundary
    pub fn build(self) -> DomainBoundary {
        let event_bus = self
            .event_bus
            .unwrap_or_else(|| Box::new(InMemoryEventBus::new()));
        let mut boundary = DomainBoundary::new(event_bus);

        for (name, service) in self.services {
            boundary.services.insert(name, service);
        }

        boundary
    }
}

impl Default for DomainBoundaryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_service() {
        let mut service = MockService::new();

        service.set_query_response("get_user", "user123");

        let response = service.execute_query("get_user").await;
        assert!(response.is_ok());
        assert_eq!(response.unwrap(), "user123");

        let result = service.execute_command("create_user").await;
        assert!(result.is_ok());
        assert_eq!(service.commands().len(), 1);
    }

    #[test]
    fn test_event_bus() {
        let mut bus = InMemoryEventBus::new();

        bus.publish("event1");
        bus.publish("event2");

        assert_eq!(bus.events().len(), 2);

        bus.clear();
        assert_eq!(bus.events().len(), 0);
    }

    #[tokio::test]
    async fn test_domain_boundary() {
        let mut boundary = DomainBoundary::with_default_bus();

        let service = MockService::new();
        service.set_query_response("test", "result");

        boundary.register_service("test_service", Box::new(service));

        let result = boundary.execute_query("test_service", "test").await;
        assert_eq!(result.unwrap(), "result");

        boundary.publish_event("test_event");
        assert_eq!(boundary.events().len(), 1);
    }

    #[test]
    fn test_domain_boundary_builder() {
        let service1 = Box::new(MockService::new());
        let service2 = Box::new(MockService::new());

        let boundary = DomainBoundaryBuilder::new()
            .with_service("service1", service1)
            .with_service("service2", service2)
            .with_event_bus(Box::new(InMemoryEventBus::new()))
            .build();

        assert!(boundary.service("service1").is_some());
        assert!(boundary.service("service2").is_some());
        assert!(boundary.service("service3").is_none());
    }
}
