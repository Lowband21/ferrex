//! Test harness for managing test lifecycle
//!
//! Provides setup, execution, teardown, and isolation for domain tests.

use futures::FutureExt;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Configuration for the test harness
#[derive(Debug, Clone)]
pub struct HarnessConfig {
    /// Timeout for test execution
    pub timeout: Duration,
    /// Whether to capture panics
    pub capture_panics: bool,
    /// Whether to record operations
    pub record_operations: bool,
    /// Whether to run in isolation (cleanup after each test)
    pub isolate: bool,
    /// Random seed for deterministic tests
    pub seed: Option<u64>,
}

impl Default for HarnessConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            capture_panics: true,
            record_operations: true,
            isolate: true,
            seed: None,
        }
    }
}

/// Result of running a test
#[derive(Debug)]
pub struct TestResult {
    /// Whether the test passed
    pub passed: bool,
    /// Test execution duration
    pub duration: Duration,
    /// Error message if test failed
    pub error: Option<String>,
    /// Panic message if test panicked
    pub panic_message: Option<String>,
    /// Recorded operations (if recording was enabled)
    pub operations: Vec<String>,
}

impl TestResult {
    /// Create a successful test result
    pub fn success(duration: Duration) -> Self {
        Self {
            passed: true,
            duration,
            error: None,
            panic_message: None,
            operations: Vec::new(),
        }
    }

    /// Create a failed test result
    pub fn failure(duration: Duration, error: String) -> Self {
        Self {
            passed: false,
            duration,
            error: Some(error),
            panic_message: None,
            operations: Vec::new(),
        }
    }

    /// Create a panic result
    pub fn panic(duration: Duration, panic_message: String) -> Self {
        Self {
            passed: false,
            duration,
            error: None,
            panic_message: Some(panic_message),
            operations: Vec::new(),
        }
    }
}

/// Test harness that manages test lifecycle
pub struct TestHarness {
    config: HarnessConfig,
    setup_hooks: Vec<Box<dyn Fn() + Send + Sync>>,
    teardown_hooks: Vec<Box<dyn Fn() + Send + Sync>>,
    global_state: Arc<Mutex<Option<Box<dyn std::any::Any + Send>>>>,
}

impl TestHarness {
    /// Create a new test harness
    pub fn new() -> Self {
        Self::with_config(HarnessConfig::default())
    }

    /// Create a test harness with custom configuration
    pub fn with_config(config: HarnessConfig) -> Self {
        Self {
            config,
            setup_hooks: Vec::new(),
            teardown_hooks: Vec::new(),
            global_state: Arc::new(Mutex::new(None)),
        }
    }

    /// Add a setup hook that runs before each test
    pub fn add_setup<F>(mut self, hook: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.setup_hooks.push(Box::new(hook));
        self
    }

    /// Add a teardown hook that runs after each test
    pub fn add_teardown<F>(mut self, hook: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.teardown_hooks.push(Box::new(hook));
        self
    }

    /// Set global state that persists across test runs
    pub fn set_global_state<T>(self, state: T) -> Self
    where
        T: std::any::Any + Send + 'static,
    {
        *self.global_state.lock().unwrap() = Some(Box::new(state));
        self
    }

    /// Run a single test
    pub async fn run_test<F, Fut>(&self, name: &str, test_fn: F) -> TestResult
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<(), String>> + Send,
    {
        let start = Instant::now();

        // Run setup hooks
        self.run_setup();

        // Run the test with timeout and panic capture
        println!("capture_panics: {}", self.config.capture_panics);
        let result = if self.config.capture_panics {
            self.run_with_panic_capture(test_fn).await
        } else {
            self.run_without_panic_capture(test_fn).await
        };

        // Run teardown hooks
        self.run_teardown();

        // Create test result
        let duration = start.elapsed();
        match result {
            Ok(Ok(())) => TestResult::success(duration),
            Ok(Err(error)) => TestResult::failure(duration, error),
            Err(panic_msg) => TestResult::panic(duration, panic_msg),
        }
    }

