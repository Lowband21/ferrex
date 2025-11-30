pub mod tmdb_image_size;

pub use tmdb_image_size::TmdbImageSize;

use crate::{
    database::{
        ports::{images::ImageRepository, media_files::MediaFilesReadPort},
        traits::{ImageLookupParams, ImageRecord, ImageVariant},
    },
    domain::media::image::{
        MediaImageKind,
        records::{MediaImageVariantKey, MediaImageVariantRecord},
    },
    error::{MediaError, Result},
};

use chrono::Utc;
use sha2::{Digest, Sha256};
use std::{
    any::type_name_of_val,
    collections::HashSet,
    fmt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::{Mutex, Notify, Semaphore};
use tracing::{debug, info, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct EnsureReport {
    pub image_id: Option<Uuid>,
    pub ready_path: Option<PathBuf>,
}

#[cfg(feature = "ffmpeg")]
use ffmpeg_next as ffmpeg;
#[cfg(feature = "ffmpeg")]
use once_cell::sync::OnceCell;

#[derive(Clone)]
pub struct ImageService {
    media_files: Arc<dyn MediaFilesReadPort>,
    images: Arc<dyn ImageRepository>,
    cache_dir: PathBuf,
    http_client: reqwest::Client,
    // Non-blocking variant generation coordination
    in_flight: Arc<Mutex<HashSet<String>>>,
    // Per-variant singleflight to avoid duplicate downloads of the same size
    in_flight_variants:
        Arc<Mutex<std::collections::HashMap<String, Arc<Notify>>>>,
    permits: Arc<Semaphore>,
    // Diagnostics: counts of singleflight leaders/waiters for variants
    sf_variant_leaders: Arc<AtomicU64>,
    sf_variant_waiters: Arc<AtomicU64>,
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
            .field(
                "sf_variant_leaders",
                &self.sf_variant_leaders.load(Ordering::Relaxed),
            )
            .field(
                "sf_variant_waiters",
                &self.sf_variant_waiters.load(Ordering::Relaxed),
            )
            .finish()
    }
}

impl ImageService {
    pub fn new(
        media_files: Arc<dyn MediaFilesReadPort>,
        images: Arc<dyn ImageRepository>,
        cache_dir: PathBuf,
    ) -> Self {
        Self::new_with_concurrency(media_files, images, cache_dir, 12)
    }

    pub fn new_with_concurrency(
        media_files: Arc<dyn MediaFilesReadPort>,
        images: Arc<dyn ImageRepository>,
        cache_dir: PathBuf,
        download_concurrency: usize,
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
            in_flight_variants: Arc::new(Mutex::new(
                std::collections::HashMap::new(),
            )),
            // Cap concurrent variant work; configurable via server wiring.
            permits: Arc::new(Semaphore::new(download_concurrency.max(1))),
            sf_variant_leaders: Arc::new(AtomicU64::new(0)),
            sf_variant_waiters: Arc::new(AtomicU64::new(0)),
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

        // Write-once guard: if this variant is already cached, don't overwrite it.
        // This prevents race conditions where a server is serving a file while
        // the scanner tries to overwrite it.
        if let Some(ref key) = context {
            if self.images.is_media_image_variant_cached(key).await? {
                // Check if the file still exists on disk
                if let Some(existing) = self
                    .images
                    .get_image_variant(image_record.id, variant_name)
                    .await?
                {
                    let path = PathBuf::from(&existing.file_path);
                    if path.exists() {
                        debug!(
                            "Variant {} already cached (write-once), returning existing path",
                            variant_name
                        );
                        return Ok(path);
                    }
                }
                // File is missing but DB says cached - auto-invalidate and regenerate
                warn!(
                    "Variant marked cached but file missing, auto-invalidating and regenerating: {:?}/{}",
                    key.image_type, key.variant
                );
                self.images.invalidate_media_image_variant(key).await?;
            }
        }

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

        let response = self
            .http_client
            // Avoid compressed, range-susceptible responses for binary assets
            .get(&url)
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .send()
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Failed to download image: {}", e))
            })?;

        if !response.status().is_success() {
            return Err(MediaError::Internal(format!(
                "Failed to download image: HTTP {}",
                response.status()
            )));
        }

        let expected_len = response.content_length();
        let bytes = response.bytes().await.map_err(|e| {
            MediaError::Internal(format!("Failed to read image data: {}", e))
        })?;

        if let Some(content_len) = expected_len
            && bytes.len() as u64 != content_len
        {
            return Err(MediaError::Internal(format!(
                "Image size mismatch: got {} bytes, expected {}",
                bytes.len(),
                content_len
            )));
        }

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

        // Write atomically: write to a temporary file, fsync, then hard_link.
        // Using hard_link instead of rename ensures we NEVER overwrite an existing file.
        // This is critical for write-once semantics to prevent race conditions where
        // a server is reading a file while the scanner would otherwise overwrite it.
        // Use a unique temp name to avoid collisions when concurrent writers target
        // the same destination path (e.g., orchestrator + on-demand request).
        let tmp_path = file_path
            .with_extension(format!("tmp.{}", uuid::Uuid::new_v4().simple()));
        {
            use tokio::io::AsyncWriteExt;
            let mut f = tokio::fs::File::create(&tmp_path)
                .await
                .map_err(MediaError::Io)?;
            f.write_all(&bytes).await.map_err(MediaError::Io)?;
            // Ensure file contents are durable before linking
            f.sync_all().await.map_err(MediaError::Io)?;
        }
        // Best-effort: fsync the parent directory to persist the link operation
        if let Some(parent) = file_path.parent()
            && let Ok(dir) = tokio::fs::File::open(parent).await
        {
            let _ = dir.sync_all().await; // ignore errors; hard_link below is still atomic
        }
        // Use hard_link to atomically "publish" the file.
        // hard_link fails with AlreadyExists if target exists, preventing overwrites.
        match tokio::fs::hard_link(&tmp_path, &file_path).await {
            Ok(_) => {
                // Successfully linked; remove the temp file name (inode is now at final path)
                let _ = tokio::fs::remove_file(&tmp_path).await;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // File already exists - another writer won the race or write-once guard was bypassed
                // This is fine; discard our temp file and use the existing one.
                let _ = tokio::fs::remove_file(&tmp_path).await;
                debug!("File already exists (hard_link check): {:?}", file_path);
            }
            Err(e) => {
                // Clean up temp file on error
                let _ = tokio::fs::remove_file(&tmp_path).await;
                return Err(MediaError::Io(e));
            }
        }

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
        // Per-variant singleflight to avoid duplicate thumbnail generation
        let vkey =
            format!("{}:{}", image_record.id.as_hyphenated(), &variant_name);
        let (is_leader, notify) = self.subscribe_variant(&vkey).await;
        if !is_leader {
            notify.notified().await;
            if let Some(existing) = self
                .images
                .get_image_variant(image_record.id, &variant_name)
                .await?
            {
                return Ok(PathBuf::from(existing.file_path));
            } else {
                return Err(MediaError::Internal(
                    "thumbnail singleflight finished but variant missing"
                        .into(),
                ));
            }
        }

        let variant_dir = self.cache_dir.join(image_folder).join(&variant_name);
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
        let gen_result = tokio::task::spawn_blocking(move || {
            extract_frame_at_percentage(&video_path_string, &output_path, 0.3)
        })
        .await
        .map_err(|err| {
            MediaError::Internal(format!("Failed to join ffmpeg task: {err}"))
        })?;
        if let Err(e) = gen_result {
            // Notify waiters on failure to avoid indefinite waits
            self.complete_variant(&vkey).await;
            return Err(e);
        }
        // Signal completion after successful extraction; DB rows will follow.
        self.complete_variant(&vkey).await;

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

                // Notify any waiters that thumbnail is ready
                self.complete_variant(&vkey).await;
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

        // Notify any waiters that thumbnail is ready
        self.complete_variant(&vkey).await;

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
    /// - ready_path: present if a variant is already cached and ready.
    /// - image_id: present if the image record exists in the database.
    pub async fn ensure_variant_async(
        &self,
        params: &ImageLookupParams,
    ) -> Result<EnsureReport> {
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
            // Choose a canonical first variant to fetch for this image type so
            // different requested sizes coalesce on the same download.
            let requested_variant = params.variant.as_deref();
            let canonical_size =
                select_canonical_size(&params.image_type, requested_variant);
            let canonical_variant_str = canonical_size.as_str();
            let variant_key_struct =
                build_variant_key(params, canonical_variant_str);

            if let Some(variant) = existing_variant {
                // Variant already cached: avoid hot-path DB writes.
                // We deliberately skip upserting media_image_variants here to keep
                // GET fast; cache bookkeeping occurs when downloads complete.
                return Ok(EnsureReport {
                    image_id: Some(image_record.id),
                    ready_path: Some(PathBuf::from(variant.file_path)),
                });
            }

            // Missing: spawn background download for requested size if it maps to TMDB size
            if let Some(size_str) = requested_variant
                && let Some(_size) = TmdbImageSize::from_str(size_str)
            {
                // Per-variant singleflight key (same image, same TMDB size)
                let vkey = format!(
                    "{}:{}",
                    image_record.id.as_hyphenated(),
                    canonical_variant_str
                );
                let (is_leader, _notify) = self.subscribe_variant(&vkey).await;
                if !is_leader {
                    debug!(
                        "Variant {} already in-flight for image {}, skipping background ensure",
                        canonical_variant_str, image_record.id
                    );
                    return Ok(EnsureReport {
                        image_id: Some(image_record.id),
                        ready_path: None,
                    });
                }
                let key = self.image_key(params);
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
                    let vkey2 = vkey.clone();
                    tokio::spawn(async move {
                        let _permit =
                            permits.acquire().await.expect("semaphore");
                        let download_result = this
                            .download_variant(
                                &tmdb_path,
                                canonical_size,
                                key_struct.clone(),
                            )
                            .await;
                        if let Err(e) = download_result {
                            warn!(
                                "Background variant download failed for {} {}: {}",
                                tmdb_path,
                                canonical_size.as_str(),
                                e
                            );
                        }
                        this.complete_variant(&vkey2).await;
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
            return Ok(EnsureReport {
                image_id: Some(image_record.id),
                ready_path: None,
            });
        }

        Ok(EnsureReport {
            image_id: None,
            ready_path: None,
        })
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

    /// Pick best available variant given a known image_id, avoiding an extra lookup.
    pub async fn pick_best_available_for_image(
        &self,
        image_id: Uuid,
        target_width: Option<u32>,
        requested_variant: Option<&str>,
    ) -> Result<Option<(PathBuf, String)>> {
        let mut variants = self.images.get_image_variants(image_id).await?;
        if variants.is_empty() {
            return Ok(None);
        }

        let target_w = target_width
            .or_else(|| requested_variant.and_then(tmdb_variant_to_width_hint))
            .unwrap_or(500);

        variants.sort_by_key(|v| {
            tmdb_variant_to_width_hint(&v.variant).unwrap_or(u32::MAX)
        });

        let mut best: Option<&ImageVariant> = None;
        for v in variants.iter().rev() {
            if let Some(w) = tmdb_variant_to_width_hint(&v.variant)
                && w <= target_w
            {
                best = Some(v);
                break;
            }
        }
        if best.is_none() {
            for v in variants.iter() {
                if let Some(w) = tmdb_variant_to_width_hint(&v.variant) {
                    if w >= target_w {
                        best = Some(v);
                        break;
                    }
                } else {
                    best = Some(v);
                    break;
                }
            }
        }

        if let Some(v) = best {
            return Ok(Some((PathBuf::from(&v.file_path), v.variant.clone())));
        }
        Ok(None)
    }

    fn image_key(&self, params: &ImageLookupParams) -> String {
        // Dedupe in-flight work by image identity only (ignore variant)
        format!(
            "{}:{}:{}:{}",
            params.media_type,
            params.media_id,
            params.image_type.as_str(),
            params.index,
        )
    }

    async fn subscribe_variant(&self, vkey: &str) -> (bool, Arc<Notify>) {
        let mut map = self.in_flight_variants.lock().await;
        if let Some(n) = map.get(vkey) {
            let waiters =
                self.sf_variant_waiters.fetch_add(1, Ordering::Relaxed) + 1;
            let leaders = self.sf_variant_leaders.load(Ordering::Relaxed);
            debug!(
                "singleflight-variant wait: key={}, leaders={}, waiters={}",
                vkey, leaders, waiters
            );
            return (false, Arc::clone(n));
        }
        let notify = Arc::new(Notify::new());
        map.insert(vkey.to_string(), Arc::clone(&notify));
        let leaders =
            self.sf_variant_leaders.fetch_add(1, Ordering::Relaxed) + 1;
        let waiters = self.sf_variant_waiters.load(Ordering::Relaxed);
        debug!(
            "singleflight-variant lead: key={}, leaders={}, waiters={}",
            vkey, leaders, waiters
        );
        (true, notify)
    }

    async fn complete_variant(&self, vkey: &str) {
        let notify = {
            let mut map = self.in_flight_variants.lock().await;
            map.remove(vkey)
        };
        if let Some(n) = notify {
            n.notify_waiters();
            debug!("singleflight-variant complete: key={}", vkey);
        }
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
                // If present in DB, verify on-disk path exists and is in the expected structure.
                if let Some(existing) = self
                    .images
                    .get_image_variant(image_record.id, size_str)
                    .await?
                {
                    let existing_path = PathBuf::from(&existing.file_path);
                    let image_folder = image_type_folder(&params.image_type);
                    let variant_dir_ok = existing_path
                        .parent()
                        .map(|dir| dir.ends_with(size_str))
                        .unwrap_or(false);
                    let type_dir_ok = existing_path
                        .parent()
                        .and_then(|dir| dir.parent())
                        .map(|dir| dir.ends_with(image_folder))
                        .unwrap_or(false);

                    if existing_path.exists() && variant_dir_ok && type_dir_ok {
                        // Happy path: cached file is present and in the right place.
                        return Ok(Some(existing_path));
                    } else {
                        // Stale or legacy path: heal by re-downloading into the canonical location.
                        warn!(
                            "Variant record exists but file missing or legacy path, re-downloading: {}",
                            existing.file_path
                        );
                    }
                }

                // Per-variant singleflight: serialize concurrent downloads of the same variant
                let vkey =
                    format!("{}:{}", image_record.id.as_hyphenated(), size_str);
                let (is_leader, notify) = self.subscribe_variant(&vkey).await;
                if !is_leader {
                    notify.notified().await;
                    if let Some(v) = self
                        .images
                        .get_image_variant(image_record.id, size_str)
                        .await?
                    {
                        return Ok(Some(PathBuf::from(v.file_path)));
                    } else {
                        return Ok(None);
                    }
                }

                let key_struct = build_variant_key(params, size_str);
                let result = self
                    .download_variant(&image_record.tmdb_path, size, key_struct)
                    .await;
                self.complete_variant(&vkey).await;
                let path = result?;
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

    /// Invalidate a cached variant, allowing it to be re-downloaded.
    /// Also removes the file from disk if it exists.
    pub async fn invalidate_variant(
        &self,
        key: &MediaImageVariantKey,
    ) -> Result<()> {
        // Look up the file path before invalidating
        let params = ImageLookupParams {
            media_type: key.media_type.clone(),
            media_id: key.media_id.to_string(),
            image_type: key.image_type.clone(),
            index: key.order_index as u32,
            variant: Some(key.variant.clone()),
        };

        if let Some((image_record, _)) =
            self.images.lookup_image_variant(&params).await?
        {
            if let Some(variant) = self
                .images
                .get_image_variant(image_record.id, &key.variant)
                .await?
            {
                let file_path = PathBuf::from(&variant.file_path);
                // Delete file from disk
                if file_path.exists() {
                    tokio::fs::remove_file(&file_path)
                        .await
                        .map_err(MediaError::Io)?;
                    info!("Removed cached file for invalidation: {:?}", file_path);
                }
            }
        }

        // Mark as uncached in DB
        self.images.invalidate_media_image_variant(key).await?;
        info!("Invalidated variant cache: {:?}/{}", key.image_type, key.variant);

        Ok(())
    }

    /// Invalidate all cached variants for a media item.
    /// Also removes the files from disk.
    /// Returns the number of variants invalidated.
    pub async fn invalidate_all_variants(
        &self,
        media_type: &str,
        media_id: Uuid,
    ) -> Result<u32> {
        // Get all variants for this media item
        let variants = self
            .images
            .list_media_image_variants(media_type, media_id)
            .await?;

        let mut removed_count = 0u32;
        for record in &variants {
            if !record.cached {
                continue;
            }

            // Look up file path
            let params = ImageLookupParams {
                media_type: record.key.media_type.clone(),
                media_id: record.key.media_id.to_string(),
                image_type: record.key.image_type.clone(),
                index: record.key.order_index as u32,
                variant: Some(record.key.variant.clone()),
            };

            if let Some((image_record, _)) =
                self.images.lookup_image_variant(&params).await?
            {
                if let Some(variant) = self
                    .images
                    .get_image_variant(image_record.id, &record.key.variant)
                    .await?
                {
                    let file_path = PathBuf::from(&variant.file_path);
                    if file_path.exists() {
                        if let Err(e) = tokio::fs::remove_file(&file_path).await {
                            warn!("Failed to remove cached file {:?}: {}", file_path, e);
                        } else {
                            removed_count += 1;
                        }
                    }
                }
            }
        }

        // Mark all as uncached in DB
        let db_count = self
            .images
            .invalidate_all_media_image_variants(media_type, media_id)
            .await?;

        info!(
            "Invalidated {} variants for {}/{} ({} files removed)",
            db_count, media_type, media_id, removed_count
        );

        Ok(db_count)
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

fn select_canonical_size(
    kind: &MediaImageKind,
    requested_variant: Option<&str>,
) -> TmdbImageSize {
    match kind {
        MediaImageKind::Poster => TmdbImageSize::PosterW342,
        // Backdrops are displayed large; prefer original to avoid detail loss.
        MediaImageKind::Backdrop => TmdbImageSize::Original,
        MediaImageKind::Thumbnail => TmdbImageSize::StillW300,
        MediaImageKind::Logo => TmdbImageSize::Original,
        MediaImageKind::Cast => TmdbImageSize::ProfileW185,
        MediaImageKind::Other(_) => {
            // Fall back to requested if parsable; else original
            requested_variant
                .and_then(TmdbImageSize::from_str)
                .unwrap_or(TmdbImageSize::Original)
        }
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

    // Encode as JPEG and write atomically: write to temp file, fsync, then rename
    atomic_write_jpeg_rgb8(output_path, width, height, buffer.into_raw())
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

/// Atomically write an RGB8 image as JPEG to the given output path.
///
/// Strategy:
/// - Write the encoded JPEG to a sibling temp file (same directory, .tmp extension)
/// - fsync the temp file to ensure contents are durable
/// - Rename the temp file over the destination path (atomic on POSIX filesystems)
/// - Best-effort fsync the parent directory to persist the rename metadata
fn atomic_write_jpeg_rgb8(
    output_path: &Path,
    width: u32,
    height: u32,
    rgb_bytes: Vec<u8>,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use image::ColorType;
    use image::codecs::jpeg::JpegEncoder;
    use std::fs::File;
    use std::io::Write;

    // Ensure parent directory exists
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp_path = output_path
        .with_extension(format!("tmp.{}", uuid::Uuid::new_v4().simple()));
    {
        let mut file = File::create(&tmp_path)?;
        let mut encoder = JpegEncoder::new_with_quality(&mut file, 85);
        encoder.encode(&rgb_bytes, width, height, ColorType::Rgb8.into())?;
        file.flush()?;
        file.sync_all()?;
    }

    match std::fs::rename(&tmp_path, output_path) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // Another concurrent writer won; remove our temp and proceed.
            let _ = std::fs::remove_file(&tmp_path);
        }
        Err(e) => return Err(Box::new(e)),
    }

    // Best-effort fsync of parent directory to persist rename metadata
    if let Some(parent) = output_path.parent()
        && let Ok(dir) = File::open(parent)
    {
        let _ = dir.sync_all();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::atomic_write_jpeg_rgb8;

    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::{Duration, Instant};

    #[test]
    fn atomic_jpeg_write_does_not_expose_partial_file() {
        // Large-ish buffer to make encoding take noticeable time
        let width = 2000u32;
        let height = 2000u32;
        let mut rgb = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                // simple gradient pattern
                rgb.push((x % 256) as u8);
                rgb.push((y % 256) as u8);
                rgb.push(((x + y) % 256) as u8);
            }
        }

        let dir = tempfile::tempdir().expect("tmpdir");
        let out = dir.path().join("test_atomic.jpg");
        let tmp = out.with_extension("tmp");

        // Marker for completion
        let done = Arc::new(AtomicBool::new(false));
        let done_w = done.clone();

        // Start writer on a separate thread
        let out_w = out.clone();
        let rgb_w = rgb.clone();
        let handle = std::thread::spawn(move || {
            atomic_write_jpeg_rgb8(&out_w, width, height, rgb_w)
                .expect("atomic write ok");
            done_w.store(true, Ordering::SeqCst);
        });

        // While the writer is running, the final file should not be visible
        // before the atomic rename. There is, however, a tiny window where the
        // rename may complete before the `done` flag is observed as true on
        // this thread. To avoid a racy assertion, break as soon as the final
        // path appears (indicating the rename has happened) or the writer is
        // marked done.
        let start = Instant::now();
        while !done.load(Ordering::SeqCst) {
            if out.exists() {
                // Final path appeared, meaning rename completed; validate it isn't partial.
                let data =
                    std::fs::read(&out).expect("read final during writer");
                assert!(!data.is_empty(), "jpeg not empty during writer");
                let img = image::load_from_memory(&data)
                    .expect("decode final jpeg during writer");
                assert_eq!(img.width(), width);
                assert_eq!(img.height(), height);
                // Stop polling once validated.
                break;
            }
            // tmp file may appear during write; don't assert either way
            if start.elapsed() > Duration::from_secs(5) {
                break; // encoding might be fast in CI; avoid spinning too long
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        // Join and verify results
        handle.join().expect("writer thread");
        assert!(out.exists(), "final file should exist after write");
        assert!(!tmp.exists(), "temp file should be gone after rename");

        // Basic sanity: resulting file is a JPEG with non-zero size
        let meta = std::fs::metadata(&out).expect("metadata");
        assert!(meta.len() > 0, "jpeg size should be > 0");
    }

    #[test]
    fn concurrent_atomic_writes_to_same_output_do_not_corrupt() {
        // Prepare two distinct RGB buffers (different colors/patterns)
        let width = 800u32;
        let height = 600u32;

        let mut rgb_a = vec![0u8; (width * height * 3) as usize];
        for i in (0..rgb_a.len()).step_by(3) {
            rgb_a[i] = 255; // R
            rgb_a[i + 1] = 0; // G
            rgb_a[i + 2] = 0; // B
        }

        let mut rgb_b = vec![0u8; (width * height * 3) as usize];
        for i in (0..rgb_b.len()).step_by(3) {
            rgb_b[i] = 0; // R
            rgb_b[i + 1] = 0; // G
            rgb_b[i + 2] = 255; // B
        }

        let dir = tempfile::tempdir().expect("tmpdir");
        let out = dir.path().join("race.jpg");

        // Launch two writers concurrently to the same destination
        let out_a = out.clone();
        let handle_a = std::thread::spawn(move || {
            atomic_write_jpeg_rgb8(&out_a, width, height, rgb_a)
                .expect("atomic write A ok");
        });

        let out_b = out.clone();
        let handle_b = std::thread::spawn(move || {
            atomic_write_jpeg_rgb8(&out_b, width, height, rgb_b)
                .expect("atomic write B ok");
        });

        handle_a.join().expect("join A");
        handle_b.join().expect("join B");

        // Final file must exist and be decodable as JPEG
        assert!(out.exists(), "final image should exist");

        let data = std::fs::read(&out).expect("read");
        assert!(!data.is_empty(), "jpeg not empty");

        // Decode image; this fails if the file is corrupt/partial
        let img = image::load_from_memory(&data).expect("decode jpeg");
        assert_eq!(img.width(), width);
        assert_eq!(img.height(), height);
    }
}
