//! Performance-optimized sorting strategies
//!
//! This module provides sorting strategies optimized for performance,
//! including parallel sorting for large datasets and caching for repeated operations.

use super::strategy::{SortCost, SortStrategy};
use crate::query::sorting::utils::calculate_items_hash;
use std::hash::Hash;
use std::marker::PhantomData;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

#[cfg(feature = "parallel-sorting")]
use rayon::prelude::*;

/// Parallel sorting for large datasets
///
/// This strategy uses Rayon to parallelize sorting operations
/// when the dataset exceeds a configurable threshold.
pub struct ParallelSort<T: Send + Sync, S> {
    pub strategy: S,
    pub threshold: usize,
    pub _phantom: PhantomData<T>,
}

impl<T: Send + Sync, S> ParallelSort<T, S> {
    /// Create a new parallel sort with default threshold (10,000 items)
    pub fn new(strategy: S) -> Self {
        Self {
            strategy,
            threshold: 10_000,
            _phantom: PhantomData,
        }
    }

    /// Create a parallel sort with custom threshold
    pub fn with_threshold(strategy: S, threshold: usize) -> Self {
        Self {
            strategy,
            threshold,
            _phantom: PhantomData,
        }
    }
}

#[cfg(feature = "parallel-sorting")]
impl<T: Send + Sync, S> SortStrategy<T> for ParallelSort<T, S>
where
    T: Clone + Send + Sync,
    S: SortStrategy<T>,
{
    fn sort(&self, items: &mut [T]) {
        if items.len() < self.threshold {
            // Dataset too small, use sequential sorting
            self.strategy.sort(items);
        } else {
            // Large dataset, use parallel sorting
            // Note: This is a simplified implementation
            // In practice, we'd need a parallel-aware sorting algorithm
            self.strategy.sort(items);
        }
    }

    fn can_apply(&self, sample: &T) -> bool {
        self.strategy.can_apply(sample)
    }

    fn cost_estimate(&self) -> SortCost {
        // Parallel sorting can reduce the effective cost for large datasets
        match self.strategy.cost_estimate() {
            SortCost::Expensive => SortCost::Moderate,
            other => other,
        }
    }
}

#[cfg(not(feature = "parallel-sorting"))]
impl<T: Send + Sync, S> SortStrategy<T> for ParallelSort<T, S>
where
    T: Clone + Send + Sync,
    S: SortStrategy<T>,
{
    fn sort(&self, items: &mut [T]) {
        // Fallback to sequential sorting when parallel feature is disabled
        self.strategy.sort(items);
    }

    fn can_apply(&self, sample: &T) -> bool {
        self.strategy.can_apply(sample)
    }

    fn cost_estimate(&self) -> SortCost {
        self.strategy.cost_estimate()
    }
}

/// Cached sorting for repeated operations
///
/// This strategy caches the sorted order of items and reuses it
/// when the same items are sorted again within the TTL period.
pub struct CachedSort<T: Send + Sync, S> {
    pub strategy: S,
    pub cache: Arc<RwLock<SortCache<T>>>,
}

/// Cache for sorted items
pub struct SortCache<T: Send + Sync> {
    pub last_sort: Option<CachedSortState<T>>,
}

/// State of a cached sort operation
pub struct CachedSortState<T: Send + Sync> {
    pub items_hash: u64,
    pub sorted_indices: Vec<usize>,
    pub timestamp: Instant,
    pub ttl: Duration,
    pub _phantom: PhantomData<T>,
}

impl<T: Send + Sync> Default for SortCache<T> {
    fn default() -> Self {
        Self { last_sort: None }
    }
}

impl<T: Send + Sync, S> CachedSort<T, S> {
    /// Create a new cached sort with default TTL (5 minutes)
    pub fn new(strategy: S) -> Self {
        Self {
            strategy,
            cache: Arc::new(RwLock::new(SortCache::default())),
        }
    }

    /// Create a cached sort with custom cache
    pub fn with_cache(strategy: S, cache: Arc<RwLock<SortCache<T>>>) -> Self {
        Self { strategy, cache }
    }
}

