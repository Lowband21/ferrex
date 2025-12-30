use crate::ImageSize;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq, Hash)))]
pub struct ImageQuery {
    /// `tmdb_image_variants.id` (UUID) for the selected image.
    pub iid: Uuid,
    pub imz: ImageSize,
}
