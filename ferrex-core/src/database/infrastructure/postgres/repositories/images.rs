use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    database::ports::images::ImageRepository,
    database::postgres::PostgresDatabase,
    database::traits::{
        ImageLookupParams, ImageRecord, ImageVariant, MediaDatabaseTrait,
        MediaImage,
    },
    error::Result,
    image::MediaImageKind,
    image::records::{MediaImageVariantKey, MediaImageVariantRecord},
};

#[derive(Clone)]
pub struct PostgresImageRepository {
    db: Arc<PostgresDatabase>,
}

impl PostgresImageRepository {
    pub fn new(db: Arc<PostgresDatabase>) -> Self {
        Self { db }
    }
}

impl fmt::Debug for PostgresImageRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pool = self.db.pool();
        f.debug_struct("PostgresImageRepository")
            .field("pool_size", &pool.size())
            .field("idle_connections", &pool.num_idle())
            .finish()
    }
}

#[async_trait]
impl ImageRepository for PostgresImageRepository {
    async fn create_image(&self, tmdb_path: &str) -> Result<ImageRecord> {
        self.db.create_image(tmdb_path).await
    }

    async fn get_image_by_tmdb_path(
        &self,
        tmdb_path: &str,
    ) -> Result<Option<ImageRecord>> {
        self.db.get_image_by_tmdb_path(tmdb_path).await
    }

    async fn get_image_by_hash(
        &self,
        hash: &str,
    ) -> Result<Option<ImageRecord>> {
        self.db.get_image_by_hash(hash).await
    }

    async fn update_image_metadata(
        &self,
        image_id: Uuid,
        hash: &str,
        size: i32,
        width: i32,
        height: i32,
        format: &str,
    ) -> Result<()> {
        self.db
            .update_image_metadata(image_id, hash, size, width, height, format)
            .await
    }

    async fn create_image_variant(
        &self,
        image_id: Uuid,
        variant: &str,
        file_path: &str,
        size: i32,
        width: Option<i32>,
        height: Option<i32>,
    ) -> Result<ImageVariant> {
        self.db
            .create_image_variant(
                image_id, variant, file_path, size, width, height,
            )
            .await
    }

    async fn get_image_variant(
        &self,
        image_id: Uuid,
        variant: &str,
    ) -> Result<Option<ImageVariant>> {
        self.db.get_image_variant(image_id, variant).await
    }

    async fn get_image_variants(
        &self,
        image_id: Uuid,
    ) -> Result<Vec<ImageVariant>> {
        self.db.get_image_variants(image_id).await
    }

    async fn link_media_image(
        &self,
        media_type: &str,
        media_id: Uuid,
        image_id: Uuid,
        image_type: MediaImageKind,
        order_index: i32,
        is_primary: bool,
    ) -> Result<()> {
        self.db
            .link_media_image(
                media_type,
                media_id,
                image_id,
                image_type,
                order_index,
                is_primary,
            )
            .await
    }

    async fn get_media_images(
        &self,
        media_type: &str,
        media_id: Uuid,
    ) -> Result<Vec<MediaImage>> {
        self.db.get_media_images(media_type, media_id).await
    }

    async fn get_media_primary_image(
        &self,
        media_type: &str,
        media_id: Uuid,
        image_type: MediaImageKind,
    ) -> Result<Option<MediaImage>> {
        self.db
            .get_media_primary_image(media_type, media_id, image_type)
            .await
    }

    async fn lookup_image_variant(
        &self,
        params: &ImageLookupParams,
    ) -> Result<Option<(ImageRecord, Option<ImageVariant>)>> {
        self.db.lookup_image_variant(params).await
    }

    async fn upsert_media_image_variant(
        &self,
        record: &MediaImageVariantRecord,
    ) -> Result<MediaImageVariantRecord> {
        self.db.upsert_media_image_variant(record).await
    }

    async fn mark_media_image_variant_cached(
        &self,
        key: &MediaImageVariantKey,
        width: Option<i32>,
        height: Option<i32>,
        content_hash: Option<&str>,
        theme_color: Option<&str>,
    ) -> Result<MediaImageVariantRecord> {
        self.db
            .mark_media_image_variant_cached(
                key,
                width,
                height,
                content_hash,
                theme_color,
            )
            .await
    }

    async fn list_media_image_variants(
        &self,
        media_type: &str,
        media_id: Uuid,
    ) -> Result<Vec<MediaImageVariantRecord>> {
        self.db
            .list_media_image_variants(media_type, media_id)
            .await
    }

    async fn update_media_theme_color(
        &self,
        media_type: &str,
        media_id: Uuid,
        theme_color: Option<&str>,
    ) -> Result<()> {
        self.db
            .update_media_theme_color(media_type, media_id, theme_color)
            .await
    }

    async fn cleanup_orphaned_images(&self) -> Result<u32> {
        self.db.cleanup_orphaned_images().await
    }

    async fn get_image_cache_stats(&self) -> Result<HashMap<String, u64>> {
        self.db.get_image_cache_stats().await
    }
}
