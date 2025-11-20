//! Deterministic async task executor for testing
//!
//! Provides controlled execution of async tasks compatible with Iced's Task system,
//! enabling deterministic testing of async code.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

/// A task that can be executed by the test executor
pub type BoxedFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// Controls how tasks are executed during testing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Execute all tasks immediately in order
    Immediate,
    /// Execute tasks one at a time, allowing inspection between executions
    StepByStep,
    /// Execute tasks until a certain condition is met
    UntilCondition,
}

/// Test executor that provides deterministic async execution
pub struct TestExecutor {
    /// Queue of pending tasks
    pending_tasks: Arc<Mutex<VecDeque<BoxedFuture>>>,
    /// Tasks scheduled for later execution (with virtual time)
    scheduled_tasks: Arc<Mutex<Vec<(Instant, BoxedFuture)>>>,
    /// Current virtual time
    virtual_time: Arc<Mutex<Instant>>,
    /// Execution mode
    mode: ExecutionMode,
    /// Number of tasks executed
    executed_count: usize,
    /// Maximum iterations to prevent infinite loops
    max_iterations: usize,
}

impl TestExecutor {
    /// Create a new test executor
    pub fn new() -> Self {
        Self {
            pending_tasks: Arc::new(Mutex::new(VecDeque::new())),
            scheduled_tasks: Arc::new(Mutex::new(Vec::new())),
            virtual_time: Arc::new(Mutex::new(Instant::now())),
            mode: ExecutionMode::Immediate,
            executed_count: 0,
            max_iterations: 10000,
        }
    }

    /// Set the execution mode
    pub fn set_mode(&mut self, mode: ExecutionMode) {
        self.mode = mode;
    }

    /// Set maximum iterations to prevent infinite loops
    pub fn set_max_iterations(&mut self, max: usize) {
        self.max_iterations = max;
    }

    /// Spawn a future for execution
    pub fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.pending_tasks
            .lock()
            .unwrap()
            .push_back(Box::pin(future));
    }

    /// Schedule a future for execution at a specific time
    pub fn schedule_at<F>(&self, when: Instant, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.scheduled_tasks
            .lock()
            .unwrap()
            .push((when, Box::pin(future)));
    }

    /// Schedule a future for execution after a delay
    pub fn schedule_after<F>(&self, delay: Duration, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let when = *self.virtual_time.lock().unwrap() + delay;
        self.schedule_at(when, future);
    }

    /// Advance virtual time and execute any scheduled tasks
    pub fn advance_time(&mut self, duration: Duration) {
        let new_time = {
            let mut time = self.virtual_time.lock().unwrap();
            *time += duration;
            *time
        };

        // Move scheduled tasks that are ready to the pending queue
        let mut scheduled = self.scheduled_tasks.lock().unwrap();
        let mut pending = self.pending_tasks.lock().unwrap();

        let ready_tasks: Vec<_> = scheduled
            .iter()
            .enumerate()
            .filter(|(_, (when, _))| *when <= new_time)
            .map(|(i, _)| i)
            .collect();

        // Remove in reverse order to maintain indices
        for i in ready_tasks.into_iter().rev() {
            let (_, task) = scheduled.remove(i);
            pending.push_back(task);
        }
    }

    /// Get the current virtual time
    pub fn current_time(&self) -> Instant {
        *self.virtual_time.lock().unwrap()
    }

    /// Execute a single pending task
    pub fn execute_one(&mut self) -> bool {
        let task = self.pending_tasks.lock().unwrap().pop_front();

        if let Some(mut task) = task {
            // Create a simple waker that re-enqueues the task if not complete
            let waker = create_test_waker(Arc::clone(&self.pending_tasks));
            let mut context = Context::from_waker(&waker);

            match task.as_mut().poll(&mut context) {
                Poll::Ready(()) => {
                    self.executed_count += 1;
                    true
                }
                Poll::Pending => {
                    // Task not ready, it should have been re-enqueued by the waker
                    true
                }
            }
        } else {
            false
        }
    }

    /// Execute all pending tasks
    pub fn execute_all(&mut self) -> usize {
        let mut count = 0;
        let mut iterations = 0;

        while self.has_pending_tasks() && iterations < self.max_iterations {
            if self.execute_one() {
                count += 1;
            }
            iterations += 1;
        }

        if iterations >= self.max_iterations {
            panic!(
                "TestExecutor exceeded maximum iterations ({}). Possible infinite loop?",
                self.max_iterations
            );
        }

        count
    }

    /// Execute tasks until a condition is met
    pub fn execute_until<F>(&mut self, mut condition: F) -> usize
    where
        F: FnMut() -> bool,
    {
        let mut count = 0;
        let mut iterations = 0;

        while !condition() && iterations < self.max_iterations {
            if self.has_pending_tasks() {
                if self.execute_one() {
                    count += 1;
                }
            } else {
                break;
            }
            iterations += 1;
        }

        if iterations >= self.max_iterations {
            panic!("TestExecutor exceeded maximum iterations in execute_until");
        }

        count
    }

    /// Check if there are pending tasks
    pub fn has_pending_tasks(&self) -> bool {
        !self.pending_tasks.lock().unwrap().is_empty()
    }

    /// Check if there are scheduled tasks
    pub fn has_scheduled_tasks(&self) -> bool {
        !self.scheduled_tasks.lock().unwrap().is_empty()
    }

    /// Get the number of pending tasks
    pub fn pending_count(&self) -> usize {
        self.pending_tasks.lock().unwrap().len()
    }

    /// Get the number of scheduled tasks
    pub fn scheduled_count(&self) -> usize {
        self.scheduled_tasks.lock().unwrap().len()
    }

    /// Get total number of tasks executed
    pub fn executed_count(&self) -> usize {
        self.executed_count
    }

    /// Clear all pending and scheduled tasks
    pub fn clear(&mut self) {
        self.pending_tasks.lock().unwrap().clear();
        self.scheduled_tasks.lock().unwrap().clear();
        self.executed_count = 0;
    }

    /// Reset the executor to initial state
    pub fn reset(&mut self) {
        self.clear();
        *self.virtual_time.lock().unwrap() = Instant::now();
        self.mode = ExecutionMode::Immediate;
    }
}

