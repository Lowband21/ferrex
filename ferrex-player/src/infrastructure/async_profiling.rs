//! Async-safe profiling utilities for background threads and async functions
//!
//! This module provides utilities for profiling async code and background threads
//! where the standard profiling::scope! macro doesn't work well due to await points.
//!
//! The key insight is that profiling::scope! uses RAII which doesn't survive across
//! await points. Instead, we profile each poll() of the Future separately.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

// =============================================================================
// Async Profiling Wrapper
// =============================================================================

/// A Future wrapper that adds profiling to each poll() call
///
/// Since profiling::scope! uses RAII and doesn't survive across await points,
/// we instead create a new scope for each poll() of the future.
pub struct ProfiledFuture<F> {
    future: F,
    scope_name: &'static str,
}

impl<F> ProfiledFuture<F> {
    pub fn new(future: F, scope_name: &'static str) -> Self {
        Self { future, scope_name }
    }
}

impl<F> Future for ProfiledFuture<F>
where
    F: Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Get the scope name before creating the profiling scope
        let scope_name = self.scope_name;

        // Create a profiling scope for this poll
        // This will be automatically closed when we return from poll()
        profiling::scope!(scope_name);

        // Get the inner future
        let this = unsafe { self.get_unchecked_mut() };
        let future = unsafe { Pin::new_unchecked(&mut this.future) };

        // Poll the inner future
        future.poll(cx)
    }
}

// =============================================================================
// Async Profiling Functions
// =============================================================================

/// Profile an async function with a given scope name
///
/// # Example
/// ```rust
/// use ferrex_player::infrastructure::async_profiling::profile_async;
///
/// async fn fetch_data() -> Result<Data, Error> {
///     // Your async code here
/// }
///
/// let result = profile_async("API::FetchData", fetch_data()).await;
/// ```
pub async fn profile_async<F, T>(scope_name: &'static str, future: F) -> T
where
    F: Future<Output = T>,
{
    ProfiledFuture::new(future, scope_name).await
}

/// Profile an async block with manual scope management
///
/// # Example
/// ```rust
/// use ferrex_player::infrastructure::async_profiling::profile_async_block;
///
/// profile_async_block!("Service::ProcessBatch", async {
///     // Your async code here
///     process_batch().await
/// })
/// ```
#[macro_export]
macro_rules! profile_async_block {
    ($scope_name:expr_2021, $block:expr_2021) => {{ $crate::infrastructure::async_profiling::profile_async($scope_name, $block).await }};
}

// =============================================================================
// Thread Registration Helpers
// =============================================================================

/// Register a thread with the profiler
///
/// This should be called at the start of any background thread or tokio task
/// that you want to profile.
pub fn register_profiler_thread(name: impl AsRef<str>) {
    // The profiling crate handles the actual registration
    profiling::register_thread!(name.as_ref());
}

/// Mark a frame boundary for background threads
///
/// Background threads that run in loops should call this to mark frame boundaries,
/// allowing the profiler to properly segment the work.
pub fn mark_frame_boundary() {
    profiling::finish_frame!();
}

// =============================================================================
// Async Service Profiling
// =============================================================================

/// A trait for profiling async services
pub trait ProfiledService {
    /// Register this service's thread with the profiler
    fn register_thread(&self, name: impl AsRef<str>) {
        register_profiler_thread(name);
    }

    /// Mark a frame boundary (for services that process in batches)
    fn mark_frame(&self) {
        mark_frame_boundary();
    }
}

// =============================================================================
// Tokio Spawn Wrapper
// =============================================================================

/// Spawn a tokio task with automatic thread registration
///
/// # Example
/// ```rust
/// use ferrex_player::infrastructure::async_profiling::spawn_profiled;
///
/// spawn_profiled("BackgroundWorker", async move {
///     // Your async task code
///     loop {
///         process_item().await;
///         mark_frame_boundary();
///     }
/// });
/// ```
pub fn spawn_profiled<F>(
    name: impl Into<String> + Send + 'static,
    future: F,
) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let thread_name = name.into();
    tokio::spawn(async move {
        register_profiler_thread(&thread_name);
        future.await
    })
}

// =============================================================================
// Specialized Profiling Helpers
// =============================================================================

