//! Utility functions for sorting operations
//!
//! This module provides helper functions for efficient sorting operations,
//! including in-place reordering and hash calculations.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Reorder items in-place based on the given indices
///
/// This function efficiently reorders a slice based on a permutation of indices.
/// Each index in `indices` tells us which element from the original slice
/// should be at that position.
pub fn reorder_by_indices<T: Clone>(items: &mut [T], indices: &[usize]) {
    debug_assert_eq!(
        items.len(),
        indices.len(),
        "Indices length must match items length"
    );

    // Create a temporary vector with reordered items
    let mut temp = Vec::with_capacity(items.len());
    for &idx in indices {
        debug_assert!(idx < items.len(), "Index out of bounds");
        temp.push(items[idx].clone());
    }

    // Copy back to original slice
    items.clone_from_slice(&temp);
}

/// Calculate a hash of items for cache invalidation
///
/// This creates a hash that can be used to detect when the items have changed,
/// useful for cache invalidation in CachedSort strategies.
pub fn calculate_items_hash<T: Hash>(items: &[T]) -> u64 {
    let mut hasher = DefaultHasher::new();
    items.len().hash(&mut hasher);
    for item in items {
        item.hash(&mut hasher);
    }
    hasher.finish()
}

/// Check if a slice is already sorted according to a comparison function
///
/// Returns true if the slice is already sorted, false otherwise.
/// This can be used to skip sorting when data is already in the desired order.
pub fn is_sorted_by<T, F>(items: &[T], mut compare: F) -> bool
where
    F: FnMut(&T, &T) -> std::cmp::Ordering,
{
    items.windows(2).all(|w| {
        matches!(
            compare(&w[0], &w[1]),
            std::cmp::Ordering::Less | std::cmp::Ordering::Equal
        )
    })
}

/// Partition items into two groups based on a predicate
///
/// Returns the index where the second group starts.
/// Items for which the predicate returns true come first.
pub fn partition_by<T, F>(items: &mut [T], mut predicate: F) -> usize
where
    T: Clone,
    F: FnMut(&T) -> bool,
{
    let mut partition_point = 0;
    let mut temp = Vec::with_capacity(items.len());

    // First pass: collect items that satisfy predicate
    for item in items.iter() {
        if predicate(item) {
            temp.push(item.clone());
            partition_point += 1;
        }
    }

    // Second pass: collect items that don't satisfy predicate
    for item in items.iter() {
        if !predicate(item) {
            temp.push(item.clone());
        }
    }

    items.clone_from_slice(&temp);
    partition_point
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reorder_by_indices() {
        let mut items = vec!["a", "b", "c", "d"];
        let indices = vec![3, 1, 0, 2]; // d, b, a, c

        reorder_by_indices(&mut items, &indices);

        assert_eq!(items, vec!["d", "b", "a", "c"]);
    }

    #[test]
    fn test_is_sorted_by() {
        let sorted = vec![1, 2, 3, 4, 5];
        let unsorted = vec![1, 3, 2, 4, 5];

        assert!(is_sorted_by(&sorted, |a, b| a.cmp(b)));
        assert!(!is_sorted_by(&unsorted, |a, b| a.cmp(b)));

        // Test reverse sorting
        let reverse_sorted = vec![5, 4, 3, 2, 1];
        assert!(is_sorted_by(&reverse_sorted, |a, b| b.cmp(a)));
    }

    #[test]
    fn test_partition_by() {
        let mut items = vec![1, 2, 3, 4, 5, 6];
        let partition_point = partition_by(&mut items, |&x| x % 2 == 0);

        assert_eq!(partition_point, 3); // Three even numbers
        assert_eq!(&items[..partition_point], &[2, 4, 6]);
        assert_eq!(&items[partition_point..], &[1, 3, 5]);
    }

    #[test]
    fn test_calculate_items_hash() {
        let items1 = vec![1, 2, 3];
        let items2 = vec![1, 2, 3];
        let items3 = vec![1, 2, 4];

        let hash1 = calculate_items_hash(&items1);
        let hash2 = calculate_items_hash(&items2);
        let hash3 = calculate_items_hash(&items3);

        assert_eq!(hash1, hash2, "Same items should produce same hash");
        assert_ne!(
            hash1, hash3,
            "Different items should produce different hash"
        );
    }
}
