//! Common scalar and enum conversions.

use chrono::{DateTime, Utc};

use crate::fb::common::{LibraryType as FbLibraryType, Timestamp};

/// Convert a UTC timestamp to the FlatBuffers timestamp struct.
#[inline]
pub fn timestamp_to_fb(dt: &DateTime<Utc>) -> Timestamp {
    Timestamp::new(dt.timestamp_millis())
}

/// Convert an optional UTC timestamp to the FlatBuffers timestamp struct.
/// `None` is encoded as Unix epoch millis `0`.
#[inline]
pub fn option_timestamp_to_fb(dt: Option<&DateTime<Utc>>) -> Timestamp {
    dt.map_or_else(|| Timestamp::new(0), timestamp_to_fb)
}

/// Convert a FlatBuffers timestamp back to UTC.
#[inline]
pub fn fb_to_timestamp(ts: &Timestamp) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(ts.millis())
        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
}

/// Convert a FlatBuffers timestamp back to an optional UTC timestamp.
/// Epoch millis `0` is treated as absent.
#[inline]
pub fn fb_to_option_timestamp(ts: &Timestamp) -> Option<DateTime<Utc>> {
    (ts.millis() != 0).then(|| fb_to_timestamp(ts))
}

/// Convert a Ferrex model library type to its FlatBuffers enum.
#[inline]
pub fn library_type_to_fb(kind: ferrex_model::LibraryType) -> FbLibraryType {
    match kind {
        ferrex_model::LibraryType::Movies => FbLibraryType::Movies,
        ferrex_model::LibraryType::Series => FbLibraryType::Series,
    }
}
