//! Test harness framework for Ferrex player domain-driven testing
//!
//! This module provides utilities for testing the domain-driven architecture
//! of the Ferrex player, including mock builders, assertion helpers, and
//! async task execution utilities.

pub mod assertions;
pub mod fixtures;
pub mod mocks;
pub mod scenario;
pub mod state_traits;

use ferrex_player::messages::DomainMessage;
use iced::Task;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Test context for managing test state and message history
pub struct TestContext {
    /// Messages that have been processed
    pub message_history: Arc<Mutex<Vec<DomainMessage>>>,
    /// Tasks that have been created
    pub task_history: Arc<Mutex<Vec<String>>>,
}

impl TestContext {
    pub fn new() -> Self {
        Self {
            message_history: Arc::new(Mutex::new(Vec::new())),
            task_history: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Record a message that was processed
    pub async fn record_message(&self, message: DomainMessage) {
        self.message_history.lock().await.push(message);
    }

    /// Record a task that was created
    pub async fn record_task(&self, task_description: String) {
        self.task_history.lock().await.push(task_description);
    }

    /// Get all processed messages
    pub async fn get_messages(&self) -> Vec<DomainMessage> {
        self.message_history.lock().await.clone()
    }

    /// Clear the test context
    pub async fn clear(&self) {
        self.message_history.lock().await.clear();
        self.task_history.lock().await.clear();
    }
}

/// Test runner for executing tasks in a test environment
pub struct TestTaskRunner {
    runtime: tokio::runtime::Runtime,
}

impl TestTaskRunner {
    pub fn new() -> Self {
        Self {
            runtime: tokio::runtime::Runtime::new().unwrap(),
        }
    }

    /// Execute a task and collect all messages it produces
    pub fn run_task<M>(&mut self, task: Task<M>) -> Vec<M>
    where
        M: Clone + Send + 'static,
    {
        let messages = Arc::new(Mutex::new(Vec::new()));
        let messages_clone = messages.clone();

        // Convert Iced task to futures and execute
        // Note: This is a simplified version - actual implementation
        // will need to handle Iced's task execution model properly
        self.runtime.block_on(async move {
            // Task execution logic here
            // This will be implemented based on Iced's internal task handling
            vec![]
        })
    }
}

/// Assertion builder for fluent test assertions
pub struct TestAssertion<T> {
    value: T,
}

impl<T> TestAssertion<T> {
    pub fn new(value: T) -> Self {
        Self { value }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_context_records_messages() {
        let ctx = TestContext::new();
        ctx.record_message(DomainMessage::NoOp).await;

        let messages = ctx.get_messages().await;
        assert_eq!(messages.len(), 1);
    }
}
