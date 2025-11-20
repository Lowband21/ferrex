use async_trait::async_trait;
use uuid::Uuid;

use crate::MediaImageKind;
use crate::image::records::{MediaImageVariantKey, MediaImageVariantRecord};
use crate::{
    Result,
    database::traits::{ImageLookupParams, ImageRecord, ImageVariant, MediaImage},
};

/// Repository port for image persistence and media-image associations.
///
/// This port intentionally uses typed `ImageType` where appropriate to avoid
/// stringly-typed boundaries at the application layer. Infrastructure adapters
/// are responsible for mapping to DB representations.
#[async_trait]
pub trait ImageRepository: Send + Sync {
    // Image records
    async fn create_image(&self, tmdb_path: &str) -> Result<ImageRecord>;
    async fn get_image_by_tmdb_path(&self, tmdb_path: &str) -> Result<Option<ImageRecord>>;
    async fn get_image_by_hash(&self, hash: &str) -> Result<Option<ImageRecord>>;
    async fn update_image_metadata(
        &self,
        image_id: Uuid,
        hash: &str,
        size: i32,
        width: i32,
        height: i32,
        format: &str,
    ) -> Result<()>;

    // Variants
    async fn create_image_variant(
        &self,
        image_id: Uuid,
        variant: &str,
        file_path: &str,
        size: i32,
        width: Option<i32>,
        height: Option<i32>,
    ) -> Result<ImageVariant>;
    async fn get_image_variant(
        &self,
        image_id: Uuid,
        variant: &str,
    ) -> Result<Option<ImageVariant>>;
    async fn get_image_variants(&self, image_id: Uuid) -> Result<Vec<ImageVariant>>;

    // Media linking
    async fn link_media_image(
        &self,
        media_type: &str,
        media_id: Uuid,
        image_id: Uuid,
        image_type: MediaImageKind,
        order_index: i32,
        is_primary: bool,
    ) -> Result<()>;
    async fn get_media_images(&self, media_type: &str, media_id: Uuid) -> Result<Vec<MediaImage>>;
    async fn get_media_primary_image(
        &self,
        media_type: &str,
        media_id: Uuid,
        image_type: MediaImageKind,
    ) -> Result<Option<MediaImage>>;

    // Combined lookup for serving
    async fn lookup_image_variant(
        &self,
        params: &ImageLookupParams,
    ) -> Result<Option<(ImageRecord, Option<ImageVariant>)>>;

    // Media image variant cache bookkeeping
    async fn upsert_media_image_variant(
        &self,
        record: &MediaImageVariantRecord,
    ) -> Result<MediaImageVariantRecord>;
    async fn mark_media_image_variant_cached(
        &self,
        key: &MediaImageVariantKey,
        width: Option<i32>,
        height: Option<i32>,
        content_hash: Option<&str>,
        theme_color: Option<&str>,
    ) -> Result<MediaImageVariantRecord>;
    async fn list_media_image_variants(
        &self,
        media_type: &str,
        media_id: Uuid,
    ) -> Result<Vec<MediaImageVariantRecord>>;

    async fn update_media_theme_color(
        &self,
        media_type: &str,
        media_id: Uuid,
        theme_color: Option<&str>,
    ) -> Result<()>;

    // Maintenance
    async fn cleanup_orphaned_images(&self) -> Result<u32>;
    async fn get_image_cache_stats(&self) -> Result<std::collections::HashMap<String, u64>>;
}
