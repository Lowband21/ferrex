//! Smart fallback system for adaptive sorting
//!
//! This module provides a rule-based fallback system that can intelligently
//! choose between different sorting strategies based on data characteristics.

use super::strategy::{SortCost, SortStrategy};
use std::fmt;
use std::sync::Arc;

/// Smart fallback sort with rule-based strategy selection
///
/// This allows defining multiple strategies with conditions,
/// automatically selecting the best one based on the data.
pub struct SmartFallbackSort<T> {
    pub rules: Vec<FallbackRule<T>>,
}

impl<T> fmt::Debug for SmartFallbackSort<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SmartFallbackSort")
            .field("rule_count", &self.rules.len())
            .finish()
    }
}

/// A rule that determines when to use a particular sorting strategy
pub struct FallbackRule<T> {
    /// Condition that must be met for this rule to apply
    pub condition: Arc<dyn Fn(&T) -> bool + Send + Sync>,
    /// The sorting strategy to use when the condition is met
    pub strategy: Box<dyn SortStrategy<T>>,
    /// Priority of this rule (higher = checked first)
    pub priority: u8,
}

impl<T> fmt::Debug for FallbackRule<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let strategy_type = std::any::type_name_of_val(&*self.strategy);

        f.debug_struct("FallbackRule")
            .field("priority", &self.priority)
            .field("strategy", &strategy_type)
            .finish()
    }
}

impl<T> SmartFallbackSort<T> {
    /// Create a new smart fallback sort
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Add a fallback rule
    pub fn with_rule(
        mut self,
        condition: impl Fn(&T) -> bool + Send + Sync + 'static,
        strategy: impl SortStrategy<T> + 'static,
        priority: u8,
    ) -> Self {
        self.rules.push(FallbackRule {
            condition: Arc::new(condition),
            strategy: Box::new(strategy),
            priority,
        });
        // Keep rules sorted by priority (highest first)
        self.rules.sort_by(|a, b| b.priority.cmp(&a.priority));
        self
    }

    /// Add a default fallback strategy (lowest priority)
    pub fn with_default(mut self, strategy: impl SortStrategy<T> + 'static) -> Self {
        self.rules.push(FallbackRule {
            condition: Arc::new(|_| true), // Always matches
            strategy: Box::new(strategy),
            priority: 0,
        });
        self
    }
}

impl<T: Clone> SortStrategy<T> for SmartFallbackSort<T> {
    fn sort(&self, items: &mut [T]) {
        // Find the first rule that applies
        if let Some(sample) = items.first() {
            for rule in &self.rules {
                if (rule.condition)(sample) && rule.strategy.can_apply(sample) {
                    rule.strategy.sort(items);
                    return;
                }
            }
        }

        // No rules matched, items remain unsorted
        // In production, we might want to log a warning here
    }

    fn can_apply(&self, sample: &T) -> bool {
        // Can apply if any rule matches
        self.rules
            .iter()
            .any(|rule| (rule.condition)(sample) && rule.strategy.can_apply(sample))
    }

    fn cost_estimate(&self) -> SortCost {
        // Return the cost of the highest priority applicable rule
        // This is an estimate since we don't know which rule will actually be used
        self.rules
            .first()
            .map(|rule| rule.strategy.cost_estimate())
            .unwrap_or(SortCost::Trivial)
    }
}

/// Builder for creating smart fallback sorts with common patterns
pub struct FallbackSortBuilder<T> {
    fallback: SmartFallbackSort<T>,
}

impl<T> fmt::Debug for FallbackSortBuilder<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FallbackSortBuilder")
            .field("pending_rules", &self.fallback.rules.len())
            .finish()
    }
}

impl<T> FallbackSortBuilder<T> {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            fallback: SmartFallbackSort::new(),
        }
    }

    /// Add a rule that checks for data availability
    pub fn when_available<F>(
        self,
        check: impl Fn(&T) -> bool + Send + Sync + 'static,
        strategy: impl SortStrategy<T> + 'static,
    ) -> Self {
        Self {
            fallback: self.fallback.with_rule(check, strategy, 100),
        }
    }

    /// Add a rule for partial data
    pub fn when_partial<F>(
        self,
        check: impl Fn(&T) -> bool + Send + Sync + 'static,
        strategy: impl SortStrategy<T> + 'static,
    ) -> Self {
        Self {
            fallback: self.fallback.with_rule(check, strategy, 50),
        }
    }

    /// Set the default fallback strategy
    pub fn otherwise(self, strategy: impl SortStrategy<T> + 'static) -> SmartFallbackSort<T> {
        self.fallback.with_default(strategy)
    }

    /// Build the fallback sort
    pub fn build(self) -> SmartFallbackSort<T> {
        self.fallback
    }
}

/// Specialized fallback for handling missing metadata
///
/// This provides a pre-configured fallback system optimized for
/// the common case where TMDB metadata might be missing.
pub struct MetadataFallbackSort<T> {
    /// Strategy to use when full metadata is available
    pub with_metadata: Box<dyn SortStrategy<T>>,
    /// Strategy to use when metadata is missing
    pub without_metadata: Box<dyn SortStrategy<T>>,
    /// Function to check if metadata is available
    pub has_metadata: Arc<dyn Fn(&T) -> bool + Send + Sync>,
}

impl<T> fmt::Debug for MetadataFallbackSort<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let with_type = std::any::type_name_of_val(&*self.with_metadata);
        let without_type = std::any::type_name_of_val(&*self.without_metadata);

        f.debug_struct("MetadataFallbackSort")
            .field("with_metadata_strategy", &with_type)
            .field("without_metadata_strategy", &without_type)
            .finish()
    }
}

impl<T> MetadataFallbackSort<T> {
    /// Create a new metadata fallback sort
    pub fn new(
        with_metadata: impl SortStrategy<T> + 'static,
        without_metadata: impl SortStrategy<T> + 'static,
        has_metadata: impl Fn(&T) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            with_metadata: Box::new(with_metadata),
            without_metadata: Box::new(without_metadata),
            has_metadata: Arc::new(has_metadata),
        }
    }
}

impl<T: Clone> SortStrategy<T> for MetadataFallbackSort<T> {
    fn sort(&self, items: &mut [T]) {
        if let Some(sample) = items.first() {
            if (self.has_metadata)(sample) {
                self.with_metadata.sort(items);
            } else {
                self.without_metadata.sort(items);
            }
        }
    }

    fn can_apply(&self, sample: &T) -> bool {
        // Can apply with either strategy
        if (self.has_metadata)(sample) {
            self.with_metadata.can_apply(sample)
        } else {
            self.without_metadata.can_apply(sample)
        }
    }

    fn cost_estimate(&self) -> SortCost {
        // Estimate based on likely path
        // Could be made smarter with statistics
        self.with_metadata.cost_estimate()
    }
}
