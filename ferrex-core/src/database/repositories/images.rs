use chrono::{DateTime, Utc};
use ferrex_model::{
    ImageMediaType, ImageSize,
    image::{ImageVariant, SqlxImageSizeVariant},
};

use async_trait::async_trait;
use sqlx::PgPool;
use std::collections::HashMap;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
    database::{
        repository_ports::images::{
            ImageRepository, ImgDbLookup, ImgInput, VarInput,
        },
        traits::{ImageRecord, OriginalImage},
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

    fn map_variant_row(&self, r: TmdbVariantRow) -> Result<OriginalImage> {
        Self::map_variant_row_static(r)
    }

    fn map_variant_row_static(r: TmdbVariantRow) -> Result<OriginalImage> {
        let imz = if r.width > 0 {
            ImageSize::original(r.width as u32, r.image_variant)
        } else {
            ImageSize::original_unknown(r.image_variant)
        };
        Ok(OriginalImage {
            iid: r.id,
            media_id: r.media_id,
            media_type: r.media_type,
            tmdb_path: r.tmdb_path,
            imz,
            iso_lang: r.iso_lang.unwrap_or_default(),
            vote_avg: r.vote_avg,
            vote_cnt: r.vote_cnt as u32,
            is_primary: r.is_primary,
        })
    }
}

