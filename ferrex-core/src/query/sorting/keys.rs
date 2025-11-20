//! Sort key types for comparing media items
//!
//! These types wrap the actual values extracted from media items
//! and handle missing data gracefully in their Ord implementations.

use super::traits::SortKey;
use ordered_float::OrderedFloat;
use std::cmp::Ordering;

/// String key for text-based sorting (title, etc.)
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StringKey(Option<String>);

impl StringKey {
    pub fn new(value: Option<String>) -> Self {
        StringKey(value)
    }
}

impl Ord for StringKey {
    fn cmp(&self, other: &Self) -> Ordering {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => a.cmp(b),
            (Some(_), None) => Ordering::Less, // Items with values come first
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }
}

impl PartialOrd for StringKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl SortKey for StringKey {
    fn missing() -> Self {
        StringKey(None)
    }

    fn is_missing(&self) -> bool {
        self.0.is_none()
    }
}

/// Date/time key for temporal sorting
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionalDateKey(Option<chrono::DateTime<chrono::Utc>>);

impl OptionalDateKey {
    pub fn new(value: Option<chrono::DateTime<chrono::Utc>>) -> Self {
        OptionalDateKey(value)
    }
}

impl Ord for OptionalDateKey {
    fn cmp(&self, other: &Self) -> Ordering {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => a.cmp(b),
            (Some(_), None) => Ordering::Less, // Items with dates come first
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }
}

impl PartialOrd for OptionalDateKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl SortKey for OptionalDateKey {
    fn missing() -> Self {
        OptionalDateKey(None)
    }

    fn is_missing(&self) -> bool {
        self.0.is_none()
    }

    fn compare_with_order(&self, other: &Self, reverse: bool) -> Ordering {
        match (self.0.is_none(), other.0.is_none()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                if reverse {
                    other.cmp(self)
                } else {
                    self.cmp(other)
                }
            }
        }
    }
}

/// Float key for numeric sorting (rating, progress, etc.)
#[derive(Clone, Debug, PartialEq)]
pub struct OptionalFloatKey(Option<OrderedFloat<f32>>);

impl OptionalFloatKey {
    pub fn new(value: Option<f32>) -> Self {
        OptionalFloatKey(value.map(OrderedFloat))
    }
}

impl Eq for OptionalFloatKey {}

impl Ord for OptionalFloatKey {
    fn cmp(&self, other: &Self) -> Ordering {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => a.cmp(b),
            (Some(_), None) => Ordering::Less, // Items with values come first
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }
}

impl PartialOrd for OptionalFloatKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl SortKey for OptionalFloatKey {
    fn missing() -> Self {
        OptionalFloatKey(None)
    }

    fn is_missing(&self) -> bool {
        self.0.is_none()
    }

    fn compare_with_order(&self, other: &Self, reverse: bool) -> Ordering {
        match (self.0.is_none(), other.0.is_none()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                if reverse {
                    other.cmp(self)
                } else {
                    self.cmp(other)
                }
            }
        }
    }
}

/// Unsigned integer key for counts and durations
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionalU32Key(Option<u32>);

impl OptionalU32Key {
    pub fn new(value: Option<u32>) -> Self {
        OptionalU32Key(value)
    }
}

impl Ord for OptionalU32Key {
    fn cmp(&self, other: &Self) -> Ordering {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => a.cmp(b),
            (Some(_), None) => Ordering::Less, // Items with values come first
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }
}

impl PartialOrd for OptionalU32Key {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl SortKey for OptionalU32Key {
    fn missing() -> Self {
        OptionalU32Key(None)
    }

    fn is_missing(&self) -> bool {
        self.0.is_none()
    }

    fn compare_with_order(&self, other: &Self, reverse: bool) -> Ordering {
        match (self.0.is_none(), other.0.is_none()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                if reverse {
                    other.cmp(self)
                } else {
                    self.cmp(other)
                }
            }
        }
    }
}

/// Unsigned integer key for large numeric values (file size, bitrate)
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionalU64Key(Option<u64>);

impl OptionalU64Key {
    pub fn new(value: Option<u64>) -> Self {
        OptionalU64Key(value)
    }
}

impl Ord for OptionalU64Key {
    fn cmp(&self, other: &Self) -> Ordering {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => a.cmp(b),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }
}

impl PartialOrd for OptionalU64Key {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl SortKey for OptionalU64Key {
    fn missing() -> Self {
        OptionalU64Key(None)
    }

    fn is_missing(&self) -> bool {
        self.0.is_none()
    }

    fn compare_with_order(&self, other: &Self, reverse: bool) -> Ordering {
        match (self.0.is_none(), other.0.is_none()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                if reverse {
                    other.cmp(self)
                } else {
                    self.cmp(other)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_missing_values_sort_last() {
        // String keys
        let present = StringKey::new(Some("test".to_string()));
        let missing = StringKey::missing();
        assert!(present < missing);

        // Date keys
        let date_present = OptionalDateKey::new(Some(chrono::Utc::now()));
        let date_missing = OptionalDateKey::missing();
        assert!(date_present < date_missing);

        // Float keys
        let float_present = OptionalFloatKey::new(Some(5.0));
        let float_missing = OptionalFloatKey::missing();
        assert!(float_present < float_missing);

        // U32 keys
        let u32_present = OptionalU32Key::new(Some(42));
        let u32_missing = OptionalU32Key::missing();
        assert!(u32_present < u32_missing);

        // U64 keys
        let u64_present = OptionalU64Key::new(Some(42));
        let u64_missing = OptionalU64Key::missing();
        assert!(u64_present < u64_missing);
    }

    #[test]
    fn test_key_ordering() {
        // Test string ordering
        let a = StringKey::new(Some("apple".to_string()));
        let b = StringKey::new(Some("banana".to_string()));
        assert!(a < b);

        // Test float ordering
        let low = OptionalFloatKey::new(Some(1.0));
        let high = OptionalFloatKey::new(Some(10.0));
        assert!(low < high);
    }
}
