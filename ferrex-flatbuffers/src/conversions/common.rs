//! Common type conversions (timestamps, enums).

use crate::fb::common::{
    LibraryType as FbLibraryType, Timestamp as FbTimestamp,
};
use chrono::{DateTime, Utc};
use ferrex_model::LibraryType;

/// Convert a `chrono::DateTime<Utc>` to FlatBuffers `Timestamp`.
#[inline]
pub fn timestamp_to_fb(dt: &DateTime<Utc>) -> FbTimestamp {
    FbTimestamp::new(dt.timestamp_millis())
}

/// Convert an `Option<DateTime<Utc>>` to FlatBuffers `Timestamp`.
/// Returns epoch 0 for None (callers can check millis == 0).
#[inline]
pub fn option_timestamp_to_fb(dt: Option<&DateTime<Utc>>) -> FbTimestamp {
    match dt {
        Some(dt) => timestamp_to_fb(dt),
        None => FbTimestamp::new(0),
    }
}

/// Convert FlatBuffers `Timestamp` back to `DateTime<Utc>`.
#[inline]
pub fn fb_to_timestamp(ts: &FbTimestamp) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(ts.millis())
        .unwrap_or_else(|| DateTime::UNIX_EPOCH)
}

/// Convert FlatBuffers `Timestamp` to `Option<DateTime<Utc>>`.
/// Returns None for epoch 0.
#[inline]
pub fn fb_to_option_timestamp(ts: &FbTimestamp) -> Option<DateTime<Utc>> {
    if ts.millis() == 0 {
        None
    } else {
        Some(fb_to_timestamp(ts))
    }
}

/// Convert `ferrex_model::LibraryType` to FlatBuffers enum.
#[inline]
pub fn library_type_to_fb(lt: &LibraryType) -> FbLibraryType {
    match lt {
        LibraryType::Movies => FbLibraryType::Movies,
        LibraryType::Series => FbLibraryType::Series,
    }
}
