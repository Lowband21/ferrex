//! Strategy pattern for composable sorting
//!
//! This module provides the core trait and implementations for sorting strategies
//! that can be composed, cached, and optimized based on dataset characteristics.

use super::{HasField, SortFieldMarker, SortKey, SortableEntity};
use std::marker::PhantomData;

/// A sorting strategy that can be composed
pub trait SortStrategy<T>: Send + Sync {
    /// Apply this sorting strategy to the given items
    fn sort(&self, items: &mut [T]);

    /// Check if this strategy can be applied to the given sample item
    /// Used for adaptive sorting to determine if required data is available
    fn can_apply(&self, sample: &T) -> bool;

    /// Estimate the computational cost of this sorting strategy
    fn cost_estimate(&self) -> SortCost;
}

/// Estimated cost of a sorting operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SortCost {
    /// O(1) - already sorted or no-op
    Trivial,
    /// O(n) - single pass operation
    Cheap,
    /// O(n log n) - standard sorting algorithm
    Moderate,
    /// O(n²) or requires data fetching
    Expensive,
}

/// Single field sort strategy
pub struct FieldSort<T, F>
where
    T: SortableEntity,
    F: SortFieldMarker,
    T::AvailableFields: HasField<F>,
{
    pub field: F,
    pub reverse: bool,
    pub _phantom: PhantomData<T>,
}

impl<T, F> FieldSort<T, F>
where
    T: SortableEntity,
    F: SortFieldMarker,
    T::AvailableFields: HasField<F>,
{
    /// Create a new field sort strategy
    pub fn new(field: F, reverse: bool) -> Self {
        Self {
            field,
            reverse,
            _phantom: PhantomData,
        }
    }
}

impl<T, F> SortStrategy<T> for FieldSort<T, F>
where
    T: SortableEntity + Clone,
    F: SortFieldMarker + Clone,
    T::AvailableFields: HasField<F>,
{
    fn sort(&self, items: &mut [T]) {
        // Extract keys once for efficiency
        let mut keys: Vec<_> = items
            .iter()
            .enumerate()
            .map(|(i, item)| (i, item.extract_key(self.field.clone())))
            .collect();

        // Sort by keys
        keys.sort_by(|a, b| {
            if self.reverse {
                b.1.cmp(&a.1)
            } else {
                a.1.cmp(&b.1)
            }
        });

        // Reorder items based on sorted indices
        let indices: Vec<_> = keys.into_iter().map(|(i, _)| i).collect();
        crate::query::sorting::utils::reorder_by_indices(items, &indices);
    }

    fn can_apply(&self, sample: &T) -> bool {
        // Check if the field's data is available
        // If it requires fetch and we don't have the data, this returns false
        if F::REQUIRES_FETCH {
            // Extract the key and check if it's missing
            let key = sample.extract_key(self.field.clone());
            !key.is_missing()
        } else {
            // Field doesn't require fetch, always available
            true
        }
    }

    fn cost_estimate(&self) -> SortCost {
        if F::REQUIRES_FETCH {
            SortCost::Expensive
        } else {
            SortCost::Moderate
        }
    }
}

/// Multi-field sort with stable sorting
pub struct ChainedSort<T> {
    strategies: Vec<Box<dyn SortStrategy<T>>>,
}

impl<T> ChainedSort<T> {
    /// Create a new chained sort
    pub fn new() -> Self {
        Self {
            strategies: Vec::new(),
        }
    }

    /// Add a sorting strategy to the chain
    pub fn then_by(mut self, strategy: impl SortStrategy<T> + 'static) -> Self {
        self.strategies.push(Box::new(strategy));
        self
    }
}

impl<T: Clone> SortStrategy<T> for ChainedSort<T> {
    fn sort(&self, items: &mut [T]) {
        // Apply strategies in reverse order for stable sorting
        // This ensures primary sort takes precedence
        for strategy in self.strategies.iter().rev() {
            strategy.sort(items);
        }
    }

    fn can_apply(&self, sample: &T) -> bool {
        // Can apply if at least the primary strategy can apply
        self.strategies
            .first()
            .map(|s| s.can_apply(sample))
            .unwrap_or(true)
    }

    fn cost_estimate(&self) -> SortCost {
        // Cost is the sum of all strategies' costs
        self.strategies
            .iter()
            .map(|s| s.cost_estimate())
            .max()
            .unwrap_or(SortCost::Trivial)
    }
}

/// Adaptive sort with fallback
pub struct AdaptiveSort<T> {
    preferred: Box<dyn SortStrategy<T>>,
    fallback: Box<dyn SortStrategy<T>>,
}

impl<T> AdaptiveSort<T> {
    /// Create a new adaptive sort
    pub fn new(
        preferred: impl SortStrategy<T> + 'static,
        fallback: impl SortStrategy<T> + 'static,
    ) -> Self {
        Self {
            preferred: Box::new(preferred),
            fallback: Box::new(fallback),
        }
    }
}

impl<T: Clone> SortStrategy<T> for AdaptiveSort<T> {
    fn sort(&self, items: &mut [T]) {
        // Check if we can use the preferred strategy
        if let Some(sample) = items.first() {
            if self.preferred.can_apply(sample) {
                self.preferred.sort(items);
            } else {
                self.fallback.sort(items);
            }
        }
    }

    fn can_apply(&self, sample: &T) -> bool {
        // Can apply if either strategy works
        self.preferred.can_apply(sample) || self.fallback.can_apply(sample)
    }

    fn cost_estimate(&self) -> SortCost {
        // Return the preferred strategy's cost if it's likely to be used
        // Otherwise return fallback cost
        self.preferred.cost_estimate()
    }
}

/// Const generic optimized sort for compile-time optimization
pub struct ConstFieldSort<T, F, const REVERSE: bool> {
    pub field: F,
    pub _phantom: PhantomData<T>,
}

impl<T, F, const REVERSE: bool> ConstFieldSort<T, F, REVERSE>
where
    T: SortableEntity,
    F: SortFieldMarker,
    T::AvailableFields: HasField<F>,
{
    /// Create a new const field sort
    pub fn new(field: F) -> Self {
        Self {
            field,
            _phantom: PhantomData,
        }
    }
}

impl<T, F, const REVERSE: bool> SortStrategy<T> for ConstFieldSort<T, F, REVERSE>
where
    T: SortableEntity + Clone,
    F: SortFieldMarker + Clone,
    T::AvailableFields: HasField<F>,
{
    fn sort(&self, items: &mut [T]) {
        // Same implementation as FieldSort but with const generic for reverse
        let mut keys: Vec<_> = items
            .iter()
            .enumerate()
            .map(|(i, item)| (i, item.extract_key(self.field.clone())))
            .collect();

        keys.sort_by(|a, b| {
            if REVERSE {
                b.1.cmp(&a.1)
            } else {
                a.1.cmp(&b.1)
            }
        });

        let indices: Vec<_> = keys.into_iter().map(|(i, _)| i).collect();
        crate::query::sorting::utils::reorder_by_indices(items, &indices);
    }

    fn can_apply(&self, sample: &T) -> bool {
        if F::REQUIRES_FETCH {
            let key = sample.extract_key(self.field.clone());
            !key.is_missing()
        } else {
            true
        }
    }

    fn cost_estimate(&self) -> SortCost {
        if F::REQUIRES_FETCH {
            SortCost::Expensive
        } else {
            SortCost::Moderate
        }
    }
}
