#[cfg(feature = "rkyv")]
use rkyv::{
    Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use ferrex_model::ImageSize;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "rkyv", derive(Archive, RkyvSerialize, RkyvDeserialize))]
pub struct ImageManifestItem {
    pub iid: Uuid,
    pub imz: ImageSize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "rkyv", derive(Archive, RkyvSerialize, RkyvDeserialize))]
pub struct ImageManifestRequest {
    pub requests: Vec<ImageManifestItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "rkyv", derive(Archive, RkyvSerialize, RkyvDeserialize))]
pub struct ImageManifestResponse {
    pub results: Vec<ImageManifestResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "rkyv", derive(Archive, RkyvSerialize, RkyvDeserialize))]
pub struct ImageManifestResult {
    pub iid: Uuid,
    pub imz: ImageSize,
    pub status: ImageManifestStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "rkyv", derive(Archive, RkyvSerialize, RkyvDeserialize))]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub enum ImageManifestStatus {
    Ready { token: String, byte_len: u64 },
    Pending { retry_after_ms: u64 },
    Missing { reason: String },
}