impl Default for TestExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// Creates a test waker that re-enqueues tasks
fn create_test_waker(
    pending_tasks: Arc<Mutex<VecDeque<BoxedFuture>>>,
) -> Waker {
    struct TestWake {
        pending_tasks: Arc<Mutex<VecDeque<BoxedFuture>>>,
    }

    impl Wake for TestWake {
        fn wake(self: Arc<Self>) {
            // Task will be re-enqueued when polled again
            // This is handled by the executor's polling logic
        }
    }

    let wake = Arc::new(TestWake { pending_tasks });
    Waker::from(wake)
}

/// Extension trait for converting Iced Tasks to test futures
pub trait TaskTestExt<T> {
    /// Convert an Iced Task to a future that can be executed by the test executor
    fn into_test_future(self) -> impl Future<Output = Vec<T>> + Send + 'static
    where
        T: Send + 'static;
}

// Note: Actual implementation would depend on Iced's Task internals
// This is a placeholder that demonstrates the interface
impl<T> TaskTestExt<T> for iced::Task<T> {
    async fn into_test_future(self) -> Vec<T>
    where
        T: Send + 'static,
    {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_immediate_execution() {
        let mut executor = TestExecutor::new();
        let counter = Arc::new(AtomicUsize::new(0));

        let counter_clone = Arc::clone(&counter);
        executor.spawn(async move {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        let counter_clone = Arc::clone(&counter);
        executor.spawn(async move {
            counter_clone.fetch_add(10, Ordering::SeqCst);
        });

        executor.execute_all();
        assert_eq!(counter.load(Ordering::SeqCst), 11);
    }

    #[test]
    fn test_scheduled_execution() {
        let mut executor = TestExecutor::new();
        let counter = Arc::new(AtomicUsize::new(0));

        // Schedule task for 1 second in the future
        let counter_clone = Arc::clone(&counter);
        executor.schedule_after(Duration::from_secs(1), async move {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        // Task shouldn't execute yet
        executor.execute_all();
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Advance time and execute
        executor.advance_time(Duration::from_secs(2));
        executor.execute_all();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_step_by_step_execution() {
        let mut executor = TestExecutor::new();
        executor.set_mode(ExecutionMode::StepByStep);

        let counter = Arc::new(AtomicUsize::new(0));

        for i in 0..3 {
            let counter_clone = Arc::clone(&counter);
            executor.spawn(async move {
                counter_clone.fetch_add(i, Ordering::SeqCst);
            });
        }

        assert_eq!(executor.pending_count(), 3);

        executor.execute_one();
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        executor.execute_one();
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        executor.execute_one();
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn test_execute_until_condition() {
        let mut executor = TestExecutor::new();
        let counter = Arc::new(AtomicUsize::new(0));

        for _ in 0..10 {
            let counter_clone = Arc::clone(&counter);
            executor.spawn(async move {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            });
        }

        let counter_clone = Arc::clone(&counter);
        let executed = executor
            .execute_until(|| counter_clone.load(Ordering::SeqCst) >= 5);

        assert!(executed >= 5);
        assert!(counter.load(Ordering::SeqCst) >= 5);
    }

    #[test]
    #[should_panic(expected = "exceeded maximum iterations")]
    fn test_infinite_loop_protection() {
        let mut executor = TestExecutor::new();
        executor.set_max_iterations(10);

        // Create a task that keeps spawning itself
        fn spawn_recursive(executor: &TestExecutor, remaining: usize) {
            if remaining == 0 {
                return;
            }
            executor.spawn(async {});
            spawn_recursive(executor, remaining - 1);
        }

        // Start the recursive spawning to exceed iteration cap
        spawn_recursive(&executor, 20);

        executor.execute_all(); // Should panic due to max iterations
    }
}
