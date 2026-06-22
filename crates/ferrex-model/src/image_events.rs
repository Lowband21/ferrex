//! Image processing events emitted when server-side image assets become ready.
//!
//! Clients consume these DTOs to refresh image manifests or invalidate local
//! placeholders after asynchronous image preparation completes.

use uuid::Uuid;

use crate::ImageSize;

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct ImageReadyEvent {
    pub iid: Uuid,
    pub imz: ImageSize,
    /// Stable, hex-encoded token for the immutable blob URL.
    pub token: String,
}
