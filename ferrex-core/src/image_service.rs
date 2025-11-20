use crate::MediaImageKind;
use crate::database::ports::images::ImageRepository;
use crate::database::ports::media_files::MediaFilesReadPort;
use crate::database::traits::{ImageLookupParams, ImageRecord, ImageVariant};
use crate::error::{MediaError, Result};
use crate::image::records::{MediaImageVariantKey, MediaImageVariantRecord};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::any::type_name_of_val;
use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use tracing::{debug, info, warn};
use uuid::Uuid;

#[cfg(feature = "ffmpeg")]
use ffmpeg_next as ffmpeg;
#[cfg(feature = "ffmpeg")]
use once_cell::sync::OnceCell;

/// TMDB image size variants
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TmdbImageSize {
    // Poster sizes
    PosterW92,
    PosterW154,
    PosterW185,
    PosterW300,
    PosterW342,
    PosterW500,
    PosterW780,
    // Backdrop sizes
    BackdropW300,
    BackdropW780,
    BackdropW1280,
    // Still sizes
    StillW92,
    StillW185,
    StillW300,
    StillW500,
    // Profile sizes
    ProfileW45,
    ProfileW185,
    ProfileH632,
    // Original
    Original,
}

impl TmdbImageSize {
    pub fn as_str(&self) -> &'static str {
        match self {
            TmdbImageSize::PosterW92 => "w92",
            TmdbImageSize::PosterW154 => "w154",
            TmdbImageSize::PosterW185 => "w185",
            TmdbImageSize::PosterW300 => "w300",
            TmdbImageSize::PosterW342 => "w342",
            TmdbImageSize::PosterW500 => "w500",
            TmdbImageSize::PosterW780 => "w780",
            TmdbImageSize::BackdropW300 => "w300",
            TmdbImageSize::BackdropW780 => "w780",
            TmdbImageSize::BackdropW1280 => "w1280",
            TmdbImageSize::StillW92 => "w92",
            TmdbImageSize::StillW185 => "w185",
            TmdbImageSize::StillW300 => "w300",
            TmdbImageSize::StillW500 => "w500",
            TmdbImageSize::ProfileW45 => "w45",
            TmdbImageSize::ProfileW185 => "w185",
            TmdbImageSize::ProfileH632 => "h632",
            TmdbImageSize::Original => "original",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "w92" => Some(TmdbImageSize::PosterW92),
            "w154" => Some(TmdbImageSize::PosterW154),
            "w185" => Some(TmdbImageSize::PosterW185),
            "w300" => Some(TmdbImageSize::PosterW300),
            "w342" => Some(TmdbImageSize::PosterW342),
            "w500" => Some(TmdbImageSize::PosterW500),
            "w780" => Some(TmdbImageSize::PosterW780),
            "w1280" => Some(TmdbImageSize::BackdropW1280),
            "h632" => Some(TmdbImageSize::ProfileH632),
            "w45" => Some(TmdbImageSize::ProfileW45),
            "original" => Some(TmdbImageSize::Original),
            _ => None,
        }
    }

    /// Get recommended sizes for native client usage
    pub fn recommended_for_kind(kind: &MediaImageKind) -> Vec<Self> {
        match kind {
            // Prioritize ~300w poster (w342) first for fast above-the-fold loads,
            // then a larger fallback for high-DPI/detail, followed by a small thumb.
            MediaImageKind::Poster => vec![
                TmdbImageSize::PosterW300,
                TmdbImageSize::PosterW500,
                TmdbImageSize::PosterW185,
            ],
            MediaImageKind::Backdrop => vec![
                TmdbImageSize::Original,
                TmdbImageSize::BackdropW780,
                TmdbImageSize::BackdropW1280,
            ],
            MediaImageKind::Logo => vec![TmdbImageSize::Original], // SVG logos should use original
            MediaImageKind::Thumbnail => {
                vec![TmdbImageSize::StillW300, TmdbImageSize::StillW500]
            }
            MediaImageKind::Cast => vec![TmdbImageSize::ProfileW185],
            MediaImageKind::Other(_) => vec![TmdbImageSize::Original],
        }
    }
}

