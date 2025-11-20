use crate::domain::media::image::MediaImageKind;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct MediaImageVariantKey {
    pub media_type: String,
    pub media_id: Uuid,
    pub image_type: MediaImageKind,
    pub order_index: i32,
    pub variant: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct MediaImageVariantRecord {
    #[cfg_attr(
        feature = "rkyv",
        rkyv(with = crate::rkyv_wrappers::DateTimeWrapper)
    )]
    pub requested_at: chrono::DateTime<chrono::Utc>,
    #[cfg_attr(
        feature = "rkyv",
        rkyv(with = crate::rkyv_wrappers::OptionDateTime)
    )]
    pub cached_at: Option<chrono::DateTime<chrono::Utc>>,
    pub cached: bool,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub content_hash: Option<String>,
    pub theme_color: Option<String>,
    pub key: MediaImageVariantKey,
}