#[async_trait]
impl ImageRepository for PostgresImageRepository {
    async fn cleanup_orphaned_images(&self) -> Result<u32> {
        let res = sqlx::query!(
            r#"
            DELETE FROM cached_images
            WHERE image_id NOT IN (SELECT id FROM tmdb_image_variants)
            "#
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed cleanup: {}", e)))?;
        Ok(res.rows_affected() as u32)
    }

    async fn lookup_original_image<'a>(
        &self,
        ctx: &'a ImgDbLookup,
    ) -> Result<Option<OriginalImage>> {
        // Highest priority: explicit image id, independent of size.
        if let Some(iid) = ctx.iid {
            return self.lookup_variant_by_iid(iid).await;
        }

        // Next: tmdb_path-only lookup without size constraints; this reflects the fact that
        // tmdb_image_variants is metadata-only and should not be keyed by requested size.
        if let Some(tmdb_path) = ctx.tmdb_path {
            return self.lookup_variant_by_path(tmdb_path).await;
        }

        // Finally: best-effort media-based lookup ignoring width, using image/media types.
        if let (Some(mid_uuid), Some(media_type)) =
            (ctx.media_id, ctx.media_type)
        {
            let rows = sqlx::query_as!(
                TmdbVariantRow,
                r#"
                SELECT id, tmdb_path, media_id, image_variant AS "image_variant!: ImageVariant", media_type AS "media_type: ImageMediaType", width, iso_lang,
                       vote_avg::real AS "vote_avg!: f32", vote_cnt,
                       COALESCE(is_primary, false) AS "is_primary!: bool"
                FROM tmdb_image_variants
                WHERE media_id = $1
                  AND image_variant = $2
                  AND media_type = $3
                ORDER BY is_primary DESC, vote_avg DESC, vote_cnt DESC
                LIMIT 1
                "#,
                mid_uuid,
                ctx.imz.image_variant() as ImageVariant,
                media_type as ImageMediaType,
            )
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!("Failed to get image: {}", e))
                })?;

            return rows.map(|r| self.map_variant_row(r)).transpose();
        }

        Ok(None)
    }

    async fn lookup_variants_for_media(
        &self,
        media_id: Uuid,
        media_type: ImageMediaType,
        imz: ImageSize,
    ) -> Result<Vec<OriginalImage>> {
        let mid_uuid = media_id;

        // Do NOT filter by width here: tmdb_image_variants is metadata-only and should
        // not be tied to the requested ImageSize's dimensions.
        let rows = sqlx::query_as!(
            TmdbVariantRow,
            r#"
            SELECT id, tmdb_path, media_id, image_variant AS "image_variant!: ImageVariant", media_type AS "media_type: ImageMediaType", width, iso_lang,
                   vote_avg::real AS "vote_avg!: f32", vote_cnt,
                   COALESCE(is_primary, false) AS "is_primary!: bool"
            FROM tmdb_image_variants
            WHERE media_id = $1
              AND image_variant = $2
              AND media_type = $3
            ORDER BY is_primary DESC, vote_avg DESC, vote_cnt DESC
            "#,
            mid_uuid,
            imz.image_variant() as ImageVariant,
            media_type as ImageMediaType
        )
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Failed to list image variants: {}", e))
            })?;

        rows.into_iter().map(|r| self.map_variant_row(r)).collect()
    }

    async fn lookup_variant_by_iid(
        &self,
        iid: Uuid,
    ) -> Result<Option<OriginalImage>> {
        let row = sqlx::query_as!(
            TmdbVariantRow,
            r#"
            SELECT id, tmdb_path, media_id, image_variant AS "image_variant!: ImageVariant", media_type AS "media_type: ImageMediaType", width, iso_lang, vote_avg::real AS "vote_avg!: f32", vote_cnt, COALESCE(is_primary, false) AS "is_primary!: bool"
            FROM tmdb_image_variants
            WHERE id = $1
            "#,
            iid
        )
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to get image: {}", e)))?;

        row.map(|r| self.map_variant_row(r)).transpose()
    }

    async fn lookup_variant_by_path(
        &self,
        tmdb_path: &str,
    ) -> Result<Option<OriginalImage>> {
        let row = sqlx::query_as!(
            TmdbVariantRow,
            r#"
            SELECT id, tmdb_path, media_id, image_variant AS "image_variant!: ImageVariant", media_type AS "media_type: ImageMediaType", width, iso_lang, vote_avg::real AS "vote_avg!: f32", vote_cnt, COALESCE(is_primary, false) AS "is_primary!: bool"
            FROM tmdb_image_variants
            WHERE tmdb_path = $1
            ORDER BY vote_avg DESC, vote_cnt DESC
            LIMIT 1
            "#,
            tmdb_path
        )
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to get image: {}", e)))?;

        row.map(|r| self.map_variant_row(r)).transpose()
    }

    async fn lookup_cached_image(
        &self,
        iid: Uuid,
        imz: ImageSize,
    ) -> Result<Option<ImageRecord>> {
        if imz.is_original() {
            self.lookup_original_cached_image(iid).await
        } else if imz.is_resized() {
            // Safe because resized variant implies width present
            self.lookup_resized_cached_image(iid, imz.width_unchecked() as i16)
                .await
        } else {
            let width = imz.width_unchecked() as i16;

            let row = sqlx::query_as!(
                CachedImageRow,
                r#"
	            SELECT
	                ci.image_id,
	                ci.image_variant AS "image_variant!: ImageVariant",
	                ci.width,
	                ci.height,
	                ci.size_variant AS "size_variant!: SqlxImageSizeVariant",
	                ci.theme_color,
	                ci.cache_key,
	                ci.integrity,
	                ci.byte_len,
	                ci.created_at as "created_at!: DateTime<Utc>",
	                ci.modified_at as "modified_at!: DateTime<Utc>"
	            FROM cached_images ci
            WHERE ci.image_id = $1
              AND ci.width = $2
            ORDER BY ci.modified_at DESC
            LIMIT 1
            "#,
                iid,
                width
            )
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to lookup cached image by iid: {}",
                    e
                ))
            })?;

            Ok(row.map(|r| self.map_cached_row(r)))
        }
    }

    async fn lookup_original_cached_image(
        &self,
        iid: Uuid,
    ) -> Result<Option<ImageRecord>> {
        let row = sqlx::query_as!(
            CachedImageRow,
            r#"
	            SELECT
	                ci.image_id,
	                ci.image_variant AS "image_variant!: ImageVariant",
	                ci.width,
	                ci.height,
	                ci.size_variant AS "size_variant: SqlxImageSizeVariant",
	                ci.theme_color,
	                ci.cache_key,
	                ci.integrity,
	                ci.byte_len,
	                ci.created_at as "created_at!: DateTime<Utc>",
	                ci.modified_at as "modified_at!: DateTime<Utc>"
	            FROM cached_images ci
            WHERE ci.image_id = $1
              AND ci.size_variant = $2
            ORDER BY ci.modified_at DESC
            LIMIT 1
            "#,
            iid,
            SqlxImageSizeVariant::Original as SqlxImageSizeVariant
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to lookup cached image by iid: {}",
                e
            ))
        })?;

        Ok(row.map(|r| self.map_cached_row(r)))
    }

    async fn lookup_resized_cached_image(
        &self,
        iid: Uuid,
        width: i16,
    ) -> Result<Option<ImageRecord>> {
        let row = sqlx::query_as!(
            CachedImageRow,
            r#"
	            SELECT
	                ci.image_id,
	                ci.image_variant AS "image_variant!: ImageVariant",
	                ci.width,
	                ci.height,
	                ci.size_variant AS "size_variant!: SqlxImageSizeVariant",
	                ci.theme_color,
	                ci.cache_key,
	                ci.integrity,
	                ci.byte_len,
	                ci.created_at as "created_at!: DateTime<Utc>",
	                ci.modified_at as "modified_at!: DateTime<Utc>"
	            FROM cached_images ci
            WHERE ci.image_id = $1
              AND ci.size_variant = $2
              AND ci.width = $3
            ORDER BY ci.modified_at DESC
            LIMIT 1
            "#,
            iid,
            SqlxImageSizeVariant::Resized as SqlxImageSizeVariant,
            width,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to lookup cached image by iid: {}",
                e
            ))
        })?;

        Ok(row.map(|r| self.map_cached_row(r)))
    }

    async fn lookup_images<'a>(
        &self,
        _ctx: &'a [ImgDbLookup],
    ) -> Result<Vec<ImageRecord>> {
        Ok(Vec::new())
    }

    async fn upsert_image<'a>(&self, ctx: &'a ImgInput) -> Result<ImageRecord> {
        let (width_u32, height_u32) = match ctx.decoded_dimensions {
            Some(dims) => dims.as_u32_tuple(),
            None => {
                if !ctx.imz.has_width() {
                    return Err(MediaError::Internal(
                        "Provided ImageSize must have a valid width (or decoded_dimensions must be provided)"
                            .to_string(),
                    ));
                }
                ctx.imz.dimensions_unchecked()
            }
        };

        let decoded_dimensions =
            ferrex_model::image::ImageDimensions::try_from((
                width_u32, height_u32,
            ))
            .map_err(|err| {
                MediaError::InvalidMedia(format!(
                    "Invalid image dimensions {width_u32}x{height_u32}: {err:?}"
                ))
            })?;

        let width: i16 = i16::try_from(decoded_dimensions.width_u32())
            .map_err(|_| {
                MediaError::Internal(format!(
                    "Image width out of range for i16: {}",
                    decoded_dimensions.width_u32()
                ))
            })?;
        let height: i16 = i16::try_from(decoded_dimensions.height_u32())
            .map_err(|_| {
                MediaError::Internal(format!(
                    "Image height out of range for i16: {}",
                    decoded_dimensions.height_u32()
                ))
            })?;

        info!(
            "[upsert_image] Called with iid={}, media_type={:?}, media_id={:?}, imz={:?}, imz.width()={:?}, tmdb_path={}, cache_key={}, integrity={}, byte_len={}",
            ctx.iid,
            ctx.media_type,
            ctx.media_id,
            ctx.imz,
            ctx.imz.width(),
            ctx.tmdb_path.unwrap_or("None"),
            ctx.cache_key,
            ctx.integrity,
            ctx.byte_len
        );

        let (original, resized) = (ctx.imz.is_original(), ctx.imz.is_resized());

        if original && resized {
            error!("Original and resized are both true, which is invalid");
        }

        info!(
            "[upsert_image] Final computed values: width={}, height={}, media_type={:?}, image_type={:?}",
            width,
            height,
            ctx.media_type,
            ctx.imz.image_variant()
        );

        let maybe_row = sqlx::query_as!(
            CachedImageRow,
            r#"
            INSERT INTO cached_images (
                image_id,
                image_variant,
                width,
                height,
                size_variant,
                theme_color,
                cache_key,
                integrity,
                byte_len
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (cache_key) DO UPDATE SET
                image_variant = EXCLUDED.image_variant,
                width = EXCLUDED.width,
                height = EXCLUDED.height,
                size_variant = EXCLUDED.size_variant,
                theme_color = EXCLUDED.theme_color,
                integrity = EXCLUDED.integrity,
                byte_len = EXCLUDED.byte_len,
                modified_at = now()
            WHERE cached_images.image_id = EXCLUDED.image_id
            RETURNING
                cached_images.image_id,
                cached_images.image_variant AS "image_variant!: ImageVariant",
                cached_images.width,
                cached_images.height,
                cached_images.size_variant AS "size_variant!: SqlxImageSizeVariant",
                cached_images.theme_color,
                cached_images.cache_key,
                cached_images.integrity,
                cached_images.byte_len,
                cached_images.created_at as "created_at!: DateTime<Utc>",
                cached_images.modified_at as "modified_at!: DateTime<Utc>"
            "#,
            ctx.iid,
            ctx.imz.image_variant() as ImageVariant,
            width,
            height,
            ctx.imz.sqlx_image_size_variant() as SqlxImageSizeVariant,
            ctx.theme_color.unwrap_or_default(),
            ctx.cache_key,
            ctx.integrity,
            ctx.byte_len
        )
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| {
                error!(
                    "[upsert_image] SQL failed: {} (iid={}, tmdb_path={}, width={}, cache_key={})",
                    e,
                    ctx.iid,
                    ctx.tmdb_path.unwrap_or("None"),
                    width,
                    ctx.cache_key
                );
                MediaError::Database(e)
            })?;

        let row = match maybe_row {
            Some(row) => row,
            None => {
                // This indicates a cache_key collision (same cache_key but a different image_id).
                // That should be impossible if cache_key generation is correct, so surface it as a
                // domain conflict with enough information to debug.
                let existing = sqlx::query_as!(
                    CachedImageKeyOwnerRow,
                    r#"
                    SELECT
                        image_id
                    FROM cached_images
                    WHERE cache_key = $1
                    "#,
                    ctx.cache_key
                )
                .fetch_one(&self.pool)
                .await
                .map_err(|e| MediaError::Database(e))?;

                warn!(
                    "[upsert_image] cache_key collision: cache_key={} existing_image_id={} requested_image_id={}",
                    ctx.cache_key, existing.image_id, ctx.iid
                );

                return Err(MediaError::Conflict(format!(
                    "cache_key collision for cached_images (cache_key={}, existing_image_id={}, requested_image_id={})",
                    ctx.cache_key, existing.image_id, ctx.iid
                )));
            }
        };

        let record = self.map_cached_row(row);
        info!(
            "[upsert_image] Successfully upserted: iid={}, imz={:?}, cache_key={}, integrity={}",
            record.iid, record.imz, record.cache_key, record.integrity
        );
        Ok(record)
    }

    async fn upsert_variant<'a>(
        &self,
        ctx: &'a VarInput,
    ) -> Result<OriginalImage> {
        let mut out = self.upsert_variants(std::slice::from_ref(ctx)).await?;
        out.pop().ok_or_else(|| {
            MediaError::Internal(
                "Upsert returned no image variants for single input".into(),
            )
        })
    }

    async fn upsert_variants<'a>(
        &self,
        variants: &'a [VarInput],
    ) -> Result<Vec<OriginalImage>> {
        if variants.is_empty() {
            return Ok(Vec::new());
        }

        // Avoid `INSERT .. ON CONFLICT` errors due to duplicate tmdb_path values in the same bulk
        // statement by de-duplicating to one row per path (last value wins).
        let mut unique: Vec<&VarInput> = Vec::new();
        let mut path_to_unique_idx: HashMap<&str, usize> =
            HashMap::with_capacity(variants.len());
        let mut original_to_unique: Vec<usize> =
            Vec::with_capacity(variants.len());

        for v in variants {
            let idx = match path_to_unique_idx.get(v.tmdb_path) {
                Some(&existing) => {
                    unique[existing] = v;
                    existing
                }
                None => {
                    let new_idx = unique.len();
                    unique.push(v);
                    path_to_unique_idx.insert(v.tmdb_path, new_idx);
                    new_idx
                }
            };
            original_to_unique.push(idx);
        }

        let mut image_variants: Vec<ImageVariant> =
            Vec::with_capacity(unique.len());
        let mut tmdb_paths: Vec<&str> = Vec::with_capacity(unique.len());
        let mut media_ids: Vec<Uuid> = Vec::with_capacity(unique.len());
        let mut media_types: Vec<ImageMediaType> =
            Vec::with_capacity(unique.len());
        let mut widths: Vec<i16> = Vec::with_capacity(unique.len());
        let mut heights: Vec<i16> = Vec::with_capacity(unique.len());
        let mut iso_langs: Vec<&str> = Vec::with_capacity(unique.len());
        let mut vote_avgs: Vec<f64> = Vec::with_capacity(unique.len());
        let mut vote_cnts: Vec<i32> = Vec::with_capacity(unique.len());
        let mut is_primary_flags: Vec<bool> = Vec::with_capacity(unique.len());

        for v in &unique {
            image_variants.push(v.imz.image_variant() as ImageVariant);
            tmdb_paths.push(v.tmdb_path);
            media_ids.push(v.media_id);
            media_types.push(v.media_type as ImageMediaType);
            widths.push(v.width);
            heights.push(v.height);
            iso_langs.push(v.lang);
            vote_avgs.push(v.v_avg as f64);
            vote_cnts.push(v.v_cnt.min(i32::MAX as u32) as i32);
            is_primary_flags.push(v.is_primary);
        }

        let rows = sqlx::query_as!(
            TmdbVariantRowWithOrd,
            r#"
            WITH input AS (
                SELECT
                    ord::bigint AS ord,
                    image_variant,
                    tmdb_path,
                    media_id,
                    media_type,
                    width,
                    height,
                    iso_lang,
                    vote_avg,
                    vote_cnt,
                    is_primary
                FROM UNNEST(
                    $1::image_variant[],
                    $2::varchar[],
                    $3::uuid[],
                    $4::media_type[],
                    $5::smallint[],
                    $6::smallint[],
                    $7::varchar[],
                    $8::float8[],
                    $9::int4[],
                    $10::bool[]
                ) WITH ORDINALITY AS v(
                    image_variant,
                    tmdb_path,
                    media_id,
                    media_type,
                    width,
                    height,
                    iso_lang,
                    vote_avg,
                    vote_cnt,
                    is_primary,
                    ord
                )
            ),
            upserted AS (
                INSERT INTO tmdb_image_variants (
                    image_variant,
                    tmdb_path,
                    media_id,
                    media_type,
                    width,
                    height,
                    iso_lang,
                    vote_avg,
                    vote_cnt,
                    is_primary
                )
                SELECT
                    image_variant,
                    tmdb_path,
                    media_id,
                    media_type,
                    width,
                    height,
                    iso_lang,
                    vote_avg,
                    vote_cnt,
                    is_primary
                FROM input
                ON CONFLICT (tmdb_path) DO UPDATE SET
                    image_variant = EXCLUDED.image_variant,
                    media_id = EXCLUDED.media_id,
                    media_type = EXCLUDED.media_type,
                    width = EXCLUDED.width,
                    height = EXCLUDED.height,
                    iso_lang = EXCLUDED.iso_lang,
                    vote_avg = EXCLUDED.vote_avg,
                    vote_cnt = EXCLUDED.vote_cnt,
                    is_primary = EXCLUDED.is_primary
                RETURNING id, image_variant, tmdb_path, media_id, media_type, width, iso_lang, vote_avg, vote_cnt, COALESCE(is_primary, false) AS is_primary
            )
            SELECT
                input.ord AS "_ord!",
                upserted.id,
                upserted.image_variant AS "image_variant!: ImageVariant",
                upserted.tmdb_path,
                upserted.media_id,
                upserted.media_type AS "media_type: ImageMediaType",
                upserted.width,
                upserted.iso_lang,
                upserted.vote_avg::real AS "vote_avg!: f32",
                upserted.vote_cnt,
                upserted.is_primary AS "is_primary!: bool"
            FROM upserted
            JOIN input USING (tmdb_path)
            ORDER BY input.ord
            "#,
            &image_variants as &[ImageVariant],
            &tmdb_paths as &[&str],
            &media_ids as &[Uuid],
            &media_types as &[ImageMediaType],
            &widths as &[i16],
            &heights as &[i16],
            &iso_langs as &[&str],
            &vote_avgs as &[f64],
            &vote_cnts as &[i32],
            &is_primary_flags as &[bool],
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to bulk upsert image variants: {}",
                e
            ))
        })?;

        if rows.len() != unique.len() {
            return Err(MediaError::Internal(format!(
                "bulk upsert returned unexpected row count: got {}, expected {}",
                rows.len(),
                unique.len()
            )));
        }

        let mut unique_out: Vec<OriginalImage> = Vec::with_capacity(rows.len());
        for r in rows {
            let row = TmdbVariantRow {
                id: r.id,
                image_variant: r.image_variant,
                tmdb_path: r.tmdb_path,
                media_id: r.media_id,
                media_type: r.media_type,
                width: r.width,
                iso_lang: r.iso_lang,
                vote_avg: r.vote_avg,
                vote_cnt: r.vote_cnt,
                is_primary: r.is_primary,
            };
            unique_out.push(self.map_variant_row(row)?);
        }

        let mut out: Vec<OriginalImage> = Vec::with_capacity(variants.len());
        for idx in original_to_unique {
            out.push(unique_out[idx].clone());
        }

        Ok(out)
    }
}