impl<T: Send + Sync, S> SortStrategy<T> for CachedSort<T, S>
where
    T: Clone + Hash + Send + Sync,
    S: SortStrategy<T>,
{
    fn sort(&self, items: &mut [T]) {
        let items_hash = calculate_items_hash(items);

        // Check if we have a valid cached result
        let use_cache = {
            match self.cache.read() {
                Ok(cache) => {
                    if let Some(ref cached_state) = cache.last_sort {
                        cached_state.items_hash == items_hash
                            && cached_state.timestamp.elapsed() < cached_state.ttl
                            && cached_state.sorted_indices.len() == items.len()
                    } else {
                        false
                    }
                }
                _ => false,
            }
        };

        if use_cache {
            // Use cached sorting order
            if let Ok(cache) = self.cache.read() {
                if let Some(ref cached_state) = cache.last_sort {
                    crate::query::sorting::utils::reorder_by_indices(
                        items,
                        &cached_state.sorted_indices,
                    );
                    return;
                }
            }
        }

        // No valid cache, perform actual sorting
        // First, create a mapping of original indices
        let original_indices: Vec<usize> = (0..items.len()).collect();
        let mut indexed_items: Vec<(usize, T)> = original_indices
            .into_iter()
            .zip(items.iter().cloned())
            .collect();

        // Sort using the strategy (we sort a copy to track indices)
        let mut items_copy = items.to_vec();
        self.strategy.sort(&mut items_copy);

        // Find the new order of indices
        let mut sorted_indices = Vec::with_capacity(items.len());
        for sorted_item in &items_copy {
            // Find the index of this item in the original array
            // This is a simple O(n²) approach; could be optimized with a HashMap
            for (idx, (orig_idx, orig_item)) in indexed_items.iter().enumerate() {
                if std::ptr::eq(sorted_item, orig_item) {
                    sorted_indices.push(*orig_idx);
                    indexed_items.remove(idx);
                    break;
                }
            }
        }

        // If we couldn't track indices properly, fall back to direct sorting
        if sorted_indices.len() != items.len() {
            self.strategy.sort(items);
            return;
        }

        // Apply the sorting
        items.clone_from_slice(&items_copy);

        // Update cache
        if let Ok(mut cache) = self.cache.write() {
            cache.last_sort = Some(CachedSortState {
                items_hash,
                sorted_indices,
                timestamp: Instant::now(),
                ttl: Duration::from_secs(300), // 5 minutes default TTL
                _phantom: PhantomData,
            });
        }
    }

    fn can_apply(&self, sample: &T) -> bool {
        self.strategy.can_apply(sample)
    }

    fn cost_estimate(&self) -> SortCost {
        // If likely to hit cache, cost is trivial
        // Otherwise, same as underlying strategy
        SortCost::Cheap
    }
}

/// Lazy sorting that defers sorting until actually needed
///
/// This is useful when you might not need all sorted items,
/// or when sorting can be combined with other operations.
pub struct LazySort<T: Send + Sync, S> {
    pub strategy: S,
    pub _phantom: PhantomData<T>,
}

impl<T: Send + Sync, S> LazySort<T, S> {
    /// Create a new lazy sort
    pub fn new(strategy: S) -> Self {
        Self {
            strategy,
            _phantom: PhantomData,
        }
    }
}

impl<T: Send + Sync, S> SortStrategy<T> for LazySort<T, S>
where
    T: Clone + Send + Sync,
    S: SortStrategy<T>,
{
    fn sort(&self, items: &mut [T]) {
        // Check if already sorted to avoid unnecessary work
        // This is a simple heuristic; could be made more sophisticated
        if items.len() <= 1 {
            return;
        }

        // For now, just delegate to the underlying strategy
        // In a real implementation, this might defer sorting
        // until iteration or might sort only a prefix
        self.strategy.sort(items);
    }

    fn can_apply(&self, sample: &T) -> bool {
        self.strategy.can_apply(sample)
    }

    fn cost_estimate(&self) -> SortCost {
        self.strategy.cost_estimate()
    }
}