#[derive(Clone)]
pub struct ImageService {
    media_files: Arc<dyn MediaFilesReadPort>,
    images: Arc<dyn ImageRepository>,
    cache_dir: PathBuf,
    http_client: reqwest::Client,
    // Non-blocking variant generation coordination
    in_flight: Arc<Mutex<HashSet<String>>>,
    permits: Arc<Semaphore>,
}

impl fmt::Debug for ImageService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let in_flight = self
            .in_flight
            .try_lock()
            .map(|guard| guard.len())
            .unwrap_or(0);

        f.debug_struct("ImageService")
            .field(
                "media_files_repository",
                &type_name_of_val(self.media_files.as_ref()),
            )
            .field("image_repository", &type_name_of_val(self.images.as_ref()))
            .field("cache_dir", &self.cache_dir)
            .field("http_client", &self.http_client)
            .field("in_flight_requests", &in_flight)
            .field("permits_available", &self.permits.available_permits())
            .finish()
    }
}

impl ImageService {
    pub fn new(
        media_files: Arc<dyn MediaFilesReadPort>,
        images: Arc<dyn ImageRepository>,
        cache_dir: PathBuf,
    ) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        // Ensure cache_dir is absolute so stored file paths are stable
        let cache_dir = if cache_dir.is_absolute() {
            cache_dir
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(cache_dir)
        };

