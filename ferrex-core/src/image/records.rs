use crate::rkyv_wrappers::{DateTimeWrapper, OptionDateTime, UuidWrapper};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct MediaImageVariantKey {
    pub media_type: String,
    #[rkyv(with = UuidWrapper)]
    pub media_id: Uuid,
    pub image_type: String,
    pub order_index: i32,
    pub variant: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct MediaImageVariantRecord {
    #[rkyv(with = DateTimeWrapper)]
    pub requested_at: chrono::DateTime<chrono::Utc>,
    #[rkyv(with = OptionDateTime)]
    pub cached_at: Option<chrono::DateTime<chrono::Utc>>,
    pub cached: bool,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub content_hash: Option<String>,
    pub theme_color: Option<String>,
    pub key: MediaImageVariantKey,
}