    /// Run multiple tests
    pub async fn run_tests<'a, I, F, Fut>(
        &self,
        tests: I,
    ) -> Vec<(&'a str, TestResult)>
    where
        I: IntoIterator<Item = (&'a str, F)>,
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<(), String>> + Send,
    {
        let mut results = Vec::new();

        for (name, test_fn) in tests {
            let result = self.run_test(name, test_fn).await;
            results.push((name, result));
        }

        results
    }

    /// Run a test suite
    pub async fn run_suite<S>(&self, suite: &S) -> SuiteResult
    where
        S: TestSuite,
    {
        let start = Instant::now();
        let mut results = Vec::new();

        let suite_name = suite.name().to_string();

        // Run suite setup
        if let Err(e) = suite.setup().await {
            return SuiteResult {
                name: suite_name,
                passed: 0,
                failed: 1,
                duration: start.elapsed(),
                error: Some(format!("Suite setup failed: {}", e)),
            };
        }

        // Run all tests in the suite
        for test in suite.tests() {
            let test_name = test.name.clone();
            let result = self.run_test(&test_name, || test.run_owned()).await;
            results.push((test_name, result));
        }

        // Run suite teardown
        if let Err(e) = suite.teardown().await {
            return SuiteResult {
                name: suite_name.clone(),
                passed: results.iter().filter(|(_, r)| r.passed).count(),
                failed: results.iter().filter(|(_, r)| !r.passed).count() + 1,
                duration: start.elapsed(),
                error: Some(format!("Suite teardown failed: {}", e)),
            };
        }

        SuiteResult {
            name: suite_name,
            passed: results.iter().filter(|(_, r)| r.passed).count(),
            failed: results.iter().filter(|(_, r)| !r.passed).count(),
            duration: start.elapsed(),
            error: None,
        }
    }

    /// Run setup hooks
    fn run_setup(&self) {
        for hook in &self.setup_hooks {
            hook();
        }
    }

    /// Run teardown hooks
    fn run_teardown(&self) {
        for hook in &self.teardown_hooks {
            hook();
        }
    }

    /// Run test with panic capture
    async fn run_with_panic_capture<F, Fut>(
        &self,
        test_fn: F,
    ) -> Result<Result<(), String>, String>
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<(), String>> + Send,
    {
        let result = AssertUnwindSafe(test_fn()).catch_unwind().await;

        match result {
            Ok(test_result) => Ok(test_result),
            Err(panic) => {
                let msg = if let Some(s) = panic.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = panic.downcast_ref::<&str>() {
                    s.to_string()
                } else {
                    "Unknown panic".to_string()
                };
                Err(msg)
            }
        }
    }

    /// Run test without panic capture
    async fn run_without_panic_capture<F, Fut>(
        &self,
        test_fn: F,
    ) -> Result<Result<(), String>, String>
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<(), String>> + Send,
    {
        Ok(test_fn().await)
    }
}

impl Default for TestHarness {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for test suites
pub trait TestSuite: Send + Sync {
    /// Name of the test suite
    fn name(&self) -> &str;

    /// Setup function run before all tests
    fn setup(&self) -> impl Future<Output = Result<(), String>> + Send {
        async { Ok(()) }
    }

    /// Teardown function run after all tests
    fn teardown(&self) -> impl Future<Output = Result<(), String>> + Send {
        async { Ok(()) }
    }

    /// Get all tests in the suite - returns owned tests from internal state
    fn tests(&self) -> Vec<Test>;
}

/// A single test in a suite
pub struct Test {
    name: String,
    run: Box<
        dyn FnOnce() -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>>
            + Send,
    >,
}

use std::pin::Pin;

impl Test {
    /// Create a new test
    pub fn new<F, Fut>(name: impl Into<String>, run: F) -> Self
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        Self {
            name: name.into(),
            run: Box::new(move || Box::pin(run())),
        }
    }

    /// Get the test name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Consume the test and return its run function
    pub fn run_owned(self) -> impl Future<Output = Result<(), String>> + Send {
        (self.run)()
    }
}

/// Result of running a test suite
#[derive(Debug)]
pub struct SuiteResult {
    /// Name of the suite
    pub name: String,
    /// Number of passed tests
    pub passed: usize,
    /// Number of failed tests
    pub failed: usize,
    /// Total duration
    pub duration: Duration,
    /// Error if suite setup/teardown failed
    pub error: Option<String>,
}

/// Macro to define a test suite
#[macro_export]
macro_rules! test_suite {
    ($name:ident {
        $(
            test $test_name:ident() $test_body:block
        )*
    }) => {
        struct $name;

        impl TestSuite for $name {
            fn name(&self) -> String {
                stringify!($name).to_string()
            }

            fn tests(self) -> Vec<Test> {
                vec![
                    $(
                        Test::new(stringify!($test_name), || async move $test_body),
                    )*
                ]
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_harness_success() {
        let harness = TestHarness::new();

        let result =
            harness.run_test("success_test", || async { Ok(()) }).await;

        assert!(result.passed);
        assert!(result.error.is_none());
        assert!(result.panic_message.is_none());
    }

    #[tokio::test]
    async fn test_harness_failure() {
        let harness = TestHarness::new();

        let result = harness
            .run_test("failure_test", || async {
                Err("Test failed".to_string())
            })
            .await;

        assert!(!result.passed);
        assert_eq!(result.error, Some("Test failed".to_string()));
    }

    #[tokio::test]
    async fn test_harness_panic() {
        let harness = TestHarness::new();

        let result = harness
            .run_test("panic_test", || async {
                panic!("Test panic");
            })
            .await;

        assert!(!result.passed);
        assert!(result.panic_message.is_some());
    }

    #[tokio::test]
    async fn test_setup_teardown_hooks() {
        let counter = Arc::new(Mutex::new(0));
        let counter_clone = Arc::clone(&counter);
        let counter_clone2 = Arc::clone(&counter);

        let harness = TestHarness::new()
            .add_setup(move || {
                *counter_clone.lock().unwrap() += 1;
            })
            .add_teardown(move || {
                *counter_clone2.lock().unwrap() += 10;
            });

        harness.run_test("hook_test", || async { Ok(()) }).await;

        assert_eq!(*counter.lock().unwrap(), 11);
    }
}