        Self {
            media_files,
            images,
            cache_dir,
            http_client,
            in_flight: Arc::new(Mutex::new(HashSet::new())),
            permits: Arc::new(Semaphore::new(4)), // cap concurrent variant work
        }
    }

    /// Register a TMDB image in the database
    pub async fn register_tmdb_image(
        &self,
        tmdb_path: &str,
    ) -> Result<ImageRecord> {
        // Check if already registered
        if let Some(existing) =
            self.images.get_image_by_tmdb_path(tmdb_path).await?
        {
            return Ok(existing);
        }

        // Create new image record
        self.images.create_image(tmdb_path).await
    }

    /// Download and cache an image variant, keeping metadata in sync with the media_variant table.
    pub async fn download_variant(
        &self,
        tmdb_path: &str,
        size: TmdbImageSize,
        context: Option<MediaImageVariantKey>,
    ) -> Result<PathBuf> {
        let image_record = self.register_tmdb_image(tmdb_path).await?;
        let variant_name = size.as_str();
        let image_folder = context
            .as_ref()
            .map(|key| image_type_folder(&key.image_type))
            .unwrap_or("untyped");

        if let Some(existing) = self
            .images
            .get_image_variant(image_record.id, variant_name)
            .await?
        {
            let path = PathBuf::from(&existing.file_path);
            // Require the new folder structure; treat legacy paths as missing so we re-download.
            let variant_dir_ok = path
                .parent()
                .map(|dir| dir.ends_with(variant_name))
                .unwrap_or(false);
            let type_dir_ok = path
                .parent()
                .and_then(|dir| dir.parent())
                .map(|dir| dir.ends_with(image_folder))
                .unwrap_or(false);
            let in_expected_dir = variant_dir_ok && type_dir_ok;

            if path.exists() && in_expected_dir {
                let mut theme_color: Option<String> = None;
                let mut content_hash: Option<String> = None;

                if let Ok(bytes) = tokio::fs::read(&path).await {
                    if should_extract_theme_color(
                        context.as_ref(),
                        variant_name,
                    ) {
                        theme_color = self.extract_theme_color(&bytes);
                    }
                    content_hash = Some(self.calculate_hash(&bytes));
                }

                if let Some(ref key) = context {
                    self.images
                        .mark_media_image_variant_cached(
                            key,
                            existing.width,
                            existing.height,
                            content_hash.as_deref(),
                            theme_color.as_deref(),
                        )
                        .await?;

                    if let Some(color) = theme_color {
                        self.images
                            .update_media_theme_color(
                                &key.media_type,
                                key.media_id,
                                Some(&color),
                            )
                            .await?;
                    }
                }

                return Ok(path);
            } else {
                warn!(
                    "Variant record exists but file missing on disk, re-downloading: {}",
                    existing.file_path
                );
            }
        }

        let url = format!(
            "https://image.tmdb.org/t/p/{}/{}",
            variant_name, tmdb_path
        );
        //debug!("Downloading image variant: {}", url);

        let response =
            self.http_client.get(&url).send().await.map_err(|e| {
                MediaError::Internal(format!("Failed to download image: {}", e))
            })?;

        if !response.status().is_success() {
            return Err(MediaError::Internal(format!(
                "Failed to download image: HTTP {}",
                response.status()
            )));
        }

        let bytes = response.bytes().await.map_err(|e| {
            MediaError::Internal(format!("Failed to read image data: {}", e))
        })?;

        let variant_dir = self
            .cache_dir
            .join("images")
            .join(image_folder)
            .join(variant_name);
        tokio::fs::create_dir_all(&variant_dir)
            .await
            .map_err(MediaError::Io)?;

        let filename = build_variant_filename(
            tmdb_path,
            variant_name,
            image_folder,
            context.as_ref(),
        );
        let file_path = variant_dir.join(&filename);

        tokio::fs::write(&file_path, &bytes)
            .await
            .map_err(MediaError::Io)?;

        let (width, height) = match self.get_image_dimensions(&bytes) {
            Ok((w, h)) => (Some(w as i32), Some(h as i32)),
            Err(e) => {
                warn!("Failed to get image dimensions: {}", e);
                (None, None)
            }
        };

        let hash = self.calculate_hash(&bytes);
        let format = self.detect_format(&bytes);
        let theme_color =
            if should_extract_theme_color(context.as_ref(), variant_name) {
                self.extract_theme_color(&bytes)
            } else {
                None
            };

        if image_record.file_hash.is_none() {
            self.images
                .update_image_metadata(
                    image_record.id,
                    &hash,
                    bytes.len() as i32,
                    width.unwrap_or(0),
                    height.unwrap_or(0),
                    &format,
                )
                .await?;
        }

        if let Some(existing_image) =
            self.images.get_image_by_hash(&hash).await?
        {
            debug!("Found duplicate image by hash: {}", hash);

            if let Some(existing_variant) = self
                .images
                .get_image_variant(existing_image.id, variant_name)
                .await?
            {
                if let Some(ref key) = context {
                    self.images
                        .mark_media_image_variant_cached(
                            key,
                            existing_variant.width,
                            existing_variant.height,
                            Some(&hash),
                            theme_color.as_deref(),
                        )
                        .await?;

                    if let Some(color) = theme_color {
                        self.images
                            .update_media_theme_color(
                                &key.media_type,
                                key.media_id,
                                Some(&color),
                            )
                            .await?;
                    }
                }

                return Ok(PathBuf::from(&existing_variant.file_path));
            }

            let variant = self
                .images
                .create_image_variant(
                    existing_image.id,
                    variant_name,
                    file_path.to_string_lossy().as_ref(),
                    bytes.len() as i32,
                    width,
                    height,
                )
                .await?;

            if let Some(ref key) = context {
                self.images
                    .mark_media_image_variant_cached(
                        key,
                        variant.width,
                        variant.height,
                        Some(&hash),
                        theme_color.as_deref(),
                    )
                    .await?;

                if let Some(color) = theme_color {
                    self.images
                        .update_media_theme_color(
                            &key.media_type,
                            key.media_id,
                            Some(&color),
                        )
                        .await?;
                }
            }

            return Ok(file_path);
        }

        let variant = self
            .images
            .create_image_variant(
                image_record.id,
                variant_name,
                file_path.to_string_lossy().as_ref(),
                bytes.len() as i32,
                width,
                height,
            )
            .await?;

        if let Some(ref key) = context {
            self.images
                .mark_media_image_variant_cached(
                    key,
                    variant.width,
                    variant.height,
                    Some(&hash),
                    theme_color.as_deref(),
                )
                .await?;

            if let Some(color) = theme_color {
                self.images
                    .update_media_theme_color(
                        &key.media_type,
                        key.media_id,
                        Some(&color),
                    )
                    .await?;
            }
        }

        Ok(file_path)
    }

    pub async fn generate_episode_thumbnail(
        &self,
        image_key: &str,
        media_file_id: Uuid,
        key: MediaImageVariantKey,
    ) -> Result<PathBuf> {
        self.ensure_ffmpeg_initialized()?;

        let image_record = self.register_tmdb_image(image_key).await?;
        let variant_name = key.variant.clone();
        let image_folder = image_type_folder(&key.image_type);

        if let Some(existing) = self
            .images
            .get_image_variant(image_record.id, &variant_name)
            .await?
        {
            let existing_path = PathBuf::from(&existing.file_path);
            let variant_dir_ok = existing_path
                .parent()
                .map(|dir| dir.ends_with(&variant_name))
                .unwrap_or(false);
            let type_dir_ok = existing_path
                .parent()
                .and_then(|dir| dir.parent())
                .map(|dir| dir.ends_with(image_folder))
                .unwrap_or(false);

            if existing_path.exists() && variant_dir_ok && type_dir_ok {
                let mut width = existing.width;
                let mut height = existing.height;
                let mut content_hash: Option<String> = None;

                if let Ok(bytes) = tokio::fs::read(&existing_path).await {
                    content_hash = Some(self.calculate_hash(&bytes));
                    if (width.is_none() || height.is_none())
                        && let Ok((w, h)) = self.get_image_dimensions(&bytes)
                    {
                        width = Some(w as i32);
                        height = Some(h as i32);
                    }
                }

                self.images
                    .mark_media_image_variant_cached(
                        &key,
                        width,
                        height,
                        content_hash.as_deref(),
                        None,
                    )
                    .await?;

                return Ok(existing_path);
            }
        }

        let media = self
            .media_files
            .get_by_id(&media_file_id)
            .await?
            .ok_or_else(|| {
                MediaError::NotFound(
                    "Media file missing for thumbnail generation".into(),
                )
            })?;

        let video_path = media.path.clone();

        let variant_dir = self
            .cache_dir
            .join("images")
            .join(image_folder)
            .join(&variant_name);
        tokio::fs::create_dir_all(&variant_dir)
            .await
            .map_err(MediaError::Io)?;

        let filename = build_variant_filename(
            image_key,
            &variant_name,
            image_folder,
            Some(&key),
        );
        let file_path = variant_dir.join(&filename);

        let output_path = file_path.clone();
        let video_path_string = video_path.to_string_lossy().to_string();
        tokio::task::spawn_blocking(move || {
            extract_frame_at_percentage(&video_path_string, &output_path, 0.3)
        })
        .await
        .map_err(|err| {
            MediaError::Internal(format!("Failed to join ffmpeg task: {err}"))
        })??;

        let bytes =
            tokio::fs::read(&file_path).await.map_err(MediaError::Io)?;
        let file_size_i32 = bytes.len() as i32;

        let (width, height) = match self.get_image_dimensions(&bytes) {
            Ok((w, h)) => (Some(w as i32), Some(h as i32)),
            Err(e) => {
                warn!("Failed to get thumbnail dimensions: {}", e);
                (None, None)
            }
        };

        let hash = self.calculate_hash(&bytes);
        let format = self.detect_format(&bytes);

        if image_record.file_hash.is_none() {
            self.images
                .update_image_metadata(
                    image_record.id,
                    &hash,
                    file_size_i32,
                    width.unwrap_or(0),
                    height.unwrap_or(0),
                    &format,
                )
                .await?;
        }

        if let Some(existing_image) =
            self.images.get_image_by_hash(&hash).await?
        {
            if let Some(existing_variant) = self
                .images
                .get_image_variant(existing_image.id, &variant_name)
                .await?
            {
                self.images
                    .mark_media_image_variant_cached(
                        &key,
                        existing_variant.width,
                        existing_variant.height,
                        Some(&hash),
                        None,
                    )
                    .await?;

                return Ok(PathBuf::from(&existing_variant.file_path));
            }

            self.images
                .create_image_variant(
                    existing_image.id,
                    &variant_name,
                    file_path.to_string_lossy().as_ref(),
                    file_size_i32,
                    width,
                    height,
                )
                .await?;
        } else {
            self.images
                .create_image_variant(
                    image_record.id,
                    &variant_name,
                    file_path.to_string_lossy().as_ref(),
                    file_size_i32,
                    width,
                    height,
                )
                .await?;
        }

        self.images
            .mark_media_image_variant_cached(
                &key,
                width,
                height,
                Some(&hash),
                None,
            )
            .await?;

        Ok(file_path)
    }

    /// Link an image to a media item
    pub async fn link_to_media(
        &self,
        media_type: &str,
        media_id: Uuid,
        tmdb_path: &str,
        image_type: MediaImageKind,
        order_index: i32,
        is_primary: bool,
    ) -> Result<Uuid> {
        //debug!(
        //    "link_to_media: type={}, id={}, tmdb_path={}, image_type={}, index={}",
        //    media_type, media_id, tmdb_path, image_type, order_index
        //);

        // Ensure image is registered
        let image_record = self.register_tmdb_image(tmdb_path).await?;

        // Create the link
        self.images
            .link_media_image(
                media_type,
                media_id,
                image_record.id,
                image_type,
                order_index,
                is_primary,
            )
            .await?;

        Ok(image_record.id)
    }

    /// Ensure an image variant exists by spawning background work on cache miss.
    /// Returns Some(path) if available immediately, or None if queued.
    pub async fn ensure_variant_async(
        &self,
        params: &ImageLookupParams,
    ) -> Result<Option<PathBuf>> {
        debug!(
            "ensure_variant_async: type={}, id={}, image_type={}, index={}, variant={:?}",
            params.media_type,
            params.media_id,
            params.image_type,
            params.index,
            params.variant
        );

        // Lookup the image record and check if the desired variant exists
        if let Some((image_record, existing_variant)) =
            self.images.lookup_image_variant(params).await?
        {
            let requested_variant = params.variant.as_deref();
            let variant_key_struct =
                requested_variant.and_then(|v| build_variant_key(params, v));

            if let Some(variant) = existing_variant {
                if let Some(ref key) = variant_key_struct {
                    let record = MediaImageVariantRecord {
                        requested_at: Utc::now(),
                        cached_at: Some(Utc::now()),
                        cached: true,
                        width: variant.width,
                        height: variant.height,
                        content_hash: None,
                        theme_color: None,
                        key: key.clone(),
                    };
                    self.images.upsert_media_image_variant(&record).await?;
                }
                return Ok(Some(PathBuf::from(variant.file_path)));
            }

            // Missing: spawn background download for requested size if it maps to TMDB size
            if let Some(size_str) = requested_variant
                && let Some(size) = TmdbImageSize::from_str(size_str)
            {
                let key = self.variant_key(params);
                let mut guard = self.in_flight.lock().await;
                if !guard.contains(&key) {
                    guard.insert(key.clone());
                    drop(guard);

                    if let Some(ref key_struct) = variant_key_struct {
                        let record = MediaImageVariantRecord {
                            requested_at: Utc::now(),
                            cached_at: None,
                            cached: false,
                            width: None,
                            height: None,
                            content_hash: None,
                            theme_color: None,
                            key: key_struct.clone(),
                        };
                        self.images.upsert_media_image_variant(&record).await?;
                    }

                    let this = self.clone();
                    let tmdb_path = image_record.tmdb_path.clone();
                    let permits = self.permits.clone();
                    let key_struct = variant_key_struct.clone();
                    tokio::spawn(async move {
                        let _permit =
                            permits.acquire().await.expect("semaphore");
                        if let Err(e) = this
                            .download_variant(
                                &tmdb_path,
                                size,
                                key_struct.clone(),
                            )
                            .await
                        {
                            warn!(
                                "Background variant download failed for {} {}: {}",
                                tmdb_path,
                                size.as_str(),
                                e
                            );
                        }
                        let mut g = this.in_flight.lock().await;
                        g.remove(&key);
                    });
                } else {
                    drop(guard);
                    debug!(
                        "Variant generation already in-flight for key: {}",
                        key
                    );
                }
            }

            // Not immediately available
            return Ok(None);
        }

        Ok(None)
    }

    /// Pick the best available fallback variant for the same image.
    /// Returns (path, variant_name) if found.
    pub async fn pick_best_available(
        &self,
        params: &ImageLookupParams,
        target_width: Option<u32>,
    ) -> Result<Option<(PathBuf, String)>> {
        if let Some((image_record, _)) =
            self.images.lookup_image_variant(params).await?
        {
            let mut variants =
                self.images.get_image_variants(image_record.id).await?;
            if variants.is_empty() {
                return Ok(None);
            }
            // Try to pick the closest by width relative to requested variant name, if present
            let target_w = target_width
                .or_else(|| {
                    params
                        .variant
                        .as_deref()
                        .and_then(tmdb_variant_to_width_hint)
                })
                .unwrap_or(500);

            variants.sort_by_key(|v| {
                tmdb_variant_to_width_hint(&v.variant)
                    .unwrap_or(i32::MAX as u32)
            });

            // Prefer <= target; else smallest > target
            let mut best: Option<&ImageVariant> = None;
            for v in variants.iter().rev() {
                // high to low
                if let Some(w) = tmdb_variant_to_width_hint(&v.variant)
                    && w <= target_w
                {
                    best = Some(v);
                    break;
                }
            }
            if best.is_none() {
                // find the smallest > target
                for v in variants.iter() {
                    if let Some(w) = tmdb_variant_to_width_hint(&v.variant) {
                        if w >= target_w {
                            best = Some(v);
                            break;
                        }
                    } else {
                        // Unknown variant width; consider as last resort
                        best = Some(v);
                        break;
                    }
                }
            }

            if let Some(v) = best {
                return Ok(Some((
                    PathBuf::from(&v.file_path),
                    v.variant.clone(),
                )));
            }
        }
        Ok(None)
    }

    fn variant_key(&self, params: &ImageLookupParams) -> String {
        format!(
            "{}:{}:{}:{}:{}",
            params.media_type,
            params.media_id,
            params.image_type.as_str(),
            params.index,
            params.variant.as_deref().unwrap_or("")
        )
    }

    /// Get or download an image variant (blocking until ready)
    pub async fn get_or_download_variant(
        &self,
        params: &ImageLookupParams,
    ) -> Result<Option<PathBuf>> {
        debug!(
            "get_or_download_variant called with params: type={}, id={}, image_type={}, index={}, variant={:?}",
            params.media_type,
            params.media_id,
            params.image_type,
            params.index,
            params.variant
        );

        // Look up the image in database
        if let Some((image_record, _)) =
            self.images.lookup_image_variant(params).await?
        {
            if let Some(size_str) = &params.variant
                && let Some(size) = TmdbImageSize::from_str(size_str)
            {
                let key_struct = build_variant_key(params, size_str);
                let path = self
                    .download_variant(&image_record.tmdb_path, size, key_struct)
                    .await?;
                return Ok(Some(path));
            }

            let variants =
                self.images.get_image_variants(image_record.id).await?;
            if let Some(variant) = variants.first() {
                return Ok(Some(PathBuf::from(&variant.file_path)));
            }
        }

        Ok(None)
    }

    /// Calculate SHA256 hash of image data
    fn calculate_hash(&self, data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    /// Detect image format from bytes
    fn detect_format(&self, data: &[u8]) -> String {
        if data.len() < 4 {
            return "unknown".to_string();
        }

        match &data[0..4] {
            [0xFF, 0xD8, 0xFF, _] => "jpg",
            [0x89, 0x50, 0x4E, 0x47] => "png",
            [0x52, 0x49, 0x46, 0x46] => "webp",
            _ => "unknown",
        }
        .to_string()
    }

    /// Get image dimensions using the image crate
    fn get_image_dimensions(&self, data: &[u8]) -> Result<(u32, u32)> {
        use image::GenericImageView;

        let img = image::load_from_memory(data).map_err(|e| {
            MediaError::InvalidMedia(format!("Failed to decode image: {}", e))
        })?;

        Ok(img.dimensions())
    }

    /// Extract dominant color from image data
    pub fn extract_theme_color(&self, data: &[u8]) -> Option<String> {
        use image::{GenericImageView, Rgba};
        use std::collections::HashMap;

        // Try to decode the image
        let img = match image::load_from_memory(data) {
            Ok(img) => img,
            Err(e) => {
                warn!("Failed to decode image for color extraction: {}", e);
                return None;
            }
        };

        let (width, height) = img.dimensions();

        // Skip very small images
        if width < 50 || height < 50 {
            return None;
        }

        // Sample pixels from a 5x5 grid, excluding 10% border
        let border_x = width / 10;
        let border_y = height / 10;
        let sample_width = width - (2 * border_x);
        let sample_height = height - (2 * border_y);

        let mut color_counts: HashMap<[u8; 3], u32> = HashMap::new();

        // Sample 25 points in a grid pattern
        for i in 0..5 {
            for j in 0..5 {
                let x = border_x + (i * sample_width / 4);
                let y = border_y + (j * sample_height / 4);

                let pixel = img.get_pixel(x, y);
                let Rgba([r, g, b, a]) = pixel;

                // Skip transparent or near-transparent pixels
                if a < 128 {
                    continue;
                }

                // Quantize colors to reduce noise (round to nearest 16)
                let quantized = [(r / 16) * 16, (g / 16) * 16, (b / 16) * 16];

                *color_counts.entry(quantized).or_insert(0) += 1;
            }
        }

        // Find the most common non-grayscale color
        let mut best_color = None;
        let mut best_count = 0;
        let mut best_saturation = 0.0;

        for (color, count) in &color_counts {
            let [r, g, b] = *color;

            // Skip near-black and near-white colors
            let brightness = (r as u32 + g as u32 + b as u32) / 3;
            if !(30..=225).contains(&brightness) {
                continue;
            }

            // Calculate saturation (how colorful it is)
            let max = r.max(g).max(b) as f32;
            let min = r.min(g).min(b) as f32;
            let saturation = if max > 0.0 { (max - min) / max } else { 0.0 };

            // Prefer more saturated colors
            if *count > best_count
                || (*count == best_count && saturation > best_saturation)
            {
                best_color = Some(*color);
                best_count = *count;
                best_saturation = saturation;
            }
        }

        // Convert to hex color
        let hex_color =
            best_color.map(|[r, g, b]| format!("#{:02x}{:02x}{:02x}", r, g, b));
        if let Some(ref color) = hex_color {
            info!(
                "Extracted theme color: {} (saturation: {:.2})",
                color, best_saturation
            );
        } else {
            info!(
                "No suitable theme color found (sampled {} colors)",
                color_counts.len()
            );
        }
        hex_color
    }

    #[cfg(feature = "ffmpeg")]
    fn ensure_ffmpeg_initialized(&self) -> Result<()> {
        static INIT: OnceCell<()> = OnceCell::new();
        INIT.get_or_try_init(|| {
            ffmpeg::init().map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to initialize ffmpeg: {e}"
                ))
            })
        })
        .map(|_| ())
    }

    #[cfg(not(feature = "ffmpeg"))]
    fn ensure_ffmpeg_initialized(&self) -> Result<()> {
        Err(MediaError::Internal(
            "FFmpeg support is required for thumbnail generation".into(),
        ))
    }

    /// Clean up orphaned images
    pub async fn cleanup_orphaned(&self) -> Result<u32> {
        self.images.cleanup_orphaned_images().await
    }

    /// Get cache statistics
    pub async fn get_stats(
        &self,
    ) -> Result<std::collections::HashMap<String, u64>> {
        self.images.get_image_cache_stats().await
    }
}

