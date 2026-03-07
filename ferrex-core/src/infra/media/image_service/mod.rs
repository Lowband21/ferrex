use crate::{
    database::{
        repository_ports::{
            images::{ImageRepository, ImgDbLookup, ImgInput},
            media_files::MediaFilesReadPort,
        },
        traits::{ImageRecord, OriginalImage},
    },
    error::{MediaError, Result},
    infra::cache::{
        CachedImageBlobMeta, ImageBlobStore, ImageCacheRoot, ImageFileStore,
        image_cache_key_for,
    },
};

use ferrex_model::ImageReadyEvent;
use ferrex_model::{
    ImageSize,
    image::{ImageDimensions, ImageVariant},
};

#[cfg(not(feature = "demo"))]
use ferrex_model::{
    MediaID,
    media_type::{ImageMediaType, VideoMediaType},
};
use std::{
    any::type_name_of_val,
    collections::HashSet,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, Notify, Semaphore, broadcast, mpsc};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

#[cfg(feature = "ffmpeg")]
use ffmpeg_next as ffmpeg;
#[cfg(feature = "ffmpeg")]
use once_cell::sync::OnceCell;

#[derive(Clone)]
pub struct ImageService {
    media_files: Arc<dyn MediaFilesReadPort>,
    pub(crate) images: Arc<dyn ImageRepository>,
    blob_store: ImageBlobStore,
    file_store: ImageFileStore,
    http_client: reqwest::Client,
    /// Non-blocking cache fill coordination (server can enqueue without awaiting).
    in_flight: Arc<std::sync::Mutex<HashSet<String>>>,
    cache_fill_tx: mpsc::Sender<CacheFillJob>,
    // Per-variant singleflight to avoid duplicate downloads of the same size
    in_flight_variants:
        Arc<Mutex<std::collections::HashMap<String, Arc<Notify>>>>,
    permits: Arc<Semaphore>,
    // Diagnostics: counts of singleflight leaders/waiters for variants
    sf_variant_leaders: Arc<AtomicU64>,
    sf_variant_waiters: Arc<AtomicU64>,
    // Diagnostics: cache-fill queue pressure
    cache_fill_enqueued: Arc<AtomicU64>,
    cache_fill_dropped: Arc<AtomicU64>,
    image_events: broadcast::Sender<ImageReadyEvent>,
}

#[derive(Debug, Clone)]
struct CacheFillJob {
    key: String,
    iid: Uuid,
    imz: ImageSize,
    policy: CachePolicy,
}

impl fmt::Debug for ImageService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let in_flight = self
            .in_flight
            .lock()
            .ok()
            .map(|guard| guard.len())
            .unwrap_or(0);

        f.debug_struct("ImageService")
            .field(
                "media_files_repository",
                &type_name_of_val(self.media_files.as_ref()),
            )
            .field("image_repository", &type_name_of_val(self.images.as_ref()))
            .field("image_cache_root", &self.blob_store.root())
            .field("image_blob_root", &self.file_store.root())
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
            .field(
                "cache_fill_enqueued",
                &self.cache_fill_enqueued.load(Ordering::Relaxed),
            )
            .field(
                "cache_fill_dropped",
                &self.cache_fill_dropped.load(Ordering::Relaxed),
            )
            .finish()
    }
}

impl ImageService {
    pub fn new(
        media_files: Arc<dyn MediaFilesReadPort>,
        images: Arc<dyn ImageRepository>,
        image_cache_dir: std::path::PathBuf,
    ) -> Self {
        Self::new_with_concurrency(media_files, images, image_cache_dir, 12)
    }

    pub fn new_with_concurrency(
        media_files: Arc<dyn MediaFilesReadPort>,
        images: Arc<dyn ImageRepository>,
        image_cache_dir: std::path::PathBuf,
        download_concurrency: usize,
    ) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .no_deflate()
            .no_zstd()
            .no_brotli()
            .no_gzip()
            .build()
            .expect("Failed to create HTTP client");

        // Ensure cache_dir is absolute so the cache root is stable.
        let image_cache_dir = if image_cache_dir.is_absolute() {
            image_cache_dir
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join(image_cache_dir)
        };

        // Materialized, immutable blobs live alongside the `cacache` root in a dedicated subdir.
        let image_blob_dir = image_cache_dir.join("blobs-v2");
        if let Err(err) = std::fs::create_dir_all(&image_blob_dir) {
            warn!(
                "Failed to create image blob dir {:?}: {}",
                image_blob_dir, err
            );
        }

        let (image_events, _) = broadcast::channel::<ImageReadyEvent>(4096);

