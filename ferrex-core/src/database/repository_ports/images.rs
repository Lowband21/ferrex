use async_trait::async_trait;
use ferrex_model::{ImageMediaType, ImageSize, image::ImageDimensions};
use uuid::Uuid;

use crate::{
    database::traits::{ImageRecord, OriginalImage},
    error::Result,
};

#[derive(Debug)]
pub struct VarInput<'a> {
    pub media_id: Uuid,
    pub media_type: ImageMediaType,
    pub tmdb_path: &'a str,
    /// Logical image size (kind/variant); no longer used as the source of DB dimensions.
    pub imz: ImageSize,
    /// Explicit original width from TMDB for this image variant.
    pub width: i16,
    /// Explicit original height from TMDB for this image variant.
    pub height: i16,
    pub lang: &'a str,
    pub v_avg: f32,
    pub v_cnt: u32,
    pub is_primary: bool,
}

#[derive(Debug)]
pub struct ImgInput<'a> {
    pub iid: Uuid,
    pub media_id: Option<Uuid>,
    pub media_type: Option<ImageMediaType>,
    pub tmdb_path: Option<&'a str>,
    pub imz: ImageSize,
    /// Authoritative pixel dimensions decoded from the image bytes.
    ///
    /// When present, this is used as the source of truth for persistence
    /// (and avoids relying on aspect-ratio-derived heights).
    pub decoded_dimensions: Option<ImageDimensions>,
    pub theme_color: Option<&'a str>,
    pub cache_key: &'a str,
    pub integrity: &'a str,
    pub byte_len: i32,
}

#[derive(Debug)]
pub struct ImgDbLookup<'a> {
    pub imz: ImageSize,
    pub iid: Option<Uuid>,
    pub media_id: Option<Uuid>,
    pub media_type: Option<ImageMediaType>,
    pub tmdb_path: Option<&'a str>,
    pub lang: Option<&'a str>,
}

/// Repository port for image persistence and media-image associations.
///
/// This port intentionally uses typed `ImageType` where appropriate to avoid
/// stringly-typed boundaries at the application layer. Infrastructure adapters
/// are responsible for mapping to DB representations.
#[async_trait]
pub trait ImageRepository: Send + Sync {
    // Maintenance
    async fn cleanup_orphaned_images(&self) -> Result<u32>;

    // Variant lookups
    /// Unified image variant query from struct of optionals
    async fn lookup_original_image<'a>(
        &self,
        ctx: &'a ImgDbLookup,
    ) -> Result<Option<OriginalImage>>;

    /// List all variants for a media/size combination (player-authoritative)
    async fn lookup_variants_for_media(
        &self,
        media_id: Uuid,
        media_type: ImageMediaType,
        imz: ImageSize,
    ) -> Result<Vec<OriginalImage>>;

    /// Get from image id; preferred method
    async fn lookup_variant_by_iid(
        &self,
        iid: Uuid,
    ) -> Result<Option<OriginalImage>>;

    /// Get from tmdb url path string
    async fn lookup_variant_by_path(
        &self,
        tmdb_path: &str,
    ) -> Result<Option<OriginalImage>>;

    /// Look up a cached image record by TMDB variant id and requested logical size.
    ///
    /// This is the canonical lookup used by the iid-first image provider.
    async fn lookup_cached_image(
        &self,
        iid: Uuid,
        imz: ImageSize,
    ) -> Result<Option<ImageRecord>>;

    async fn lookup_original_cached_image(
        &self,
        iid: Uuid,
    ) -> Result<Option<ImageRecord>>;

    async fn lookup_resized_cached_image(
        &self,
        iid: Uuid,
        width: i16,
    ) -> Result<Option<ImageRecord>>;

    // Cached image lookups
    /// Unified image record query from struct of optionals
    async fn lookup_images<'a>(
        &self,
        ctx: &'a [ImgDbLookup],
    ) -> Result<Vec<ImageRecord>>;

    // Variants
    async fn upsert_image<'a>(&self, ctx: &'a ImgInput) -> Result<ImageRecord>;

    async fn upsert_variant<'a>(
        &self,
        ctx: &'a VarInput,
    ) -> Result<OriginalImage>;

    async fn upsert_variants<'a>(
        &self,
        variants: &'a [VarInput],
    ) -> Result<Vec<OriginalImage>>;
}
