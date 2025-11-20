//! Test recorder for debugging and operation tracking
//!
//! Records all operations, state changes, and events during test execution
//! to provide rich context when tests fail.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fmt::{self, Display};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Type of operation being recorded
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationType {
    Command,
    Query,
    Event,
    StateChange,
    Assertion,
    MockCall,
    TimeAdvance,
    Custom(String),
}

/// A single recorded operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    /// Type of operation
    pub op_type: OperationType,
    /// Description of the operation
    pub description: String,
    /// When the operation occurred (relative to test start)
    pub timestamp: Duration,
    /// Optional data associated with the operation
    pub data: Option<String>,
    /// Whether the operation succeeded
    pub success: bool,
    /// Error message if operation failed
    pub error: Option<String>,
}

impl Display for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = if self.success { "✓" } else { "✗" };
        let timestamp_ms = self.timestamp.as_millis();

        write!(
            f,
            "[{:>6}ms] {} {:?}: {}",
            timestamp_ms, status, self.op_type, self.description
        )?;

        if let Some(data) = &self.data {
            write!(f, "\n           Data: {}", data)?;
        }

        if let Some(error) = &self.error {
            write!(f, "\n           Error: {}", error)?;
        }

        Ok(())
    }
}

/// Snapshot of state at a point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// When the snapshot was taken
    pub timestamp: Duration,
    /// Serialized state
    pub state: String,
    /// Label for this snapshot
    pub label: String,
}

/// Records operations and state for debugging test failures
#[derive(Clone)]
pub struct TestRecorder {
    operations: Arc<Mutex<VecDeque<Operation>>>,
    snapshots: Arc<Mutex<Vec<StateSnapshot>>>,
    start_time: Instant,
    max_operations: usize,
    recording_enabled: Arc<Mutex<bool>>,
}

impl TestRecorder {
    /// Create a new test recorder
    pub fn new() -> Self {
        Self {
            operations: Arc::new(Mutex::new(VecDeque::new())),
            snapshots: Arc::new(Mutex::new(Vec::new())),
            start_time: Instant::now(),
            max_operations: 1000,
            recording_enabled: Arc::new(Mutex::new(true)),
        }
    }

    /// Set maximum number of operations to keep (for memory management)
    pub fn set_max_operations(&mut self, max: usize) {
        self.max_operations = max;
    }

    /// Enable or disable recording
    pub fn set_recording(&self, enabled: bool) {
        *self.recording_enabled.lock().unwrap() = enabled;
    }

    /// Check if recording is enabled
    pub fn is_recording(&self) -> bool {
        *self.recording_enabled.lock().unwrap()
    }

    /// Record a command execution
    pub fn record_command(&self, description: String) {
        self.record_operation(OperationType::Command, description, None, true, None);
    }

    /// Record a command with data
    pub fn record_command_with_data(&self, description: String, data: String) {
        self.record_operation(OperationType::Command, description, Some(data), true, None);
    }

    /// Record a failed command
    pub fn record_command_failure(&self, description: String, error: String) {
        self.record_operation(
            OperationType::Command,
            description,
            None,
            false,
            Some(error),
        );
    }

    /// Record a query execution
    pub fn record_query(&self, description: String) {
        self.record_operation(OperationType::Query, description, None, true, None);
    }

    /// Record a query with result
    pub fn record_query_with_result(&self, description: String, result: String) {
        self.record_operation(OperationType::Query, description, Some(result), true, None);
    }

    /// Record an event
    pub fn record_event(&self, description: String) {
        self.record_operation(OperationType::Event, description, None, true, None);
    }

    /// Record a state change
    pub fn record_state_change(&self, description: String, new_state: String) {
        self.record_operation(
            OperationType::StateChange,
            description,
            Some(new_state),
            true,
            None,
        );
    }

    /// Record an assertion
    pub fn record_assertion(&self, description: String, success: bool) {
        let error = if !success {
            Some("Assertion failed".to_string())
        } else {
            None
        };
        self.record_operation(OperationType::Assertion, description, None, success, error);
    }

    /// Record a mock call
    pub fn record_mock_call(&self, service: String, method: String, args: String) {
        let description = format!("{}.{}", service, method);
        self.record_operation(OperationType::MockCall, description, Some(args), true, None);
    }

    /// Record time advancement
    pub fn record_time_advance(&self, duration: Duration) {
        let description = format!("Advanced time by {:?}", duration);
        self.record_operation(OperationType::TimeAdvance, description, None, true, None);
    }