        let cache_fill_queue_size =
            std::env::var("IMAGE_CACHE_FILL_QUEUE_SIZE")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(4096)
                .max(1);
        let default_cache_fill_concurrency = {
            let db_max = std::env::var("DB_MAX_CONNECTIONS")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(100)
                .max(1);
            // Leave room for request traffic + other subsystems by keeping image fill
            // to a conservative fraction of the DB pool.
            let db_budget = (db_max / 4).max(1);
            std::cmp::min(download_concurrency.max(1), db_budget)
        };
        let cache_fill_concurrency =
            std::env::var("IMAGE_CACHE_FILL_CONCURRENCY")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(default_cache_fill_concurrency)
                .max(1);
        let cache_fill_max_retries =
            std::env::var("IMAGE_CACHE_FILL_MAX_RETRIES")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(5);

        let (cache_fill_tx, cache_fill_rx) =
            mpsc::channel::<CacheFillJob>(cache_fill_queue_size);

        let svc = Self {
            media_files,
            images,
            blob_store: ImageBlobStore::new(ImageCacheRoot::new(
                image_cache_dir,
            )),
            file_store: ImageFileStore::new(image_blob_dir),
            http_client,
            in_flight: Arc::new(std::sync::Mutex::new(HashSet::new())),
            cache_fill_tx,
            in_flight_variants: Arc::new(Mutex::new(
                std::collections::HashMap::new(),
            )),
            // Cap concurrent variant work; configurable via server wiring.
            permits: Arc::new(Semaphore::new(download_concurrency.max(1))),
            sf_variant_leaders: Arc::new(AtomicU64::new(0)),
            sf_variant_waiters: Arc::new(AtomicU64::new(0)),
            cache_fill_enqueued: Arc::new(AtomicU64::new(0)),
            cache_fill_dropped: Arc::new(AtomicU64::new(0)),
            image_events,
        };

        svc.start_cache_fill_workers(
            cache_fill_rx,
            cache_fill_concurrency,
            cache_fill_max_retries,
        );
        info!(
            "Image cache-fill queue initialized: workers={}, queue_size={}, max_retries={}",
            cache_fill_concurrency,
            cache_fill_queue_size,
            cache_fill_max_retries
        );

