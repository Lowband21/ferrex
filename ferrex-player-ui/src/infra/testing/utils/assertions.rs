//! Custom assertions for domain testing
//!
//! Provides async-aware assertions and eventually-consistent checks.

use std::future::Future;
use std::time::Duration;
use tokio::time::{sleep, timeout};

/// Extension trait for async assertions
pub trait AsyncAssertions {
    /// Assert that a future completes within a timeout
    fn completes_within(
        self,
        duration: Duration,
    ) -> impl Future<Output = Result<Self::Output, String>>
    where
        Self: Future + Sized,
        Self::Output: Send;

    /// Assert that a future does not complete within a timeout
    fn does_not_complete_within(
        self,
        duration: Duration,
    ) -> impl Future<Output = Result<(), String>>
    where
        Self: Future + Sized;
}

impl<F> AsyncAssertions for F
where
    F: Future,
{
    async fn completes_within(
        self,
        duration: Duration,
    ) -> Result<F::Output, String>
    where
        F::Output: Send,
    {
        timeout(duration, self).await.map_err(|_| {
            format!("Future did not complete within {:?}", duration)
        })
    }

    async fn does_not_complete_within(
        self,
        duration: Duration,
    ) -> Result<(), String> {
        match timeout(duration, self).await {
            Ok(_) => Err(format!(
                "Future completed within {:?} when it shouldn't have",
                duration
            )),
            Err(_) => Ok(()),
        }
    }
}

/// Extension trait for eventually-consistent assertions
pub trait EventuallyExt {
    /// Check that a condition eventually becomes true
    fn eventually<F>(
        condition: F,
        timeout_duration: Duration,
        check_interval: Duration,
    ) -> impl Future<Output = Result<(), String>>
    where
        F: Fn() -> bool + Send;

    /// Check that a condition eventually becomes true (async version)
    fn eventually_async<F, Fut>(
        condition: F,
        timeout_duration: Duration,
        check_interval: Duration,
    ) -> impl Future<Output = Result<(), String>>
    where
        F: Fn() -> Fut + Send,
        Fut: Future<Output = bool> + Send;

    /// Check that a value eventually equals expected
    fn eventually_equals<T, F>(
        getter: F,
        expected: T,
        timeout_duration: Duration,
        check_interval: Duration,
    ) -> impl Future<Output = Result<(), String>>
    where
        T: PartialEq + std::fmt::Debug + Send + Sync + 'static,
        F: Fn() -> T + Send;
}

#[derive(Debug, Clone, Copy)]
pub struct Eventually;

impl EventuallyExt for Eventually {
    async fn eventually<F>(
        condition: F,
        timeout_duration: Duration,
        check_interval: Duration,
    ) -> Result<(), String>
    where
        F: Fn() -> bool + Send,
    {
        let start = std::time::Instant::now();

        loop {
            if condition() {
                return Ok(());
            }

            if start.elapsed() >= timeout_duration {
                return Err(format!(
                    "Condition did not become true within {:?}",
                    timeout_duration
                ));
            }

            sleep(check_interval).await;
        }
    }

    async fn eventually_async<F, Fut>(
        condition: F,
        timeout_duration: Duration,
        check_interval: Duration,
    ) -> Result<(), String>
    where
        F: Fn() -> Fut + Send,
        Fut: Future<Output = bool> + Send,
    {
        let start = std::time::Instant::now();

        loop {
            if condition().await {
                return Ok(());
            }

            if start.elapsed() >= timeout_duration {
                return Err(format!(
                    "Async condition did not become true within {:?}",
                    timeout_duration
                ));
            }

            sleep(check_interval).await;
        }
    }

    async fn eventually_equals<T, F>(
        getter: F,
        expected: T,
        timeout_duration: Duration,
        check_interval: Duration,
    ) -> Result<(), String>
    where
        T: PartialEq + std::fmt::Debug + Send + Sync + 'static,
        F: Fn() -> T + Send,
    {
        let start = std::time::Instant::now();

        loop {
            let current = getter();
            if current == expected {
                return Ok(());
            }

            if start.elapsed() >= timeout_duration {
                return Err(format!(
                    "Value did not equal expected within {:?}. Last value: {:?}, Expected: {:?}",
                    timeout_duration, current, expected
                ));
            }

            sleep(check_interval).await;
        }
    }
}

/// Assertions for state transitions
pub trait StateAssertions {
    type State;

    /// Assert that state transitions follow a specific path
    fn assert_transition_path(
        &self,
        path: &[Self::State],
    ) -> Result<(), String>;

    /// Assert that state never enters certain states
    fn assert_never_enters(
        &self,
        forbidden: &[Self::State],
    ) -> Result<(), String>;

    /// Assert that state eventually reaches a target
    fn assert_eventually_reaches(
        &self,
        target: Self::State,
    ) -> Result<(), String>;
}

