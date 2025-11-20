#[cfg(feature = "rkyv")]
use rkyv::{
    Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize,
};
use serde::{Deserialize, Serialize};

/// Wrapper for image binary data to enable rkyv serialization
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "rkyv", derive(Archive, RkyvSerialize, RkyvDeserialize))]
pub struct ImageData {
    /// The actual image bytes (JPEG/PNG/WebP)
    pub bytes: Vec<u8>,
    /// Content type of the image
    pub content_type: String,
    /// Optional width hint
    pub width: Option<u32>,
    /// Optional height hint
    pub height: Option<u32>,
}