        svc
    }

    pub fn subscribe_image_events(
        &self,
    ) -> broadcast::Receiver<ImageReadyEvent> {
        self.image_events.subscribe()
    }

    pub fn image_blob_path(&self, token: &str) -> Result<std::path::PathBuf> {
        self.file_store.path_for_token(token)
    }

    fn start_cache_fill_workers(
        &self,
        rx: mpsc::Receiver<CacheFillJob>,
        concurrency: usize,
        max_retries: usize,
    ) {
        if tokio::runtime::Handle::try_current().is_err() {
            warn!(
                "Image cache-fill workers not started (no Tokio runtime available)"
            );
            return;
        }

        let rx = Arc::new(Mutex::new(rx));
        for worker_id in 0..concurrency {
            let svc = self.clone();
            let rx = Arc::clone(&rx);
            tokio::spawn(async move {
                loop {
                    let job = {
                        let mut guard = rx.lock().await;
                        guard.recv().await
                    };
                    let Some(job) = job else { break };
                    svc.run_cache_fill_job(job, worker_id, max_retries).await;
                }
            });
        }
    }

    /// Enqueue a best-effort cache fill for `iid + imz` without blocking the caller.
    ///
    /// This is intended for request handlers. It dedupes in-flight work so repeated
    /// requests do not spawn unbounded background tasks.
    pub fn enqueue_cache(
        &self,
        iid: Uuid,
        imz: ImageSize,
        policy: CachePolicy,
    ) {
        let key = format!(
            "fill:{}:{}:{}",
            iid.as_hyphenated(),
            imz.image_variant(),
            imz.to_tmdb_param()
        );

        if !self.try_begin_enqueue(&key) {
            return;
        }

        let job = CacheFillJob {
            key: key.clone(),
            iid,
            imz,
            policy,
        };

        match self.cache_fill_tx.try_send(job) {
            Ok(()) => {
                self.cache_fill_enqueued.fetch_add(1, Ordering::Relaxed);
            }
            Err(err) => {
                self.cache_fill_dropped.fetch_add(1, Ordering::Relaxed);
                warn!(
                    "[enqueue_cache_fill] Dropped cache fill (queue full/closed): iid={}, imz={:?}, err={}",
                    iid, imz, err
                );
                self.finish_enqueue(&key);
            }
        }
    }

    fn try_begin_enqueue(&self, key: &str) -> bool {
        let Ok(mut set) = self.in_flight.lock() else {
            return false;
        };
        if set.contains(key) {
            return false;
        }
        set.insert(key.to_string());
        true
    }

    fn finish_enqueue(&self, key: &str) {
        if let Ok(mut set) = self.in_flight.lock() {
            set.remove(key);
        }
    }

    async fn run_cache_fill_job(
        &self,
        job: CacheFillJob,
        worker_id: usize,
        max_retries: usize,
    ) {
        let CacheFillJob {
            key,
            iid,
            imz,
            policy,
        } = job;

        let mut attempt = 0usize;
        let mut backoff = Duration::from_millis(200);
        let max_backoff = Duration::from_secs(5);

        loop {
            let res = self.cache_fill_ensure_ready(iid, imz, policy).await;
            match res {
                Ok(_) => break,
                Err(err)
                    if attempt < max_retries
                        && is_retryable_cache_fill_error(&err) =>
                {
                    attempt += 1;
                    warn!(
                        "[enqueue_cache_fill] Cache fill retrying (attempt {}/{}): worker={}, iid={}, imz={:?}, err={}",
                        attempt, max_retries, worker_id, iid, imz, err
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = std::cmp::min(backoff * 2, max_backoff);
                }
                Err(err) => {
                    warn!(
                        "[enqueue_cache_fill] Background cache fill failed: worker={}, iid={}, imz={:?}, err={}",
                        worker_id, iid, imz, err
                    );
                    break;
                }
            }
        }

        self.finish_enqueue(&key);
    }

    async fn cache_fill_ensure_ready(
        &self,
        iid: Uuid,
        imz: ImageSize,
        policy: CachePolicy,
    ) -> Result<()> {
        let record = self.cached_image(iid, imz, policy).await?;

        match self.ensure_materialized_and_emit(&record).await {
            Ok(()) => Ok(()),
            Err(err)
                if policy == CachePolicy::Ensure
                    && matches!(
                        err,
                        MediaError::NotFound(_) | MediaError::InvalidMedia(_)
                    ) =>
            {
                warn!(
                    "[cache_fill] Cache entry missing/corrupt after DB hit; attempting refresh: iid={}, imz={:?}, err={}",
                    iid, record.imz, err
                );
                let refreshed = self
                    .cached_image(iid, record.imz, CachePolicy::Refresh)
                    .await?;
                self.ensure_materialized_and_emit(&refreshed).await
            }
            Err(err) => Err(err),
        }
    }

    async fn ensure_materialized_and_emit(
        &self,
        record: &ImageRecord,
    ) -> Result<()> {
        let token = ImageFileStore::token_from_integrity(&record.integrity);

        if !self.file_store.exists(&token).await? {
            let bytes = self
                .read_cached_bytes_by_key(record.iid, record.imz)
                .await?;
            self.file_store.write_if_missing(&token, &bytes).await?;
        }

        let _ = self.image_events.send(ImageReadyEvent {
            iid: record.iid,
            imz: record.imz,
            token,
        });
        Ok(())
    }

    /// Read cached image bytes for a database record.
    ///
    /// `cacache` validates integrity on read and will return an error if the entry
    /// is missing or corrupted.
    pub async fn read_cached_bytes(
        &self,
        record: &ImageRecord,
    ) -> Result<Vec<u8>> {
        self.read_cached_bytes_by_key(record.iid, record.imz).await
    }

    /// Read cached image bytes directly by `(iid, imz)` without involving the DB.
    pub async fn read_cached_bytes_by_key(
        &self,
        iid: Uuid,
        imz: ImageSize,
    ) -> Result<Vec<u8>> {
        let key = image_cache_key_for(iid, imz);
        self.blob_store.read(&key).await
    }

    /// Read cached image metadata directly by `(iid, imz)` without involving the DB.
    pub async fn read_cached_meta_by_key(
        &self,
        iid: Uuid,
        imz: ImageSize,
    ) -> Result<Option<CachedImageBlobMeta>> {
        let key = image_cache_key_for(iid, imz);
        self.blob_store.metadata(&key).await
    }

    /// Download and cache an image variant, keeping metadata in sync with the media_variant table.
    pub async fn download_variant(
        &self,
        iin: ImgInput<'_>,
    ) -> Result<ImageRecord> {
        let tmdb_path = iin.tmdb_path.ok_or(MediaError::Internal(
            "Failed to download, tmdb_path must be passed".to_string(),
        ))?;

        let _permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore should not be closed");

        // Lookup context for this specific variant.
        let lup = ImgDbLookup {
            imz: iin.imz,
            iid: Some(iin.iid),
            media_id: iin.media_id,
            media_type: iin.media_type,
            tmdb_path: Some(tmdb_path),
            lang: None,
        };

        let url = format!(
            "https://image.tmdb.org/t/p/{}{}",
            iin.imz.to_tmdb_param(),
            tmdb_path
        );

        info!(
            "[download_variant] Fetching image from tmdb for iid={}, media_type={:?}, media_id={:?}, imz={:?}, width={:?}, url={}",
            iin.iid,
            iin.media_type,
            iin.media_id,
            iin.imz,
            iin.imz.width(),
            url
        );

        // .header(reqwest::header::ACCEPT_ENCODING, "identity")

        let request = self
            .http_client
            // Avoid compressed, range-susceptible responses for binary assets
            .get(&url)
            .header(reqwest::header::USER_AGENT, "Mozilla/5.0")
            .build()
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to build reqwest Request for image: {}",
                    e
                ))
            })?;

        info!("Reqwest Headers: {:#?}", request.headers());

        let response = self.http_client.execute(request).await?;

        if !response.status().is_success() {
            return Err(MediaError::HttpStatus {
                status: response.status(),
                url,
            });
        }

        let expected_len = response.content_length();
        let bytes = response.bytes().await?;

        if let Some(content_len) = expected_len
            && bytes.len() as u64 != content_len
        {
            return Err(MediaError::Internal(format!(
                "Image size mismatch: got {} bytes, expected {}",
                bytes.len(),
                content_len
            )));
        }

        // Gather metadata from the freshly downloaded bytes.
        let (width, height) = self.get_image_dimensions(&bytes)?;
        let decoded_dimensions = ImageDimensions::try_from((width, height))
            .map_err(|err| {
                MediaError::InvalidMedia(format!(
                    "Decoded image has invalid dimensions {width}x{height}: {err:?}"
                ))
            })?;
        info!(
            "[download_variant] Decoded image dimensions: {}x{}",
            width, height
        );

        let cache_key = image_cache_key_for(iin.iid, iin.imz);
        let stored = self.blob_store.write(&cache_key, &bytes).await?;
        let integrity_string = stored.integrity.to_string();
        let token = ImageFileStore::token_from_integrity(&integrity_string);
        self.file_store
            .write_if_missing(&token, bytes.as_ref())
            .await?;

        let theme_color =
            if matches!(lup.imz.image_variant(), ImageVariant::Poster) {
                self.extract_theme_color(&bytes)
            } else {
                None
            };

        // Use owned metadata so we can safely build an ImgInput with &str fields.
        struct OwnedImgMeta {
            tmdb_path: String,
            cache_key: String,
            integrity: String,
            theme_color: Option<String>,
        }

        let owned = OwnedImgMeta {
            tmdb_path: tmdb_path.to_string(),
            cache_key: cache_key.to_string(),
            integrity: integrity_string,
            theme_color,
        };

        let ctx = ImgInput {
            iid: iin.iid,
            media_id: iin.media_id,
            media_type: iin.media_type,
            tmdb_path: Some(&owned.tmdb_path),
            imz: iin.imz,
            decoded_dimensions: Some(decoded_dimensions),
            theme_color: owned.theme_color.as_deref(),
            cache_key: &owned.cache_key,
            integrity: &owned.integrity,
            byte_len: stored.byte_len as i32,
        };

        info!(
            "[download_variant] Prepared upsert context: iid={}, media_type={:?}, media_id={:?}, imz={:?}, width={:?}, tmdb_path={}, cache_key={}, integrity={}, byte_len={}",
            ctx.iid,
            ctx.media_type,
            ctx.media_id,
            ctx.imz,
            ctx.imz.width(),
            tmdb_path,
            ctx.cache_key,
            ctx.integrity,
            ctx.byte_len
        );

        let record = self.images.upsert_image(&ctx).await?;
        let _ = self.image_events.send(ImageReadyEvent {
            iid: record.iid,
            imz: record.imz,
            token,
        });
        Ok(record)
    }

    pub async fn generate_episode_thumbnail(
        &self,
        media_file_id: Uuid,
        iid: Uuid,
        imz: ImageSize,
    ) -> Result<ImageRecord> {
        self.ensure_ffmpeg_initialized()?;

        let (target_w, target_h) =
            imz.dimensions().ok_or_else(|| MediaError::Internal(
                "Episode thumbnail generation requires an ImageSize with explicit dimensions".to_string(),
            ))?;

        // let image_record = self.register_tmdb_image(image_key).await?;
        // let imz = key.imz.clone();

        // let lup = ImgDbLookup {
        //     iid: Some(params.iid),
        //     imz: params.imz,
        //     media_id: None,
        //     media_type: None,
        //     tmdb_path: None,
        //     lang: None,
        // };

        // if let Some(existing) =
        //     self.images.lookup_cached_image(params.iid, params.imz).await?
        // {
        //     let cache_key = existing.cache_key;
        //     // let variant_dir_ok = existing_path
        //     //     .parent()
        //     //     .map(|dir| dir.ends_with(imz.to_tmdb_param()))
        //     //     .unwrap_or(false);
        //     // let type_dir_ok = existing_path
        //     //     .parent()
        //     //     .and_then(|dir| dir.parent())
        //     //     .map(|dir| dir.ends_with(imz.image_type()))
        //     //     .unwrap_or(false);

        //     if existing_path.exists() && variant_dir_ok && type_dir_ok {
        //         let mut width = existing.width;
        //         let mut height = existing.height;
        //         let mut content_hash: Option<String> = None;

        //         if let Ok(bytes) = tokio::fs::read(&existing_path).await {
        //             content_hash = Some(self.calculate_hash(&bytes));
        //             if (width.is_none() || height.is_none())
        //                 && let Ok((w, h)) = self.get_image_dimensions(&bytes)
        //             {
        //                 width = Some(w as i32);
        //                 height = Some(h as i32);
        //             }
        //         }

        //         self.images
        //             .mark_media_image_variant_cached(
        //                 &params,
        //                 width,
        //                 height,
        //                 content_hash.as_deref(),
        //                 None,
        //             )
        //             .await?;

        //         return Ok(existing_path);
        //     }
        // }

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
        let video_path_string = video_path.to_string_lossy().to_string();
        let (src_w, src_h, encoded_jpeg) =
            tokio::task::spawn_blocking(move || {
                let (src_w, src_h, rgb_bytes) =
                    extract_frame_at_percentage(&video_path_string, 0.3)?;
                let encoded_jpeg = encode_thumbnail_jpeg_rgb24(
                    src_w, src_h, rgb_bytes, target_w, target_h, 85,
                )?;
                Ok::<_, MediaError>((src_w, src_h, encoded_jpeg))
            })
            .await
            .map_err(|err| {
                MediaError::Internal(format!(
                    "Failed to join ffmpeg task: {err}"
                ))
            })??;

        // let expected_len = response.content_length();
        // let bytes = response.bytes().await.map_err(|e| {
        //     MediaError::Internal(format!("Failed to read image data: {}", e))
        // })?;

        // if let Some(content_len) = expected_len
        //     && bytes.len() as u64 != content_len
        // {
        //     return Err(MediaError::Internal(format!(
        //         "Image size mismatch: got {} bytes, expected {}",
        //         bytes.len(),
        //         content_len
        //     )));
        // }
        //
        // // Signal completion after successful extraction; DB rows will follow.
        // self.complete_variant(&vkey).await;

        let cache_key = image_cache_key_for(iid, imz);

        // let variant_dir = self
        //     .cache_dir
        //     .join(imz.image_type())
        //     .join(&imz.to_tmdb_param());
        // tokio::fs::create_dir_all(&variant_dir)
        //     .await
        //     .map_err(MediaError::Io)?;

        // let filename = build_variant_filename(&params, &image_record.tmdb_path);
        // let file_path = variant_dir.join(&filename);

        // let output_path = file_path.clone();

        info!(
            "[generate_episode_thumbnail] Extracted frame: {}x{} -> thumbnail: {}x{}",
            src_w, src_h, target_w, target_h
        );

        let stored = self.blob_store.write(&cache_key, &encoded_jpeg).await?;
        let integrity_string = stored.integrity.to_string();
        let token = ImageFileStore::token_from_integrity(&integrity_string);
        self.file_store
            .write_if_missing(&token, &encoded_jpeg)
            .await?;

        // let file_size_i32 = bytes.len() as i32;

        let theme_color = if matches!(imz.image_variant(), ImageVariant::Poster)
        {
            self.extract_theme_color(&encoded_jpeg)
        } else {
            None
        };

        // Use owned metadata so we can safely build an ImgInput with &str fields.
        struct OwnedImgMeta {
            cache_key: String,
            integrity: String,
            theme_color: Option<String>,
        }

        let owned = OwnedImgMeta {
            cache_key: cache_key.to_string(),
            integrity: integrity_string,
            theme_color,
        };

        let ctx = ImgInput {
            iid,
            tmdb_path: None,
            imz,
            decoded_dimensions: Some(
                ImageDimensions::try_from((target_w, target_h)).map_err(
                    |err| {
                        MediaError::Internal(format!(
                            "Generated thumbnail has invalid target dimensions {target_w}x{target_h}: {err:?}"
                        ))
                    },
                )?,
            ),
            theme_color: owned.theme_color.as_deref(),
            cache_key: &owned.cache_key,
            integrity: &owned.integrity,
            byte_len: stored.byte_len as i32,
            media_id: None,
            media_type: None,
        };

        info!(
            "[download_variant] Prepared upsert context: iid={}, media_type={:?}, media_id={:?}, imz={:?}, width={:?}, cache_key={}, integrity={}, byte_len={}",
            ctx.iid,
            ctx.media_type,
            ctx.media_id,
            ctx.imz,
            ctx.imz.width(),
            ctx.cache_key,
            ctx.integrity,
            ctx.byte_len
        );

        let record = self.images.upsert_image(&ctx).await?;
        let _ = self.image_events.send(ImageReadyEvent {
            iid: record.iid,
            imz: record.imz,
            token,
        });
        Ok(record)

        // if let Some(existing_image) =
        //     self.images.lookup_variant_by_hash(&hash).await?
        // {
        //     if let Some(existing_variant) =
        //         self.images.lookup_variant(existing_image.id, ctx).await?
        //     {
        //         self.images
        //             .mark_media_image_variant_cached(
        //                 &params,
        //                 existing_variant.width,
        //                 existing_variant.height,
        //                 Some(&hash),
        //                 None,
        //             )
        //             .await?;

        //         // Notify any waiters that thumbnail is ready
        //         self.complete_variant(&vkey).await;
        //         return Ok(PathBuf::from(&existing_variant.file_path));
        //     }

        //     self.images
        //         .upsert_variant(
        //             existing_image.id,
        //             imz,
        //             file_path.to_string_lossy().as_ref(),
        //             file_size_i32,
        //             width,
        //             height,
        //         )
        //         .await?;
        // } else {
        //     self.images
        //         .upsert_variant(
        //             image_record.mid,
        //             imz,
        //             file_path.to_string_lossy().as_ref(),
        //             file_size_i32,
        //             width,
        //             height,
        //         )
        //         .await?;
        //     }

        //     self.images
        //         .mark_media_image_variant_cached(
        //             &params,
        //             width,
        //             height,
        //             Some(&hash),
        //             None,
        //         )
        //         .await?;

        //     // Notify any waiters that thumbnail is ready
        //     self.complete_variant(&vkey).await;

        //     Ok(file_path)
    }

    /// Returns an image from the cache
    /// Either ensures the presence of the image, or force overwrites
    pub async fn cached_image(
        &self,
        iid: Uuid,
        imz: ImageSize,
        policy: CachePolicy,
    ) -> Result<ImageRecord> {
        info!(
            "[get_or_download_variant] Called with iid={}, imz={:?}, width={:?}",
            iid,
            imz,
            imz.width()
        );

        let mut imz = imz;
        // 1. Ensure we have a tmdb_image_variants row for this image id.
        debug!("[get_or_download_variant] Looking up variant by iid...");
        let variant: OriginalImage = match self
            .images
            .lookup_variant_by_iid(iid)
            .await?
        {
            Some(v) => {
                info!(
                    "[get_or_download_variant] Found variant: iid={}, tmdb_path={}, is_primary={}",
                    v.iid, v.tmdb_path, v.is_primary
                );
                v
            }
            None => {
                // No variant metadata for this request; nothing we can fetch.
                let msg = format!(
                    "[get_or_download_variant] No variant found for iid={}, imz={:?}",
                    iid, imz
                );

                error!(msg);
                return Err(MediaError::NotFound(msg));
            }
        };

        if !imz.has_width() {
            imz = variant.imz
        }

        if policy == CachePolicy::Ensure {
            // DB row is the source of truth for "is cached"; the serving path
            // will trigger repair if the underlying bytes are missing/corrupt.
            if let Some(existing) =
                self.images.lookup_cached_image(iid, imz).await?
            {
                return Ok(existing);
            }
        }

        if matches!(imz.image_variant(), ImageVariant::Thumbnail) {
            #[cfg(feature = "demo")]
            {
                let msg = "Cannot generate thumbnail in demo mode due to zerosize demo media files";
                warn!(msg);
                return Err(MediaError::Internal(msg.to_string()));
            }
            #[cfg(not(feature = "demo"))]
            {
                return self
                    .generate_episode_thumbnail_cached(&variant, imz)
                    .await;
            }
        }

        // 4. Download from TMDB and cache via the image repository.

        let iin = ImgInput {
            iid: variant.iid,
            media_id: Some(variant.media_id),
            media_type: Some(variant.media_type),
            tmdb_path: Some(&variant.tmdb_path),
            imz,
            decoded_dimensions: None,
            theme_color: None,
            cache_key: "",
            integrity: "",
            byte_len: 0,
        };

        info!(
            "[get_or_download_variant] Calling download_variant with tmdb_path={}, iid={}, imz={:?}",
            variant.tmdb_path, variant.iid, imz
        );

        // 3. Per-variant singleflight: serialize concurrent downloads of the same TMDB size.
        // Key by iid + TMDB size param so multiple callers converge on a single leader.
        let vkey =
            format!("{}:{}", variant.iid.as_hyphenated(), imz.to_tmdb_param());
        let (is_leader, notify) = self.subscribe_variant(&vkey).await;

        if !is_leader {
            notify.notified().await;
            if let Some(existing) =
                self.images.lookup_cached_image(iid, imz).await?
            {
                return Ok(existing);
            }
            // Leader may have failed; fall through and attempt ourselves.
        }

        let result = self.download_variant(iin).await;
        if is_leader {
            self.complete_variant(&vkey).await;
        }
        result
    }

    #[cfg(not(feature = "demo"))]
    async fn generate_episode_thumbnail_cached(
        &self,
        variant: &OriginalImage,
        imz: ImageSize,
    ) -> Result<ImageRecord> {
        if variant.media_type != ImageMediaType::Episode {
            return Err(MediaError::Internal(format!(
                "Thumbnail generation is only supported for episode media; got {:?}",
                variant.media_type
            )));
        }

        let media_id: MediaID =
            MediaID::from((variant.media_id, VideoMediaType::Episode));
        let media_file = self
            .media_files
            .get_by_media_id(&media_id)
            .await?
            .ok_or_else(|| {
                MediaError::NotFound(format!(
                    "Missing media file for episode thumbnail generation (media_id={})",
                    media_id
                ))
            })?;

        let size_key = imz
            .width()
            .map(|w| w.to_string())
            .unwrap_or_else(|| imz.to_tmdb_param().to_string());
        let vkey = format!("{}:{}", variant.iid.as_hyphenated(), size_key);
        let (is_leader, notify) = self.subscribe_variant(&vkey).await;

        if !is_leader {
            notify.notified().await;
            if let Some(existing) =
                self.images.lookup_cached_image(variant.iid, imz).await?
            {
                return Ok(existing);
            }
            // Leader may have failed; fall through and attempt ourselves.
        }

        let result = self
            .generate_episode_thumbnail(media_file.id, variant.iid, imz)
            .await;
        if is_leader {
            self.complete_variant(&vkey).await;
        }
        result
    }

    async fn subscribe_variant(&self, key: &str) -> (bool, Arc<Notify>) {
        let mut map = self.in_flight_variants.lock().await;
        if let Some(n) = map.get(key) {
            let waiters =
                self.sf_variant_waiters.fetch_add(1, Ordering::Relaxed) + 1;
            let leaders = self.sf_variant_leaders.load(Ordering::Relaxed);
            debug!(
                "singleflight-variant wait: key={}, leaders={}, waiters={}",
                key, leaders, waiters
            );
            return (false, Arc::clone(n));
        }
        let notify = Arc::new(Notify::new());
        map.insert(key.to_string(), Arc::clone(&notify));
        let leaders =
            self.sf_variant_leaders.fetch_add(1, Ordering::Relaxed) + 1;
        let waiters = self.sf_variant_waiters.load(Ordering::Relaxed);
        debug!(
            "singleflight-variant lead: key={}, leaders={}, waiters={}",
            key, leaders, waiters
        );
        (true, notify)
    }

    async fn complete_variant(&self, key: &str) {
        let notify = {
            let mut map = self.in_flight_variants.lock().await;
            map.remove(key)
        };
        if let Some(n) = notify {
            n.notify_waiters();
            debug!("singleflight-variant complete: key={}", key);
        }
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
}