#[derive(sqlx::FromRow)]
struct TmdbVariantRow {
    id: Uuid,
    image_variant: ImageVariant,
    tmdb_path: String,
    media_id: Uuid,
    media_type: ImageMediaType,
    width: i16,
    iso_lang: Option<String>,
    vote_avg: f32,
    vote_cnt: i32,
    is_primary: bool,
}

#[derive(sqlx::FromRow)]
struct TmdbVariantRowWithOrd {
    _ord: i64,
    id: Uuid,
    image_variant: ImageVariant,
    tmdb_path: String,
    media_id: Uuid,
    media_type: ImageMediaType,
    width: i16,
    iso_lang: Option<String>,
    vote_avg: f32,
    vote_cnt: i32,
    is_primary: bool,
}

#[derive(sqlx::FromRow)]
struct CachedImageRow {
    image_id: Uuid,
    image_variant: ImageVariant,
    width: i16,
    height: i16,
    size_variant: SqlxImageSizeVariant,
    theme_color: Option<String>,
    cache_key: String,
    integrity: String,
    byte_len: i32,
    created_at: DateTime<Utc>,
    modified_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct CachedImageKeyOwnerRow {
    image_id: Uuid,
}

impl PostgresImageRepository {
    fn map_cached_row(&self, r: CachedImageRow) -> ImageRecord {
        let imz = match r.size_variant {
            SqlxImageSizeVariant::Original => {
                ImageSize::original(r.width as u32, r.image_variant)
            }
            SqlxImageSizeVariant::Resized => {
                ImageSize::custom(r.width as u32, r.image_variant)
            }
            SqlxImageSizeVariant::Tmdb => {
                // Can return custom resized if width is somehow not a valid tmdb width variant
                ImageSize::from_size_and_variant(
                    r.width as u32,
                    r.image_variant,
                )
            }
        };

        ImageRecord {
            iid: r.image_id,
            imz,
            theme_color: r.theme_color.unwrap_or_default(),
            dimensions: (r.width as u32, r.height as u32),
            cache_key: r.cache_key,
            integrity: r.integrity,
            byte_len: r.byte_len,
            created_at: r.created_at,
            modified_at: r.modified_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_variant_row_treats_zero_width_as_unknown_original_size() {
        let row = TmdbVariantRow {
            id: Uuid::new_v4(),
            image_variant: ImageVariant::Backdrop,
            tmdb_path: "/backdrop.jpg".to_string(),
            media_id: Uuid::new_v4(),
            media_type: ImageMediaType::Movie,
            width: 0,
            iso_lang: None,
            vote_avg: 0.0,
            vote_cnt: 0,
            is_primary: true,
        };

        let mapped = PostgresImageRepository::map_variant_row_static(row)
            .expect("mapping should succeed");
        assert!(!mapped.imz.has_width());
        assert!(matches!(mapped.imz, ImageSize::Backdrop(_)));
    }

    #[test]
    fn map_variant_row_preserves_positive_width_as_original_width() {
        let row = TmdbVariantRow {
            id: Uuid::new_v4(),
            image_variant: ImageVariant::Backdrop,
            tmdb_path: "/backdrop.jpg".to_string(),
            media_id: Uuid::new_v4(),
            media_type: ImageMediaType::Movie,
            width: 1280,
            iso_lang: None,
            vote_avg: 0.0,
            vote_cnt: 0,
            is_primary: true,
        };

        let mapped = PostgresImageRepository::map_variant_row_static(row)
            .expect("mapping should succeed");
        assert!(mapped.imz.has_width());
        assert_eq!(mapped.imz.width(), Some(1280));
        assert!(matches!(mapped.imz, ImageSize::Backdrop(_)));
    }
}
