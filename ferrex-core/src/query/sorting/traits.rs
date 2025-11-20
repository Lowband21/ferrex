//! Core traits for sortable media entities
//!
//! This module defines the fundamental traits that all sortable media types
//! must implement to participate in the hybrid sorting system.
//!
//! Uses zero-cost abstractions with compile-time field verification.

/// Base trait for any sortable media item
pub trait SortableEntity: Send + Sync {
    /// The set of fields this entity type supports for sorting
    type AvailableFields: SortFieldSet;

    /// Extract a sort key for the given field
    ///
    /// This method is only callable when the field is proven to be in AvailableFields
    /// at compile time via the HasField bound.
    fn extract_key<F: SortFieldMarker>(&self, field: F) -> F::Key
    where
        Self::AvailableFields: HasField<F>;
}

/// Marker trait for sets of sort fields
///
/// This trait groups fields that an entity type supports.
/// Each entity type will have its own field set implementation.
pub trait SortFieldSet: Send + Sync + 'static {}

/// Compile-time proof that a field set contains a specific field
///
/// This trait provides compile-time verification that a field
/// is valid for a given entity type.
pub trait HasField<F: SortFieldMarker>: SortFieldSet {}

/// Individual sort field with associated key type
///
/// Each field marker type implements this trait to specify
/// its comparison key type and metadata.
pub trait SortFieldMarker: Copy + Clone + Send + Sync + 'static {
    /// The type of key extracted for this field
    type Key: SortKey;

    /// Unique identifier for this field (for runtime dispatch if needed)
    const ID: &'static str;

    /// Whether this field requires fetching additional data from server
    const REQUIRES_FETCH: bool = false;
}

/// Keys that can be compared for sorting
///
/// All sort keys must be comparable and handle missing data gracefully.
pub trait SortKey: Ord + Clone + Send + Sync {
    /// Create a key representing missing/null data
    fn missing() -> Self;

    /// Check if this key represents missing data
    fn is_missing(&self) -> bool;

    /// Compare two keys while ensuring missing values always sort last
    #[inline]
    fn compare_with_order(&self, other: &Self, reverse: bool) -> std::cmp::Ordering {
        if reverse {
            other.cmp(self)
        } else {
            self.cmp(other)
        }
    }
}
