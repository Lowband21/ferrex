use sqlx::types::chrono::Utc;
use std::collections::HashMap;
use tracing::{debug, info, warn};

use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    database::{
        ports::images::ImageRepository,
        traits::{ImageLookupParams, ImageRecord, ImageVariant, MediaImage},
    },
    domain::media::image::{
        MediaImageKind,
        records::{MediaImageVariantKey, MediaImageVariantRecord},
    },
    error::{MediaError, Result},
};

#[derive(Clone, Debug)]
pub struct PostgresImageRepository {
    pool: PgPool,
}

impl PostgresImageRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ImageRepository for PostgresImageRepository {
    async fn create_image(&self, tmdb_path: &str) -> Result<ImageRecord> {
        let id = Uuid::now_v7();
        let now = chrono::Utc::now();

        let row = sqlx::query!(
            r#"
            INSERT INTO images (id, tmdb_path, created_at, updated_at)
            VALUES ($1, $2, $3, $3)
            ON CONFLICT (tmdb_path) DO UPDATE SET
                updated_at = EXCLUDED.updated_at
            RETURNING id, tmdb_path, file_hash, file_size, width, height, format, created_at
            "#,
            id,
            tmdb_path,
            now
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e: sqlx::Error| {
            MediaError::Internal(format!("Failed to create image: {}", e))
        })?;

        Ok(ImageRecord {
            id: row.id,
            tmdb_path: row.tmdb_path,
            file_hash: row.file_hash,
            file_size: row.file_size,
            width: row.width,
            height: row.height,
            format: row.format,
            created_at: row.created_at,
        })
    }

