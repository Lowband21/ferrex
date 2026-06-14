//! Virtual time provider for deterministic testing
//!
//! Provides abstraction over time to enable deterministic testing of time-dependent code.

use chrono::{DateTime, Utc};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// Trait for providing time in tests and production
pub trait TimeProvider: Send + Sync + 'static {
    /// Get the current instant
    fn now(&self) -> Instant;

    /// Get the current system time
    fn system_now(&self) -> SystemTime;

    /// Get the current UTC datetime
    fn utc_now(&self) -> DateTime<Utc>;

    /// Sleep for a duration (in tests, this advances virtual time)
    fn sleep(
        &self,
        duration: Duration,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>>;

    /// Create a timer that fires after a duration
    fn timer(
        &self,
        duration: Duration,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>>;

    /// Clone the time provider into a boxed trait object
    fn clone_box(&self) -> Box<dyn TimeProvider>;
}

/// Production time provider that uses real system time
#[derive(Clone, Debug)]
pub struct SystemTimeProvider;

impl TimeProvider for SystemTimeProvider {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn system_now(&self) -> SystemTime {
        SystemTime::now()
    }

    fn utc_now(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn sleep(
        &self,
        duration: Duration,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(tokio::time::sleep(duration))
    }

    fn timer(
        &self,
        duration: Duration,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(tokio::time::sleep(duration))
    }

    fn clone_box(&self) -> Box<dyn TimeProvider> {
        Box::new(self.clone())
    }
}

/// Virtual time provider for testing
#[derive(Clone, Debug)]
pub struct VirtualTimeProvider {
    /// Current virtual instant
    instant: Arc<Mutex<Instant>>,
    /// Current virtual system time
    system_time: Arc<Mutex<SystemTime>>,
    /// Base instant for calculating offsets
    base_instant: Instant,
    /// Base system time for calculating offsets
    base_system_time: SystemTime,
    /// Pending timers
    timers: Arc<Mutex<Vec<VirtualTimer>>>,
}

/// A virtual timer that can be resolved when time advances
#[derive(Debug)]
struct VirtualTimer {
    deadline: Instant,
    waker: Option<std::task::Waker>,
}

impl VirtualTimeProvider {
    /// Create a new virtual time provider
    pub fn new() -> Self {
        let now = Instant::now();
        let system_now = SystemTime::now();

        Self {
            instant: Arc::new(Mutex::new(now)),
            system_time: Arc::new(Mutex::new(system_now)),
            base_instant: now,
            base_system_time: system_now,
            timers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Create a virtual time provider starting at a specific time
    pub fn new_at(start_time: DateTime<Utc>) -> Self {
        let now = Instant::now();
        let system_now = SystemTime::from(start_time);

        Self {
            instant: Arc::new(Mutex::new(now)),
            system_time: Arc::new(Mutex::new(system_now)),
            base_instant: now,
            base_system_time: system_now,
            timers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Advance time by a duration
    pub fn advance(&self, duration: Duration) {
        let new_instant = {
            let mut instant = self.instant.lock().unwrap();
            *instant += duration;
            *instant
        };

        {
            let mut system_time = self.system_time.lock().unwrap();
            *system_time += duration;
        }

        // Wake any timers that have expired
        self.wake_expired_timers(new_instant);
    }

    /// Set the current time to a specific instant
    pub fn set_instant(&self, instant: Instant) {
        *self.instant.lock().unwrap() = instant;
        self.wake_expired_timers(instant);
    }

    /// Set the current time to a specific system time
    pub fn set_system_time(&self, time: SystemTime) {
        let duration = time
            .duration_since(self.base_system_time)
            .unwrap_or_else(|_| Duration::from_secs(0));

        *self.system_time.lock().unwrap() = time;
        *self.instant.lock().unwrap() = self.base_instant + duration;

        self.wake_expired_timers(self.base_instant + duration);
    }

    /// Set the current time to a specific UTC datetime
    pub fn set_utc(&self, datetime: DateTime<Utc>) {
        self.set_system_time(SystemTime::from(datetime));
    }

    /// Advance time to the next timer deadline
    pub fn advance_to_next_timer(&self) -> Option<Duration> {
        let timers = self.timers.lock().unwrap();
        if let Some(next_timer) = timers.iter().min_by_key(|t| t.deadline) {
            let current = *self.instant.lock().unwrap();
            if next_timer.deadline > current {
                let duration = next_timer.deadline - current;
                drop(timers); // Release lock before advancing
                self.advance(duration);
                Some(duration)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Get the number of pending timers
    pub fn pending_timers(&self) -> usize {
        self.timers.lock().unwrap().len()
    }

    /// Wake all timers that have expired
    fn wake_expired_timers(&self, now: Instant) {
        let mut timers = self.timers.lock().unwrap();
        let mut i = 0;
        while i < timers.len() {
            if timers[i].deadline <= now {
                if let Some(waker) = timers[i].waker.take() {
                    waker.wake();
                }
                timers.remove(i);
            } else {
                i += 1;
            }
        }
    }

    /// Reset to initial time
    pub fn reset(&self) {
        *self.instant.lock().unwrap() = self.base_instant;
        *self.system_time.lock().unwrap() = self.base_system_time;
        self.timers.lock().unwrap().clear();
    }
}

impl Default for VirtualTimeProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeProvider for VirtualTimeProvider {
    fn now(&self) -> Instant {
        *self.instant.lock().unwrap()
    }

    fn system_now(&self) -> SystemTime {
        *self.system_time.lock().unwrap()
    }

    fn utc_now(&self) -> DateTime<Utc> {
        self.system_now().into()
    }

    fn sleep(
        &self,
        duration: Duration,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(VirtualSleep::new(self, duration))
    }

    fn timer(
        &self,
        duration: Duration,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(VirtualSleep::new(self, duration))
    }

    fn clone_box(&self) -> Box<dyn TimeProvider> {
        Box::new(self.clone())
    }
}

/// Future that completes when virtual time advances past a deadline
struct VirtualSleep {
    provider: VirtualTimeProvider,
    deadline: Instant,
    registered: bool,
}

impl VirtualSleep {
    fn new(provider: &VirtualTimeProvider, duration: Duration) -> Self {
        let deadline = provider.now() + duration;
        Self {
            provider: provider.clone(),
            deadline,
            registered: false,
        }
    }
}

impl std::future::Future for VirtualSleep {
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let now = self.provider.now();

        if now >= self.deadline {
            // Timer has expired
            std::task::Poll::Ready(())
        } else {
            // Register waker if not already registered
            if !self.registered {
                {
                    let mut timers = self.provider.timers.lock().unwrap();
                    timers.push(VirtualTimer {
                        deadline: self.deadline,
                        waker: Some(cx.waker().clone()),
                    });
                }
                self.registered = true;
            }
            std::task::Poll::Pending
        }
    }
}

/// Helper to inject time provider into async contexts
pub struct TimeContext {
    provider: Box<dyn TimeProvider>,
}

impl std::fmt::Debug for TimeContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TimeContext")
            .field("provider", &"<dyn TimeProvider>")
            .finish()
    }
}

impl TimeContext {
    pub fn new(provider: impl TimeProvider + 'static) -> Self {
        Self {
            provider: Box::new(provider),
        }
    }

    pub fn with_boxed(provider: Box<dyn TimeProvider>) -> Self {
        Self { provider }
    }

    pub fn provider(&self) -> &dyn TimeProvider {
        &*self.provider
    }
}

/// Macro to make time-dependent code testable
#[macro_export]
macro_rules! with_time {
    ($provider:expr_2021, $body:expr_2021) => {{
        let _time_context =
            $crate::infra::testing::time::TimeContext::new($provider);
        $body
    }};
}

#[cfg(test)]
mod tests {
    use chrono::Timelike;

    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn test_virtual_time_advance() {
        let provider = VirtualTimeProvider::new();
        let start = provider.now();

        provider.advance(Duration::from_secs(10));
        let after = provider.now();

        assert_eq!(after - start, Duration::from_secs(10));
    }

    #[tokio::test]
    async fn test_virtual_sleep() {
        let provider = VirtualTimeProvider::new();
        let counter = Arc::new(AtomicUsize::new(0));

        // Spawn a task that sleeps then increments
        let provider_clone = provider.clone();
        let counter_clone = Arc::clone(&counter);
        let handle = tokio::spawn(async move {
            provider_clone.sleep(Duration::from_secs(5)).await;
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        // Give the task time to register its timer
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Counter should still be 0
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        assert_eq!(provider.pending_timers(), 1);

        // Advance time
        provider.advance(Duration::from_secs(5));

        // Give the task time to complete
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Counter should now be 1
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert_eq!(provider.pending_timers(), 0);

        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_advance_to_next_timer() {
        let provider = VirtualTimeProvider::new();

        // Create multiple timers
        let provider1 = provider.clone();
        tokio::spawn(async move {
            provider1.sleep(Duration::from_secs(10)).await;
        });

        let provider2 = provider.clone();
        tokio::spawn(async move {
            provider2.sleep(Duration::from_secs(5)).await;
        });

        let provider3 = provider.clone();
        tokio::spawn(async move {
            provider3.sleep(Duration::from_secs(15)).await;
        });

        // Give tasks time to register timers
        tokio::time::sleep(Duration::from_millis(10)).await;

        assert_eq!(provider.pending_timers(), 3);

        // Advance to first timer (5 seconds)
        let advanced = provider.advance_to_next_timer();
        assert_eq!(advanced, Some(Duration::from_secs(5)));

        // Give task time to complete
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(provider.pending_timers(), 2);

        // Advance to next timer (5 more seconds to reach 10 total)
        let advanced = provider.advance_to_next_timer();
        assert_eq!(advanced, Some(Duration::from_secs(5)));

        // Give task time to complete
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(provider.pending_timers(), 1);
    }

    #[test]
    fn test_set_utc_time() {
        let provider = VirtualTimeProvider::new();

        let new_time = DateTime::parse_from_rfc3339("2024-01-01T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        provider.set_utc(new_time);

        let current = provider.utc_now();
        assert_eq!(current.date_naive(), new_time.date_naive());
        assert_eq!(current.time().hour(), 12);
        assert_eq!(current.time().minute(), 0);
    }

    #[test]
    fn test_system_time_provider() {
        let provider = SystemTimeProvider;

        let instant1 = provider.now();
        let system1 = provider.system_now();
        let utc1 = provider.utc_now();

        std::thread::sleep(Duration::from_millis(10));

        let instant2 = provider.now();
        let system2 = provider.system_now();
        let utc2 = provider.utc_now();

        assert!(instant2 > instant1);
        assert!(system2 > system1);
        assert!(utc2 > utc1);
    }

    #[test]
    fn test_time_provider_as_trait_object() {
        let provider: Box<dyn TimeProvider> =
            Box::new(VirtualTimeProvider::new());
        let _now = provider.now();
        let _cloned = provider.clone_box();
    }
}