fn is_retryable_cache_fill_error(err: &MediaError) -> bool {
    match err {
        MediaError::Http(e) => e.is_timeout() || e.is_connect(),
        MediaError::HttpStatus { status, .. } => {
            status.as_u16() == 429 || status.is_server_error()
        }
        #[cfg(feature = "database")]
        MediaError::Database(sqlx::Error::PoolTimedOut) => true,
        #[cfg(feature = "database")]
        MediaError::Database(sqlx::Error::PoolClosed) => true,
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachePolicy {
    Ensure,
    Refresh,
}

#[cfg(feature = "ffmpeg")]
fn extract_frame_at_percentage(
    input_path: &str,
    percentage: f64,
) -> Result<(u32, u32, Vec<u8>)> {
    use ffmpeg::codec::context::Context as CodecContext;

    let mut input_ctx = ffmpeg::format::input(&input_path).map_err(|e| {
        MediaError::InvalidMedia(format!("Failed to open video file: {e}"))
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
            MediaError::InvalidMedia(format!(
                "Failed to create codec context: {e}"
            ))
        })?;
    let mut decoder = codec_ctx.decoder().video().map_err(|e| {
        MediaError::InvalidMedia(format!("Failed to create video decoder: {e}"))
    })?;

    let duration = input_ctx.duration();
    if duration > 0 && percentage > 0.0 {
        let target_position = (duration as f64 * percentage) as i64;
        input_ctx.seek(target_position, ..).map_err(|e| {
            MediaError::InvalidMedia(format!("Failed to seek: {e}"))
        })?;
    }

    let mut received_frame = None;
    for (stream, packet) in input_ctx.packets() {
        if stream.index() != video_stream_index {
            continue;
        }

        decoder.send_packet(&packet).map_err(|e| {
            MediaError::InvalidMedia(format!("Failed to send packet: {e}"))
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
        MediaError::InvalidMedia(format!("Failed to create scaler: {e}"))
    })?;

    let mut rgb_frame = ffmpeg::frame::Video::empty();
    scaler.run(&frame, &mut rgb_frame).map_err(|e| {
        MediaError::InvalidMedia(format!("Failed to scale frame: {e}"))
    })?;

    let width = rgb_frame.width();
    let height = rgb_frame.height();
    let data = rgb_frame.data(0);
    let stride = rgb_frame.stride(0);

    let row_len = (width as usize).checked_mul(3).ok_or_else(|| {
        MediaError::InvalidMedia("RGB row length overflow".into())
    })?;
    let expected = (height as usize).checked_mul(row_len).ok_or_else(|| {
        MediaError::InvalidMedia("RGB buffer length overflow".into())
    })?;

    let mut rgb = vec![0u8; expected];
    for y in 0..height as usize {
        let src_off = y * stride;
        let dst_off = y * row_len;

        let src = data.get(src_off..src_off + row_len).ok_or_else(|| {
            MediaError::InvalidMedia(
                "FFmpeg RGB frame buffer shorter than expected".into(),
            )
        })?;
        rgb[dst_off..dst_off + row_len].copy_from_slice(src);
    }

    Ok((width, height, rgb))
    // // Encode as JPEG and write atomically: write to temp file, fsync, then rename
    // atomic_write_jpeg_rgb8(output_path, width, height, buffer.into_raw())
    //     .map_err(|e| MediaError::Io(std::io::Error::other(e)))
}

#[cfg(not(feature = "ffmpeg"))]
fn extract_frame_at_percentage(
    _input_path: &str,
    _percentage: f64,
) -> Result<(u32, u32, Vec<u8>)> {
    Err(MediaError::Internal(
        "FFmpeg support is required for thumbnail generation".into(),
    ))
}

fn encode_thumbnail_jpeg_rgb24(
    src_w: u32,
    src_h: u32,
    rgb_bytes: Vec<u8>,
    target_w: u32,
    target_h: u32,
    quality: u8,
) -> Result<Vec<u8>> {
    use image::codecs::jpeg::JpegEncoder;
    use image::imageops::FilterType;
    use image::{ColorType, RgbImage};
    use std::io::Cursor;

    let expected = src_w
        .checked_mul(src_h)
        .and_then(|px| px.checked_mul(3))
        .ok_or_else(|| {
            MediaError::InvalidMedia(
                "RGB buffer size overflow while encoding thumbnail".into(),
            )
        })? as usize;

    if rgb_bytes.len() != expected {
        return Err(MediaError::InvalidMedia(format!(
            "Invalid RGB24 buffer length: got {}, expected {} ({}x{}x3)",
            rgb_bytes.len(),
            expected,
            src_w,
            src_h
        )));
    }

    if target_w == 0 || target_h == 0 {
        return Err(MediaError::Internal(
            "Thumbnail target dimensions must be non-zero".into(),
        ));
    }

    let src = RgbImage::from_raw(src_w, src_h, rgb_bytes).ok_or_else(|| {
        MediaError::InvalidMedia(
            "Failed to construct RGB image from raw bytes".into(),
        )
    })?;

    // Center-crop to the target aspect ratio before resizing to avoid distortion.
    let dst_aspect = target_w as f64 / target_h as f64;
    let src_aspect = src_w as f64 / src_h as f64;

    let (crop_x, crop_y, crop_w, crop_h) = if src_aspect > dst_aspect {
        // Wider than target: crop width.
        let crop_w = ((src_h as f64) * dst_aspect).round() as u32;
        let crop_x = (src_w.saturating_sub(crop_w)) / 2;
        (crop_x, 0, crop_w.min(src_w), src_h)
    } else {
        // Taller than target: crop height.
        let crop_h = ((src_w as f64) / dst_aspect).round() as u32;
        let crop_y = (src_h.saturating_sub(crop_h)) / 2;
        (0, crop_y, src_w, crop_h.min(src_h))
    };

    let cropped =
        image::imageops::crop_imm(&src, crop_x, crop_y, crop_w, crop_h)
            .to_image();

    let resized = image::imageops::resize(
        &cropped,
        target_w,
        target_h,
        FilterType::Lanczos3,
    );

    let mut out = Cursor::new(Vec::new());
    let mut encoder = JpegEncoder::new_with_quality(&mut out, quality);

    encoder
        .encode(resized.as_raw(), target_w, target_h, ColorType::Rgb8.into())
        .map_err(|e| {
            MediaError::InvalidMedia(format!(
                "Failed to encode episode thumbnail JPEG: {e}"
            ))
        })?;

    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::encode_thumbnail_jpeg_rgb24;

    #[test]
    fn rgb24_thumbnail_encoder_produces_valid_jpeg_with_expected_dimensions() {
        let src_w = 640u32;
        let src_h = 480u32; // 4:3

        let mut rgb = Vec::with_capacity((src_w * src_h * 3) as usize);
        for y in 0..src_h {
            for x in 0..src_w {
                rgb.push((x % 256) as u8);
                rgb.push((y % 256) as u8);
                rgb.push(((x + y) % 256) as u8);
            }
        }

        let target_w = 512u32;
        let target_h = 288u32; // 16:9
        let encoded = encode_thumbnail_jpeg_rgb24(
            src_w,
            src_h,
            rgb.clone(),
            target_w,
            target_h,
            85,
        )
        .expect("encode thumbnail jpeg");

        assert!(
            encoded.len() < rgb.len(),
            "encoded jpeg should be smaller than raw RGB buffer"
        );
        assert!(
            encoded.starts_with(&[0xFF, 0xD8, 0xFF]),
            "jpeg magic bytes should be present"
        );

        let decoded =
            image::load_from_memory(&encoded).expect("decode thumbnail jpeg");
        assert_eq!(decoded.width(), target_w);
        assert_eq!(decoded.height(), target_h);
    }
}