    /// Record a custom operation
    pub fn record_custom(&self, label: String, description: String) {
        self.record_operation(OperationType::Custom(label), description, None, true, None);
    }

    /// Core recording function
    fn record_operation(
        &self,
        op_type: OperationType,
        description: String,
        data: Option<String>,
        success: bool,
        error: Option<String>,
    ) {
        if !self.is_recording() {
            return;
        }

        let operation = Operation {
            op_type,
            description,
            timestamp: self.start_time.elapsed(),
            data,
            success,
            error,
        };

        let mut ops = self.operations.lock().unwrap();
        ops.push_back(operation);

        // Trim old operations if we exceed the limit
        while ops.len() > self.max_operations {
            ops.pop_front();
        }
    }

    /// Take a state snapshot
    pub fn snapshot_state<S>(&self, state: &S, label: String)
    where
        S: fmt::Debug,
    {
        if !self.is_recording() {
            return;
        }

        let snapshot = StateSnapshot {
            timestamp: self.start_time.elapsed(),
            state: format!("{:?}", state),
            label,
        };

        self.snapshots.lock().unwrap().push(snapshot);
    }

    /// Get all recorded operations
    pub fn operations(&self) -> Vec<Operation> {
        self.operations.lock().unwrap().iter().cloned().collect()
    }

    /// Get operations of a specific type
    pub fn operations_of_type(&self, op_type: OperationType) -> Vec<Operation> {
        self.operations
            .lock()
            .unwrap()
            .iter()
            .filter(|op| std::mem::discriminant(&op.op_type) == std::mem::discriminant(&op_type))
            .cloned()
            .collect()
    }

    /// Get failed operations
    pub fn failed_operations(&self) -> Vec<Operation> {
        self.operations
            .lock()
            .unwrap()
            .iter()
            .filter(|op| !op.success)
            .cloned()
            .collect()
    }

    /// Get all state snapshots
    pub fn snapshots(&self) -> Vec<StateSnapshot> {
        self.snapshots.lock().unwrap().clone()
    }

    /// Get the most recent snapshot
    pub fn latest_snapshot(&self) -> Option<StateSnapshot> {
        self.snapshots.lock().unwrap().last().cloned()
    }

    /// Clear all recordings
    pub fn clear(&self) {
        self.operations.lock().unwrap().clear();
        self.snapshots.lock().unwrap().clear();
    }

    /// Generate a failure report
    pub fn failure_report(&self, test_name: &str, failure_reason: &str) -> String {
        let mut report = String::new();

        report.push_str(&format!("Test Failure Report: {}\n", test_name));
        report.push_str(&format!("Reason: {}\n", failure_reason));
        report.push_str(&format!("Duration: {:?}\n", self.start_time.elapsed()));
        report.push('\n');

        // Add recent operations
        report.push_str("Recent Operations:\n");
        report.push_str("-----------------\n");
        let ops = self.operations();
        let start = ops.len().saturating_sub(20);
        for op in &ops[start..] {
            report.push_str(&format!("{}\n", op));
        }

        // Add failed operations
        let failed = self.failed_operations();
        if !failed.is_empty() {
            report.push_str("\nFailed Operations:\n");
            report.push_str("-----------------\n");
            for op in &failed {
                report.push_str(&format!("{}\n", op));
            }
        }

        // Add latest state snapshot
        if let Some(snapshot) = self.latest_snapshot() {
            report.push_str("\nLatest State Snapshot:\n");
            report.push_str("---------------------\n");
            report.push_str(&format!("Label: {}\n", snapshot.label));
            report.push_str(&format!("Time: {:?}\n", snapshot.timestamp));
            report.push_str(&format!("State:\n{}\n", snapshot.state));
        }

        report
    }

    /// Export recording to JSON
    pub fn export_json(&self) -> Result<String, serde_json::Error> {
        #[derive(Serialize)]
        struct Export {
            operations: Vec<Operation>,
            snapshots: Vec<StateSnapshot>,
            duration: Duration,
        }

        let export = Export {
            operations: self.operations(),
            snapshots: self.snapshots(),
            duration: self.start_time.elapsed(),
        };

        serde_json::to_string_pretty(&export)
    }