fn tmdb_variant_to_width_hint(variant: &str) -> Option<u32> {
    // Extract numeric part like "w500" -> 500; treat "original" as very large
    if variant == "original" {
        return Some(10000);
    }
    if let Some(rest) = variant.strip_prefix('w') {
        return rest.parse::<u32>().ok();
    }
    None
}

fn build_variant_filename(
    tmdb_path: &str,
    variant: &str,
    image_folder: &str,
    context: Option<&MediaImageVariantKey>,
) -> String {
    let sanitized = tmdb_path.trim_start_matches('/').replace('/', "_");
    match context {
        Some(key) => format!(
            "{}__{}__{}__{}__{}__{}__{}",
            key.media_type,
            key.media_id,
            image_folder,
            key.image_type.as_str(),
            key.order_index,
            variant,
            sanitized
        ),
        None => format!("{}__{}__{}", image_folder, variant, sanitized),
    }
}

fn should_extract_theme_color(
    context: Option<&MediaImageVariantKey>,
    variant: &str,
) -> bool {
    match context {
        Some(key) => {
            matches!(key.image_type, MediaImageKind::Poster)
                && matches!(variant, "w300" | "w342" | "w185")
        }
        None => false,
    }
}

fn build_variant_key(
    params: &ImageLookupParams,
    variant: &str,
) -> Option<MediaImageVariantKey> {
    let media_id = Uuid::parse_str(&params.media_id).ok()?;
    Some(MediaImageVariantKey {
        media_type: params.media_type.clone(),
        media_id,
        image_type: params.image_type.clone(),
        order_index: params.index as i32,
        variant: variant.to_string(),
    })
}