/// Profile an async API call
pub async fn profile_api_call<F, T>(endpoint: &str, future: F) -> T
where
    F: Future<Output = T>,
{
    let scope_name = Box::leak(format!("API::{}", endpoint).into_boxed_str());
    profile_async(scope_name, future).await
}

/// Profile a database query
pub async fn profile_db_query<F, T>(query_name: &str, future: F) -> T
where
    F: Future<Output = T>,
{
    let scope_name = Box::leak(format!("DB::{}", query_name).into_boxed_str());
    profile_async(scope_name, future).await
}

/// Profile a service method
pub async fn profile_service<F, T>(service: &str, method: &str, future: F) -> T
where
    F: Future<Output = T>,
{
    let scope_name = Box::leak(format!("Service::{}::{}", service, method).into_boxed_str());
    profile_async(scope_name, future).await
}

// =============================================================================
// Batch Processing Profiling
// =============================================================================

/// Profile batch processing with per-item and per-batch metrics
pub struct BatchProfiler {
    batch_name: String,
    items_processed: usize,
    batch_start: std::time::Instant,
}

impl BatchProfiler {
    pub fn new(batch_name: impl Into<String>) -> Self {
        let name = batch_name.into();
        log::debug!("Starting batch: {}", name);

        Self {
            batch_name: name,
            items_processed: 0,
            batch_start: std::time::Instant::now(),
        }
    }

    pub fn item_processed(&mut self) {
        self.items_processed += 1;

        // Mark frame boundary every N items for better granularity
        if self.items_processed % 10 == 0 {
            mark_frame_boundary();
        }
    }

    pub fn finish(self) {
        let duration = self.batch_start.elapsed();
        log::info!(
            "Batch '{}' completed: {} items in {:?} ({:.2} items/sec)",
            self.batch_name,
            self.items_processed,
            duration,
            self.items_processed as f64 / duration.as_secs_f64()
        );
        mark_frame_boundary();
    }
}

// =============================================================================
// Manual Async Scope Management
// =============================================================================

/// For cases where you need to manually manage profiling in an async context,
/// this provides a way to create scopes that can be manually started/stopped.
///
/// NOTE: This is generally not recommended. Use `profile_async` instead.
pub struct ManualAsyncScope {
    name: String,
}

impl ManualAsyncScope {
    /// Start a new manual scope
    pub fn start(name: impl Into<String>) -> Self {
        let name = name.into();
        // We can't actually "start" a scope that persists across await points
        // Each time we want to profile, we need to create a new scope
        Self { name }
    }

    /// Profile a section of code with this scope's name
    pub fn profile<T, F: FnOnce() -> T>(&self, f: F) -> T {
        // We can't use profiling::scope! with a dynamic string
        // So we just run the function without profiling in this case
        // For profiling, use profile_async instead which can handle dynamic names
        f()
    }

    /// Profile an async section
    pub async fn profile_async<F, T>(&self, future: F) -> T
    where
        F: Future<Output = T>,
    {
        // We need to leak the string to get a 'static reference
        let scope_name = Box::leak(self.name.clone().into_boxed_str());
        profile_async(scope_name, future).await
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_async_profiling() {
        async fn slow_operation() -> u32 {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            42
        }

        let result = profile_async("test::slow_operation", slow_operation()).await;
        assert_eq!(result, 42);
    }

    #[tokio::test]
    async fn test_spawn_profiled() {
        let handle = spawn_profiled("test_worker", async {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            123
        });

        let result = handle.await.unwrap();
        assert_eq!(result, 123);
    }

    #[test]
    fn test_batch_profiler() {
        let mut profiler = BatchProfiler::new("test_batch");
        for _ in 0..5 {
            profiler.item_processed();
        }
        profiler.finish();
    }

    #[tokio::test]
    async fn test_manual_scope() {
        let scope = ManualAsyncScope::start("test_manual");

        // Test sync profiling
        let sync_result = scope.profile(|| {
            // Some sync work
            5 + 5
        });
        assert_eq!(sync_result, 10);

        // Test async profiling
        let async_result = scope
            .profile_async(async {
                tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
                20
            })
            .await;
        assert_eq!(async_result, 20);
    }
}