    async fn get_image_by_tmdb_path(
        &self,
        tmdb_path: &str,
    ) -> Result<Option<ImageRecord>> {
        let row = sqlx::query!(
            r#"
            SELECT id, tmdb_path, file_hash, file_size, width, height, format, created_at
            FROM images
            WHERE tmdb_path = $1
            "#,
            tmdb_path
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get image: {}", e)))?;

        Ok(row.map(|r| ImageRecord {
            id: r.id,
            tmdb_path: r.tmdb_path,
            file_hash: r.file_hash,
            file_size: r.file_size,
            width: r.width,
            height: r.height,
            format: r.format,
            created_at: r.created_at,
        }))
    }

    async fn get_image_by_hash(
        &self,
        hash: &str,
    ) -> Result<Option<ImageRecord>> {
        let row = sqlx::query!(
            r#"
            SELECT id, tmdb_path, file_hash, file_size, width, height, format, created_at
            FROM images
            WHERE file_hash = $1
            "#,
            hash
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get image by hash: {}", e)))?;

        Ok(row.map(|r| ImageRecord {
            id: r.id,
            tmdb_path: r.tmdb_path,
            file_hash: r.file_hash,
            file_size: r.file_size,
            width: r.width,
            height: r.height,
            format: r.format,
            created_at: r.created_at,
        }))
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
        sqlx::query!(
            r#"
            UPDATE images
            SET file_hash = $2, file_size = $3, width = $4, height = $5, format = $6, updated_at = NOW()
            WHERE id = $1
            "#,
            image_id,
            hash,
            size,
            width,
            height,
            format
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to update image metadata: {}", e)))?;

        Ok(())
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
        let id = Uuid::now_v7();
        let now = chrono::Utc::now();

        let row = sqlx::query!(
            r#"
            INSERT INTO image_variants (
                id,
                image_id,
                variant,
                file_path,
                file_size,
                width,
                height,
                created_at,
                downloaded_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())
            ON CONFLICT (image_id, variant) DO UPDATE SET
                file_path = EXCLUDED.file_path,
                file_size = EXCLUDED.file_size,
                width = EXCLUDED.width,
                height = EXCLUDED.height,
                downloaded_at = NOW()
            RETURNING id, image_id, variant, file_path, file_size, width, height, created_at, downloaded_at
            "#,
            id,
            image_id,
            variant,
            file_path,
            size,
            width,
            height,
            now
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to create image variant: {}", e)))?;

        Ok(ImageVariant {
            id: row.id,
            image_id: row.image_id,
            variant: row.variant,
            file_path: row.file_path,
            file_size: row.file_size,
            width: row.width,
            height: row.height,
            created_at: row.created_at,
            downloaded_at: row.downloaded_at,
        })
    }

    async fn get_image_variant(
        &self,
        image_id: Uuid,
        variant: &str,
    ) -> Result<Option<ImageVariant>> {
        let row = sqlx::query!(
            r#"
            SELECT
                id,
                image_id,
                variant,
                file_path,
                file_size,
                width,
                height,
                created_at,
                downloaded_at
            FROM image_variants
            WHERE image_id = $1 AND variant = $2
            "#,
            image_id,
            variant
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to get image variant: {}", e))
        })?;

        Ok(row.map(|r| ImageVariant {
            id: r.id,
            image_id: r.image_id,
            variant: r.variant,
            file_path: r.file_path,
            file_size: r.file_size,
            width: r.width,
            height: r.height,
            created_at: r.created_at,
            downloaded_at: r.downloaded_at,
        }))
    }

    async fn get_image_variants(
        &self,
        image_id: Uuid,
    ) -> Result<Vec<ImageVariant>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                id,
                image_id,
                variant,
                file_path,
                file_size,
                width,
                height,
                created_at,
                downloaded_at
            FROM image_variants
            WHERE image_id = $1
            ORDER BY variant
            "#,
            image_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to get image variants: {}", e))
        })?;

        Ok(rows
            .into_iter()
            .map(|r| ImageVariant {
                id: r.id,
                image_id: r.image_id,
                variant: r.variant,
                file_path: r.file_path,
                file_size: r.file_size,
                width: r.width,
                height: r.height,
                created_at: r.created_at,
                downloaded_at: r.downloaded_at,
            })
            .collect())
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
        info!(
            "link_media_image: type={}, media_id={}, image_id={}, image_type={}, index={}",
            media_type, media_id, image_id, image_type, order_index
        );

        sqlx::query!(
            r#"
            INSERT INTO media_images (media_type, media_id, image_id, image_type, order_index, is_primary)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (media_type, media_id, image_type, order_index) DO UPDATE SET
                image_id = EXCLUDED.image_id,
                is_primary = EXCLUDED.is_primary
            "#,
            media_type,
            media_id,
            image_id,
            image_type.as_str(),
            order_index,
            is_primary
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to link media image: {}", e)))?;

        Ok(())
    }

    async fn get_media_images(
        &self,
        media_type: &str,
        media_id: Uuid,
    ) -> Result<Vec<MediaImage>> {
        let rows = sqlx::query!(
            r#"
            SELECT media_type, media_id, image_id, image_type, order_index, is_primary
            FROM media_images
            WHERE media_type = $1 AND media_id = $2
            ORDER BY image_type, order_index
            "#,
            media_type,
            media_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get media images: {}", e)))?;

        Ok(rows
            .into_iter()
            .map(|r| MediaImage {
                media_type: r.media_type,
                media_id: r.media_id,
                image_id: r.image_id,
                image_type: MediaImageKind::parse(&r.image_type),
                order_index: r.order_index,
                is_primary: r.is_primary,
            })
            .collect())
    }

    async fn get_media_primary_image(
        &self,
        media_type: &str,
        media_id: Uuid,
        image_type: MediaImageKind,
    ) -> Result<Option<MediaImage>> {
        let row = sqlx::query!(
            r#"
            SELECT media_type, media_id, image_id, image_type, order_index, is_primary
            FROM media_images
            WHERE media_type = $1 AND media_id = $2 AND image_type = $3 AND is_primary = true
            LIMIT 1
            "#,
            media_type,
            media_id,
            image_type.as_str()
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get primary image: {}", e)))?;

        Ok(row.map(|r| MediaImage {
            media_type: r.media_type,
            media_id: r.media_id,
            image_id: r.image_id,
            image_type: MediaImageKind::parse(&r.image_type),
            order_index: r.order_index,
            is_primary: r.is_primary,
        }))
    }

    async fn lookup_image_variant(
        &self,
        params: &ImageLookupParams,
    ) -> Result<Option<(ImageRecord, Option<ImageVariant>)>> {
        debug!(
            "lookup_image_variant: type={}, id='{}', image_type={}, index={}",
            params.media_type, params.media_id, params.image_type, params.index
        );

        // Parse media_id to UUID
        let media_id = match Uuid::parse_str(&params.media_id) {
            Ok(uuid) => uuid,
            Err(e) => {
                warn!(
                    "Failed to parse media_id '{}' as UUID: {}",
                    params.media_id, e
                );
                return Err(MediaError::InvalidMedia(format!(
                    "Invalid media ID '{}': {}",
                    params.media_id, e
                )));
            }
        };

        // First find the media image link
        debug!(
            "Querying media_images table: type={}, media_id={}, image_type={}, index={}",
            &params.media_type, media_id, &params.image_type, params.index
        );

        let media_image = sqlx::query!(
            r#"
            SELECT image_id
            FROM media_images
            WHERE media_type = $1 AND media_id = $2 AND image_type = $3 AND order_index = $4
            "#,
            &params.media_type,
            media_id,
            params.image_type.as_str(),
            params.index as i32
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to lookup media image: {}", e)))?;

        if let Some(media_image) = media_image {
            // Get the image record
            let image = sqlx::query!(
                r#"
                SELECT id, tmdb_path, file_hash, file_size, width, height, format, created_at
                FROM images
                WHERE id = $1
                "#,
                media_image.image_id
            )
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to get image: {}", e)))?;

            if let Some(image_row) = image {
                let image_record = ImageRecord {
                    id: image_row.id,
                    tmdb_path: image_row.tmdb_path,
                    file_hash: image_row.file_hash,
                    file_size: image_row.file_size,
                    width: image_row.width,
                    height: image_row.height,
                    format: image_row.format,
                    created_at: image_row.created_at,
                };

                // Get the variant if requested
                let variant = if let Some(variant_name) = &params.variant {
                    self.get_image_variant(image_row.id, variant_name).await?
                } else {
                    None
                };

                return Ok(Some((image_record, variant)));
            }
        }

        Ok(None)
    }

    async fn upsert_media_image_variant(
        &self,
        record: &MediaImageVariantRecord,
    ) -> Result<MediaImageVariantRecord> {
        let key = &record.key;
        let row = sqlx::query!(
            r#"
            INSERT INTO media_image_variants (
                media_type,
                media_id,
                image_type,
                order_index,
                variant,
                cached,
                width,
                height,
                content_hash,
                theme_color,
                requested_at,
                cached_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT (media_type, media_id, image_type, order_index, variant) DO UPDATE SET
                cached = EXCLUDED.cached,
                width = EXCLUDED.width,
                height = EXCLUDED.height,
                content_hash = EXCLUDED.content_hash,
                theme_color = EXCLUDED.theme_color,
                requested_at = LEAST(media_image_variants.requested_at, EXCLUDED.requested_at),
                cached_at = COALESCE(EXCLUDED.cached_at, media_image_variants.cached_at)
            RETURNING
                media_type,
                media_id,
                image_type,
                order_index,
                variant,
                cached,
                width,
                height,
                content_hash,
                theme_color,
                requested_at,
                cached_at
            "#,
            key.media_type,
            key.media_id,
            key.image_type.as_str(),
            key.order_index,
            key.variant,
            record.cached,
            record.width,
            record.height,
            record.content_hash.as_deref(),
            record.theme_color.as_deref(),
            record.requested_at,
            record.cached_at
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to upsert media image variant: {}", e))
        })?;

        Ok(MediaImageVariantRecord {
            requested_at: row.requested_at,
            cached_at: row.cached_at,
            cached: row.cached,
            width: row.width,
            height: row.height,
            content_hash: row.content_hash,
            theme_color: row.theme_color,
            key: MediaImageVariantKey {
                media_type: row.media_type,
                media_id: row.media_id,
                image_type: MediaImageKind::parse(&row.image_type),
                order_index: row.order_index,
                variant: row.variant,
            },
        })
    }

    async fn mark_media_image_variant_cached(
        &self,
        key: &MediaImageVariantKey,
        width: Option<i32>,
        height: Option<i32>,
        content_hash: Option<&str>,
        theme_color: Option<&str>,
    ) -> Result<MediaImageVariantRecord> {
        let row = sqlx::query!(
            r#"
            UPDATE media_image_variants
            SET
                cached = true,
                width = COALESCE($5, width),
                height = COALESCE($6, height),
                content_hash = COALESCE($7, content_hash),
                theme_color = COALESCE($8, theme_color),
                cached_at = NOW()
            WHERE media_type = $1
              AND media_id = $2
              AND image_type = $3
              AND order_index = $4
              AND variant = $9
            RETURNING
                media_type,
                media_id,
                image_type,
                order_index,
                variant,
                cached,
                width,
                height,
                content_hash,
                theme_color,
                requested_at,
                cached_at
            "#,
            key.media_type,
            key.media_id,
            key.image_type.as_str(),
            key.order_index,
            width,
            height,
            content_hash,
            theme_color,
            key.variant
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to mark media image variant cached: {}",
                e
            ))
        })?;

        if let Some(row) = row {
            return Ok(MediaImageVariantRecord {
                requested_at: row.requested_at,
                cached_at: row.cached_at,
                cached: row.cached,
                width: row.width,
                height: row.height,
                content_hash: row.content_hash,
                theme_color: row.theme_color,
                key: MediaImageVariantKey {
                    media_type: row.media_type,
                    media_id: row.media_id,
                    image_type: MediaImageKind::parse(&row.image_type),
                    order_index: row.order_index,
                    variant: row.variant,
                },
            });
        }

        let record = MediaImageVariantRecord {
            requested_at: Utc::now(),
            cached_at: Some(Utc::now()),
            cached: true,
            width,
            height,
            content_hash: content_hash.map(|s| s.to_string()),
            theme_color: theme_color.map(|s| s.to_string()),
            key: key.clone(),
        };

        self.upsert_media_image_variant(&record).await
    }

    async fn list_media_image_variants(
        &self,
        media_type: &str,
        media_id: Uuid,
    ) -> Result<Vec<MediaImageVariantRecord>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                media_type,
                media_id,
                image_type,
                order_index,
                variant,
                cached,
                width,
                height,
                content_hash,
                theme_color,
                requested_at,
                cached_at
            FROM media_image_variants
            WHERE media_type = $1 AND media_id = $2
            ORDER BY image_type, order_index, variant
            "#,
            media_type,
            media_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to list media image variants: {}",
                e
            ))
        })?;

        Ok(rows
            .into_iter()
            .map(|row| MediaImageVariantRecord {
                requested_at: row.requested_at,
                cached_at: row.cached_at,
                cached: row.cached,
                width: row.width,
                height: row.height,
                content_hash: row.content_hash,
                theme_color: row.theme_color,
                key: MediaImageVariantKey {
                    media_type: row.media_type,
                    media_id: row.media_id,
                    image_type: MediaImageKind::parse(&row.image_type),
                    order_index: row.order_index,
                    variant: row.variant,
                },
            })
            .collect())
    }

    async fn update_media_theme_color(
        &self,
        media_type: &str,
        media_id: Uuid,
        theme_color: Option<&str>,
    ) -> Result<()> {
        match media_type {
            "movie" => {
                sqlx::query!(
                "UPDATE movie_references SET theme_color = $2 WHERE id = $1",
                media_id,
                theme_color
            )
            .execute(&self.pool)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to update movie theme color: {}",
                    e
                ))
            })?;
            }
            "series" => {
                sqlx::query!(
                "UPDATE series_references SET theme_color = $2 WHERE id = $1",
                media_id,
                theme_color
            )
            .execute(&self.pool)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to update series theme color: {}",
                    e
                ))
            })?;
            }
            "season" => {
                sqlx::query!(
                "UPDATE season_references SET theme_color = $2 WHERE id = $1",
                media_id,
                theme_color
            )
            .execute(&self.pool)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to update season theme color: {}",
                    e
                ))
            })?;
            }
            _ => {}
        }

        Ok(())
    }

    async fn is_media_image_variant_cached(
        &self,
        key: &MediaImageVariantKey,
    ) -> Result<bool> {
        let row = sqlx::query!(
            r#"
            SELECT cached
            FROM media_image_variants
            WHERE media_type = $1
              AND media_id = $2
              AND image_type = $3
              AND order_index = $4
              AND variant = $5
            "#,
            key.media_type,
            key.media_id,
            key.image_type.as_str(),
            key.order_index,
            key.variant
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to check media image variant cached status: {}",
                e
            ))
        })?;

        Ok(row.map(|r| r.cached).unwrap_or(false))
    }

    async fn invalidate_media_image_variant(
        &self,
        key: &MediaImageVariantKey,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE media_image_variants
            SET cached = false, cached_at = NULL
            WHERE media_type = $1
              AND media_id = $2
              AND image_type = $3
              AND order_index = $4
              AND variant = $5
            "#,
            key.media_type,
            key.media_id,
            key.image_type.as_str(),
            key.order_index,
            key.variant
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to invalidate media image variant: {}",
                e
            ))
        })?;

        debug!(
            "Invalidated media image variant: {:?}/{}",
            key.image_type, key.variant
        );

        Ok(())
    }

    async fn invalidate_all_media_image_variants(
        &self,
        media_type: &str,
        media_id: Uuid,
    ) -> Result<u32> {
        let result = sqlx::query!(
            r#"
            UPDATE media_image_variants
            SET cached = false, cached_at = NULL
            WHERE media_type = $1 AND media_id = $2 AND cached = true
            "#,
            media_type,
            media_id
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to invalidate all media image variants: {}",
                e
            ))
        })?;

        let count = result.rows_affected() as u32;
        info!(
            "Invalidated {} cached variants for {}/{}",
            count, media_type, media_id
        );

        Ok(count)
    }

    async fn cleanup_orphaned_images(&self) -> Result<u32> {
        let result = sqlx::query!(
            r#"
            DELETE FROM images
            WHERE id NOT IN (
                SELECT DISTINCT image_id FROM media_images
            )
            "#
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to cleanup orphaned images: {}",
                e
            ))
        })?;

        Ok(result.rows_affected() as u32)
    }

    async fn get_image_cache_stats(&self) -> Result<HashMap<String, u64>> {
        let mut stats = HashMap::new();

        // Total images
        let total_images = sqlx::query!("SELECT COUNT(*) as count FROM images")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Failed to count images: {}", e))
            })?;
        stats.insert(
            "total_images".to_string(),
            total_images.count.unwrap_or(0) as u64,
        );

        // Total variants
        let total_variants =
            sqlx::query!("SELECT COUNT(*) as count FROM image_variants")
                .fetch_one(&self.pool)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!(
                        "Failed to count variants: {}",
                        e
                    ))
                })?;
        stats.insert(
            "total_variants".to_string(),
            total_variants.count.unwrap_or(0) as u64,
        );

        // Total size
        let total_size = sqlx::query!(
            "SELECT COALESCE(SUM(file_size), 0) as total FROM image_variants"
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to sum sizes: {}", e))
        })?;
        stats.insert(
            "total_size_bytes".to_string(),
            total_size.total.unwrap_or(0) as u64,
        );

        // Variants by type
        let variant_counts = sqlx::query!(
            r#"
            SELECT variant, COUNT(*) as count
            FROM image_variants
            GROUP BY variant
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to count by variant: {}", e))
        })?;

        for row in variant_counts {
            stats.insert(
                format!("variant_{}", row.variant),
                row.count.unwrap_or(0) as u64,
            );
        }

        Ok(stats)
    }
}