fn image_type_folder(image_type: &MediaImageKind) -> &str {
    match image_type {
        MediaImageKind::Poster => "poster",
        MediaImageKind::Backdrop => "backdrop",
        MediaImageKind::Logo => "logo",
        MediaImageKind::Thumbnail => "thumbnail",
        MediaImageKind::Cast => "cast",
        MediaImageKind::Other(value) => value.as_str(),
    }
}

#[cfg(feature = "ffmpeg")]
fn extract_frame_at_percentage(
    input_path: &str,
    output_path: &Path,
    percentage: f64,
) -> Result<()> {
    use ffmpeg::codec::context::Context as CodecContext;

    let mut input_ctx = ffmpeg::format::input(&input_path).map_err(|e| {
        MediaError::Internal(format!("Failed to open video file: {e}"))
    })?;

    let video_stream = input_ctx
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or_else(|| {
            MediaError::InvalidMedia("No video stream found".into())
        })?;

    let video_stream_index = video_stream.index();
    let codec_params = video_stream.parameters();

    let codec_ctx =
        CodecContext::from_parameters(codec_params).map_err(|e| {
            MediaError::Internal(format!("Failed to create codec context: {e}"))
        })?;
    let mut decoder = codec_ctx.decoder().video().map_err(|e| {
        MediaError::Internal(format!("Failed to create video decoder: {e}"))
    })?;

    let duration = input_ctx.duration();
    if duration > 0 && percentage > 0.0 {
        let target_position = (duration as f64 * percentage) as i64;
        input_ctx.seek(target_position, ..).map_err(|e| {
            MediaError::Internal(format!("Failed to seek: {e}"))
        })?;
    }

    let mut received_frame = None;
    for (stream, packet) in input_ctx.packets() {
        if stream.index() != video_stream_index {
            continue;
        }

        decoder.send_packet(&packet).map_err(|e| {
            MediaError::Internal(format!("Failed to send packet: {e}"))
        })?;

        let mut frame = ffmpeg::frame::Video::empty();
        match decoder.receive_frame(&mut frame) {
            Ok(_) => {
                received_frame = Some(frame);
                break;
            }
            Err(err) => {
                debug!("Skipping packet during thumbnail extraction: {err}");
                continue;
            }
        }
    }

    let frame = received_frame.ok_or_else(|| {
        MediaError::InvalidMedia(
            "Unable to decode frame for thumbnail generation".into(),
        )
    })?;

    let mut scaler = ffmpeg::software::scaling::Context::get(
        decoder.format(),
        decoder.width(),
        decoder.height(),
        ffmpeg::format::Pixel::RGB24,
        decoder.width(),
        decoder.height(),
        ffmpeg::software::scaling::flag::Flags::BILINEAR,
    )
    .map_err(|e| {
        MediaError::Internal(format!("Failed to create scaler: {e}"))
    })?;

    let mut rgb_frame = ffmpeg::frame::Video::empty();
    scaler.run(&frame, &mut rgb_frame).map_err(|e| {
        MediaError::Internal(format!("Failed to scale frame: {e}"))
    })?;

    let width = rgb_frame.width();
    let height = rgb_frame.height();
    let data = rgb_frame.data(0);
    let stride = rgb_frame.stride(0);

    let buffer = image::ImageBuffer::<image::Rgb<u8>, Vec<u8>>::from_fn(
        width,
        height,
        |x, y| {
            let offset = y as usize * stride + (x as usize * 3);
            image::Rgb([data[offset], data[offset + 1], data[offset + 2]])
        },
    );

    buffer
        .save(output_path)
        .map_err(|e| MediaError::Io(std::io::Error::other(e)))
}

#[cfg(not(feature = "ffmpeg"))]
fn extract_frame_at_percentage(
    _input_path: &str,
    _output_path: &Path,
    _percentage: f64,
) -> Result<()> {
    Err(MediaError::Internal(
        "FFmpeg support is required for thumbnail generation".into(),
    ))
}