    /// Generate a replay script
    pub fn generate_replay(&self) -> String {
        let mut replay = String::new();
        replay.push_str("// Test Replay Script\n");
        replay.push_str("// Generated from recorded operations\n\n");

        for op in self.operations() {
            match op.op_type {
                OperationType::Command => {
                    replay.push_str(&format!(
                        "// [{:?}] Execute command: {}\n",
                        op.timestamp, op.description
                    ));
                    if let Some(data) = &op.data {
                        replay.push_str(&format!("ctx.execute_command({});\n", data));
                    }
                }
                OperationType::Query => {
                    replay.push_str(&format!(
                        "// [{:?}] Execute query: {}\n",
                        op.timestamp, op.description
                    ));
                    if let Some(data) = &op.data {
                        replay.push_str(&format!("let result = ctx.execute_query({});\n", data));
                    }
                }
                OperationType::TimeAdvance => {
                    replay.push_str(&format!("// [{:?}] {}\n", op.timestamp, op.description));
                    replay.push_str(&format!(
                        "time_provider.advance(Duration::from_millis({}));\n",
                        op.timestamp.as_millis()
                    ));
                }
                _ => {
                    replay.push_str(&format!(
                        "// [{:?}] {:?}: {}\n",
                        op.timestamp, op.op_type, op.description
                    ));
                }
            }
            replay.push('\n');
        }

        replay
    }
}

impl Default for TestRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for TestRecorder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TestRecorder")
            .field("operations_count", &self.operations.lock().unwrap().len())
            .field("snapshots_count", &self.snapshots.lock().unwrap().len())
            .field("elapsed", &self.start_time.elapsed())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recording_operations() {
        let recorder = TestRecorder::new();

        recorder.record_command("CreateUser".to_string());
        recorder.record_query("GetUser".to_string());
        recorder.record_event("UserCreated".to_string());

        let ops = recorder.operations();
        assert_eq!(ops.len(), 3);

        assert!(matches!(ops[0].op_type, OperationType::Command));
        assert!(matches!(ops[1].op_type, OperationType::Query));
        assert!(matches!(ops[2].op_type, OperationType::Event));
    }

    #[test]
    fn test_recording_failures() {
        let recorder = TestRecorder::new();

        recorder.record_command_failure(
            "InvalidCommand".to_string(),
            "Command not recognized".to_string(),
        );

        let failed = recorder.failed_operations();
        assert_eq!(failed.len(), 1);
        assert!(!failed[0].success);
        assert_eq!(failed[0].error, Some("Command not recognized".to_string()));
    }

    #[test]
    fn test_state_snapshots() {
        let recorder = TestRecorder::new();

        #[derive(Debug)]
        struct TestState {
            value: i32,
        }

        let state = TestState { value: 42 };
        recorder.snapshot_state(&state, "initial".to_string());

        let state = TestState { value: 100 };
        recorder.snapshot_state(&state, "after_update".to_string());

        let snapshots = recorder.snapshots();
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].label, "initial");
        assert_eq!(snapshots[1].label, "after_update");
    }

    #[test]
    fn test_max_operations_limit() {
        let mut recorder = TestRecorder::new();
        recorder.set_max_operations(5);

        for i in 0..10 {
            recorder.record_event(format!("Event {}", i));
        }

        let ops = recorder.operations();
        assert_eq!(ops.len(), 5);
        assert_eq!(ops[0].description, "Event 5");
        assert_eq!(ops[4].description, "Event 9");
    }

    #[test]
    fn test_recording_disabled() {
        let recorder = TestRecorder::new();
        recorder.set_recording(false);

        recorder.record_command("ShouldNotRecord".to_string());
        assert_eq!(recorder.operations().len(), 0);

        recorder.set_recording(true);
        recorder.record_command("ShouldRecord".to_string());
        assert_eq!(recorder.operations().len(), 1);
    }

    #[test]
    fn test_failure_report() {
        let recorder = TestRecorder::new();

        recorder.record_command("CreateUser".to_string());
        recorder.record_command_failure("UpdateUser".to_string(), "User not found".to_string());
        recorder.record_event("SystemError".to_string());

        let report = recorder.failure_report("test_user_operations", "Assertion failed");

        assert!(report.contains("Test Failure Report: test_user_operations"));
        assert!(report.contains("Reason: Assertion failed"));
        assert!(report.contains("Failed Operations:"));
        assert!(report.contains("User not found"));
    }

    #[test]
    fn test_export_json() {
        let recorder = TestRecorder::new();

        recorder.record_command("TestCommand".to_string());
        recorder.snapshot_state(&"test_state", "snapshot1".to_string());

        let json = recorder.export_json().unwrap();
        assert!(json.contains("TestCommand"));
        assert!(json.contains("snapshot1"));
    }
}