/// Helper macro for asserting async results
#[macro_export]
macro_rules! assert_async_ok {
    ($expr:expr_2021) => {
        match $expr.await {
            Ok(val) => val,
            Err(e) => panic!("Assertion failed: {:?}", e),
        }
    };
    ($expr:expr_2021, $msg:literal) => {
        match $expr.await {
            Ok(val) => val,
            Err(e) => panic!("{}: {:?}", $msg, e),
        }
    };
}

/// Helper macro for asserting async errors
#[macro_export]
macro_rules! assert_async_err {
    ($expr:expr_2021) => {
        match $expr.await {
            Ok(_) => panic!("Expected error but got Ok"),
            Err(e) => e,
        }
    };
    ($expr:expr_2021, $msg:literal) => {
        match $expr.await {
            Ok(_) => panic!("{}: Expected error but got Ok", $msg),
            Err(e) => e,
        }
    };
}

/// Assert that events were emitted in a specific order
pub fn assert_event_sequence<E>(
    events: &[E],
    expected: &[E],
) -> Result<(), String>
where
    E: PartialEq + std::fmt::Debug,
{
    if events.len() < expected.len() {
        return Err(format!(
            "Not enough events. Got {} events, expected at least {}",
            events.len(),
            expected.len()
        ));
    }

    // Find the expected sequence in the events
    for window in events.windows(expected.len()) {
        if window == expected {
            return Ok(());
        }
    }

    Err(format!(
        "Event sequence not found. Events: {:?}, Expected sequence: {:?}",
        events, expected
    ))
}

/// Assert that certain events were never emitted
pub fn assert_events_never<E, F>(
    events: &[E],
    predicate: F,
) -> Result<(), String>
where
    E: std::fmt::Debug,
    F: Fn(&E) -> bool,
{
    for event in events {
        if predicate(event) {
            return Err(format!("Forbidden event was emitted: {:?}", event));
        }
    }
    Ok(())
}

/// Assert properties hold for all items in a collection
pub fn assert_all<T, F>(items: &[T], predicate: F) -> Result<(), String>
where
    T: std::fmt::Debug,
    F: Fn(&T) -> bool,
{
    for (i, item) in items.iter().enumerate() {
        if !predicate(item) {
            return Err(format!(
                "Assertion failed for item at index {}: {:?}",
                i, item
            ));
        }
    }
    Ok(())
}

/// Assert properties hold for at least one item
pub fn assert_any<T, F>(items: &[T], predicate: F) -> Result<(), String>
where
    T: std::fmt::Debug,
    F: Fn(&T) -> bool,
{
    for item in items {
        if predicate(item) {
            return Ok(());
        }
    }
    Err(format!(
        "No items matched the predicate. Items: {:?}",
        items
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn test_completes_within() {
        let future = async { 42 };
        let result = future.completes_within(Duration::from_secs(1)).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_does_not_complete_within() {
        let future = async {
            sleep(Duration::from_secs(2)).await;
            42
        };
        let result = future
            .does_not_complete_within(Duration::from_millis(100))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_eventually() {
        let counter = Arc::new(Mutex::new(0));
        let counter_clone = Arc::clone(&counter);

        tokio::spawn(async move {
            sleep(Duration::from_millis(100)).await;
            *counter_clone.lock().unwrap() = 5;
        });

        let counter_check = Arc::clone(&counter);
        let result = Eventually::eventually(
            move || *counter_check.lock().unwrap() == 5,
            Duration::from_secs(1),
            Duration::from_millis(50),
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_eventually_equals() {
        let value = Arc::new(Mutex::new(0));
        let value_clone = Arc::clone(&value);

        tokio::spawn(async move {
            sleep(Duration::from_millis(100)).await;
            *value_clone.lock().unwrap() = 42;
        });

        let value_check = Arc::clone(&value);
        let result = Eventually::eventually_equals(
            move || *value_check.lock().unwrap(),
            42,
            Duration::from_secs(1),
            Duration::from_millis(50),
        )
        .await;

        assert!(result.is_ok());
    }

    #[test]
    fn test_assert_event_sequence() {
        let events = vec!["a", "b", "c", "d", "e"];

        assert!(assert_event_sequence(&events, &["b", "c", "d"]).is_ok());
        assert!(assert_event_sequence(&events, &["a", "b"]).is_ok());
        assert!(assert_event_sequence(&events, &["d", "e"]).is_ok());
        assert!(assert_event_sequence(&events, &["a", "c"]).is_err());
    }

    #[test]
    fn test_assert_all() {
        let numbers = vec![2, 4, 6, 8];

        assert!(assert_all(&numbers, |n| n % 2 == 0).is_ok());
        assert!(assert_all(&numbers, |n| *n > 0).is_ok());
        assert!(assert_all(&numbers, |n| *n > 5).is_err());
    }

    #[test]
    fn test_assert_any() {
        let numbers = vec![1, 3, 5, 7];

        assert!(assert_any(&numbers, |n| *n == 5).is_ok());
        assert!(assert_any(&numbers, |n| n % 2 == 1).is_ok());
        assert!(assert_any(&numbers, |n| n % 2 == 0).is_err());
    }
}
