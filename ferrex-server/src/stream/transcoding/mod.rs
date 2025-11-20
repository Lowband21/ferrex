pub mod cache;
pub mod config;
pub mod hardware;
pub mod job;
pub mod profiles;
pub mod queue;
pub mod segments;
pub mod transcoding_handlers;
pub mod worker;

use anyhow::{Context, Result};
use ferrex_core::{MediaDatabase, MediaFile};
use futures::future;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

use self::cache::{CacheCleaner, CacheManager};
use self::config::{ToneMappingConfig, TranscodingConfig};
use self::hardware::{HardwareDetector, HardwareSelector};
use self::job::{
    JobMessage, JobPriority, JobProgress, JobResponse, JobType, SourceVideoMetadata,
    TranscodingJob, TranscodingStatus,
};
use self::profiles::{AdaptiveBitrateProfile, ProfileVariant, TranscodingProfile};
use self::queue::{JobQueue, JobQueueHandle};
use self::segments::SegmentGenerator;
use self::worker::{WorkerConfig, WorkerPool};

/// Cached hardware encoders to avoid re-detection
static HARDWARE_ENCODERS_CACHE: std::sync::OnceLock<Vec<hardware::HardwareEncoder>> =
    std::sync::OnceLock::new();

/// Main transcoding service
pub struct TranscodingService {
    config: TranscodingConfig,
    db: Arc<MediaDatabase>,
    queue_handle: JobQueueHandle,
    worker_pool: Arc<WorkerPool>,
    segment_generator: Arc<SegmentGenerator>,
    cache_manager: Arc<CacheManager>,
    hardware_selector: Arc<HardwareSelector>,
    progress_store: Arc<RwLock<HashMap<String, JobProgress>>>,
    media_cache: Arc<RwLock<HashMap<Uuid, (std::time::Instant, MediaFile)>>>,
}

impl TranscodingService {
    /// Create a new transcoding service
    pub async fn new(config: TranscodingConfig, db: Arc<MediaDatabase>) -> Result<Self> {
        // Ensure transcode cache directory exists
        fs::create_dir_all(&config.transcode_cache_dir)
            .await
            .context("Failed to create transcode cache directory")?;

        // Get hardware encoders from cache or detect them
        let hardware_encoders = if let Some(cached) = HARDWARE_ENCODERS_CACHE.get() {
            info!("Using cached hardware encoders");
            cached.clone()
        } else {
            info!("Detecting hardware encoders for the first time");
            let start = std::time::Instant::now();
            let detector = HardwareDetector::new(config.ffmpeg_path.clone());
            let encoders = detector.detect_hardware_encoders().await?;
            info!("Hardware detection completed in {:?}", start.elapsed());

            // Try to cache the result
            let _ = HARDWARE_ENCODERS_CACHE.set(encoders.clone());
            encoders
        };
        let hardware_selector = Arc::new(HardwareSelector::new(hardware_encoders));

        // Create job queue
        let (queue, queue_handle) = JobQueue::new(config.max_concurrent_jobs * 10);

        // Create progress channel
        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel::<JobProgress>(100);

        // Create worker pool
        let worker_config = WorkerConfig {
            worker_count: config.worker_count,
            ffmpeg_path: config.ffmpeg_path.clone(),
            max_retries: 3,
            retry_delay: Duration::from_secs(5),
            progress_update_interval: Duration::from_secs(1),
            job_timeout: Duration::from_secs(3600),
        };
        let worker_pool = Arc::new(WorkerPool::new(
            worker_config,
            queue.clone(),
            hardware_selector.clone(),
            progress_tx,
        ));

        // Create segment generator
        let segment_generator = Arc::new(SegmentGenerator::new(
            config.ffmpeg_path.clone(),
            config.segment_duration,
            config.max_cache_size_mb,
        ));

        // Create cache manager
        let cache_manager = Arc::new(CacheManager::new(
            config.transcode_cache_dir.clone(),
            config.max_cache_size_mb,
            30, // 30 days max age
        ));

        // Start cache cleaner
        let cleaner = CacheCleaner::new(
            cache_manager.clone(),
            Duration::from_secs(3600), // Run every hour
        );
        cleaner.start();

        // Create service instance
        let service = Self {
            config,
            db,
            queue_handle,
            worker_pool,
            segment_generator,
            cache_manager,
            hardware_selector,
            progress_store: Arc::new(RwLock::new(HashMap::new())),
            media_cache: Arc::new(RwLock::new(HashMap::new())),
        };

        // Spawn progress update handler
        let progress_store_clone = service.progress_store.clone();
        tokio::spawn(async move {
            while let Some(progress) = progress_rx.recv().await {
                let mut store = progress_store_clone.write().await;
                store.insert(progress.job_id.clone(), progress);
            }
        });

        Ok(service)
    }

    /// Check if a media file contains HDR content
    pub async fn is_hdr_content(media: &MediaFile) -> bool {
        if let Some(metadata) = &media.media_file_metadata {
            // Check for HDR indicators
            if let Some(bit_depth) = metadata.bit_depth {
                if bit_depth > 8 {
                    return true;
                }
            }

            // Check color transfer characteristics
            if let Some(color_transfer) = &metadata.color_transfer {
                let hdr_transfers = ["smpte2084", "arib-std-b67", "smpte2086"];
                if hdr_transfers.iter().any(|&t| color_transfer.contains(t)) {
                    return true;
                }
            }

            // Check color primaries
            if let Some(color_primaries) = &metadata.color_primaries {
                if color_primaries.contains("bt2020") {
                    return true;
                }
            }
        }

        false
    }

    /// Get media with request-level caching
    async fn get_media_cached(&self, media_id: Uuid) -> Result<MediaFile> {
        const CACHE_TTL: Duration = Duration::from_secs(30);

        // Check cache first
        {
            let cache = self.media_cache.read().await;
            if let Some((cached_at, media)) = cache.get(&media_id) {
                if cached_at.elapsed() < CACHE_TTL {
                    return Ok(media.clone());
                }
            }
        }

        // Not in cache or expired, fetch from database
        let media = self
            .db
            .backend()
            .get_media(&media_id)
            .await
            .context("Failed to get media from database")?
            .ok_or_else(|| anyhow::anyhow!("Media not found"))?;

        // Update cache
        {
            let mut cache = self.media_cache.write().await;
            cache.insert(media_id, (std::time::Instant::now(), media.clone()));

            // Clean up old entries if cache gets too large
            if cache.len() > 100 {
                let now = std::time::Instant::now();
                cache.retain(|_, (cached_at, _)| now.duration_since(*cached_at) < CACHE_TTL);
            }
        }

        Ok(media)
    }

    /// Start a transcoding job for a media file
    pub async fn start_transcoding(
        &self,
        media_id: &String,
        profile: TranscodingProfile,
        _tone_mapping_config: Option<ToneMappingConfig>,
        priority: Option<JobPriority>,
    ) -> Result<String> {
        let start = std::time::Instant::now();

        let media_id_uuid = Uuid::parse_str(media_id).unwrap();

        // Get media file from database (with caching)
        let media = self.get_media_cached(media_id_uuid).await?;

        // Check if file exists
        if !tokio::fs::try_exists(&media.path).await.unwrap_or(false) {
            return Err(anyhow::anyhow!("Media file not found on disk"));
        }

        // Check if we already have this version cached
        if self
            .cache_manager
            .has_cached_version(media_id, &profile.name)
            .await
        {
            info!(
                "Using cached version for media {} with profile {}",
                media_id, profile.name
            );
            return Ok(media_id.clone());
        }

        // Generate job ID
        let job_id = Uuid::now_v7().to_string();

        // Create output directory
        let output_dir = self
            .config
            .transcode_cache_dir
            .join(media_id)
            .join(&profile.name);

        // Create job
        let mut job = TranscodingJob::new(
            job_id.clone(),
            media.path.to_string_lossy().to_string(),
            profile,
            output_dir,
            priority.unwrap_or_default(),
        );

        // Set tone mapping config if provided
        job.tone_mapping_config = _tone_mapping_config;

        // Extract source metadata for accurate progress tracking
        if let Some(metadata) = &media.media_file_metadata {
            let duration = metadata.duration.unwrap_or(0.0);
            let framerate = metadata.framerate.unwrap_or(25.0); // Default to 25fps if unknown

            job.source_metadata = Some(SourceVideoMetadata {
                duration,
                framerate,
                total_frames: (duration * framerate).round() as u64,
                width: metadata.width.unwrap_or(0),
                height: metadata.height.unwrap_or(0),
                codec: metadata.video_codec.clone().unwrap_or_default(),
            });
        }

        // Submit job to queue
        self.queue_handle.submit(job).await?;

        info!(
            "Transcoding job {} created in: {:?}",
            job_id,
            start.elapsed()
        );
        Ok(job_id)
    }

    /// Start adaptive bitrate transcoding
    pub async fn start_adaptive_transcoding(
        &self,
        media_id: Uuid,
        priority: Option<JobPriority>,
    ) -> Result<String> {
        let overall_start = std::time::Instant::now();
        info!("Starting adaptive transcoding for media: {}", media_id);

        if !self.config.enable_adaptive_bitrate {
            return Err(anyhow::anyhow!("Adaptive bitrate streaming is disabled"));
        }

        // Get media file from database (with caching)
        let db_start = std::time::Instant::now();
        let media = self.get_media_cached(media_id).await?;
        info!("Database lookup took: {:?}", db_start.elapsed());

        // Get video dimensions
        let (width, height) = if let Some(metadata) = &media.media_file_metadata {
            (
                metadata.width.unwrap_or(1920),
                metadata.height.unwrap_or(1080),
            )
        } else {
            (1920, 1080)
        };

        // Generate adaptive profiles
        let adaptive_profile = AdaptiveBitrateProfile::generate_for_resolution(width, height);

        // Check if all variants are already cached (in parallel)
        let has_any_variant = !adaptive_profile.variants.is_empty();

        // Check all variants in parallel for better performance
        let cache_check_start = std::time::Instant::now();
        let cache_checks: Vec<_> = adaptive_profile
            .variants
            .iter()
            .map(|variant| {
                let profile_name = format!("adaptive_{}", variant.name);
                let cache_manager = self.cache_manager.clone();
                async move {
                    cache_manager
                        .has_cached_version(&media_id.to_string(), &profile_name)
                        .await
                }
            })
            .collect();

        let cache_results = future::join_all(cache_checks).await;
        let all_cached = has_any_variant && cache_results.iter().all(|&cached| cached);
        info!(
            "Cache checking {} variants took: {:?}",
            adaptive_profile.variants.len(),
            cache_check_start.elapsed()
        );

        // If all variants are cached and master playlist exists, return success immediately
        if all_cached && has_any_variant {
            let master_path = self
                .config
                .transcode_cache_dir
                .join(media_id.to_string())
                .join("master.m3u8");
            if tokio::fs::try_exists(&master_path).await.unwrap_or(false) {
                info!(
                    "All variants already cached for media {}, using existing transcoding",
                    media_id
                );
                return Ok(format!("cached_{}", media_id));
            }
        }

        // Create jobs for each variant
        let master_job_id = Uuid::new_v4().to_string();
        let mut variant_job_ids = Vec::new();

        // Start only 2 variants initially for immediate playback:
        // 1. A fast variant (720p or 480p) for quick start
        // 2. Original quality for best experience
        let mut initial_variants = Vec::new();
        let mut remaining_variants = Vec::new();

        // Debug: log all available variants
        info!(
            "Available variants: {:?}",
            adaptive_profile
                .variants
                .iter()
                .map(|v| &v.name)
                .collect::<Vec<_>>()
        );

        // Find the fast variant and original quality
        for variant in &adaptive_profile.variants {
            match variant.name.as_str() {
                "720p" | "480p" => {
                    if initial_variants
                        .iter()
                        .all(|v: &&ProfileVariant| v.name != "720p" && v.name != "480p")
                    {
                        info!("Adding fast variant: {}", variant.name);
                        initial_variants.push(variant);
                    } else {
                        remaining_variants.push(variant);
                    }
                }
                "original" => {
                    info!("Adding original quality variant");
                    initial_variants.push(variant);
                }
                _ => remaining_variants.push(variant),
            }
        }

        // Ensure we have at least 2 variants to start
        while initial_variants.len() < 2 && !remaining_variants.is_empty() {
            initial_variants.push(remaining_variants.remove(0));
        }

        info!(
            "Starting {} initial variants for immediate playback: {:?}",
            initial_variants.len(),
            initial_variants.iter().map(|v| &v.name).collect::<Vec<_>>()
        );

        // Start initial variants with high priority
        for variant in &initial_variants {
            let profile = TranscodingProfile {
                name: format!("adaptive_{}", variant.name),
                video_codec: variant.video_codec.clone(),
                audio_codec: variant.audio_codec.clone(),
                video_bitrate: variant.video_bitrate.clone(),
                audio_bitrate: variant.audio_bitrate.clone(),
                resolution: Some(variant.resolution.clone()),
                preset: variant.preset.clone(),
                apply_tone_mapping: Self::is_hdr_content(&media).await,
            };

            let job_id = self
                .start_transcoding(&media_id.to_string(), profile, None, priority)
                .await?;

            // Skip cached versions - they don't need status tracking
            if !job_id.starts_with("cached_") {
                variant_job_ids.push(job_id);
            }
        }

        // Store remaining variants info for on-demand transcoding later
        info!(
            "Remaining variants available for on-demand transcoding: {:?}",
            remaining_variants
                .iter()
                .map(|v| &v.name)
                .collect::<Vec<_>>()
        );

        // Don't start remaining variants - they will be transcoded on-demand when requested

        // If no new jobs were created (all cached), just return success
        if variant_job_ids.is_empty() {
            info!("All variants already cached for media {}", media_id);
            // Ensure master playlist exists
            self.ensure_master_playlist_exists(&media_id.to_string(), &adaptive_profile)
                .await?;
            return Ok(format!("cached_{}", media_id));
        }

        // Create master job that tracks all variant jobs
        let master_output_dir = self.config.transcode_cache_dir.join(media_id.to_string());

        info!(
            "Creating master job {} with variant jobs: {:?}",
            master_job_id, variant_job_ids
        );

        // Create master playlist immediately
        fs::create_dir_all(&master_output_dir).await?;
        let master_playlist_path = master_output_dir.join("master.m3u8");

        // Generate initial master playlist content - only include initial variants
        let mut master_content = "#EXTM3U\n#EXT-X-VERSION:3\n\n".to_string();

        // Only add initial variants to master playlist
        let all_variants_for_playlist = initial_variants.clone();
        for variant in &all_variants_for_playlist {
            let profile_name = format!("adaptive_{}", variant.name);

            let (width, height) = {
                let parts: Vec<&str> = variant.resolution.split('x').collect();
                if parts.len() == 2 {
                    (parts[0].parse().unwrap_or(0), parts[1].parse().unwrap_or(0))
                } else {
                    (0, 0)
                }
            };

            let bandwidth = if variant.video_bitrate.ends_with('M') {
                variant
                    .video_bitrate
                    .trim_end_matches('M')
                    .parse::<u32>()
                    .unwrap_or(0)
                    * 1000000 // Convert Mbps to bps
            } else if variant.video_bitrate.ends_with('k') {
                variant
                    .video_bitrate
                    .trim_end_matches('k')
                    .parse::<u32>()
                    .unwrap_or(0)
                    * 1000 // Convert kbps to bps
            } else {
                variant.video_bitrate.parse::<u32>().unwrap_or(0)
            };

            master_content.push_str(&format!(
                "#EXT-X-STREAM-INF:BANDWIDTH={},RESOLUTION={}x{}\nvariant/{}/playlist.m3u8\n\n",
                bandwidth, width, height, profile_name
            ));
        }

        // Write master playlist
        fs::write(&master_playlist_path, &master_content).await?;
        info!(
            "Created initial master playlist at: {:?}",
            master_playlist_path
        );

        // Wait a moment to ensure variant jobs are in the queue
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let mut master_job = TranscodingJob::new_master(
            master_job_id.clone(),
            media_id.to_string(),
            master_output_dir,
            priority.unwrap_or_default(),
            variant_job_ids.clone(),
        );

        // Extract and store source metadata in master job too
        if let Some(metadata) = &media.media_file_metadata {
            let duration = metadata.duration.unwrap_or(0.0);
            let framerate = metadata.framerate.unwrap_or(25.0); // Default to 25fps if unknown

            master_job.source_metadata = Some(SourceVideoMetadata {
                duration,
                framerate,
                total_frames: (duration * framerate).round() as u64,
                width: metadata.width.unwrap_or(0),
                height: metadata.height.unwrap_or(0),
                codec: metadata.video_codec.clone().unwrap_or_default(),
            });

            info!(
                "Master job {} has source duration: {} seconds",
                master_job_id, duration
            );
        }

        // Submit master job to queue with lower priority so variants process first
        master_job.priority = JobPriority::Low;

        self.queue_handle.submit(master_job).await?;

        info!(
            "Adaptive transcoding setup completed in: {:?}",
            overall_start.elapsed()
        );
        Ok(master_job_id)
    }

    /// Get a specific segment on demand
    pub async fn get_segment(&self, job_id: &str, segment_number: u32) -> Result<PathBuf> {
        // Get job details
        let response = self
            .queue_handle
            .send_command(JobMessage::GetStatus(job_id.to_string()))
            .await?;

        match response {
            JobResponse::Status(Some(job)) => {
                // Parse media path from job
                let media_path = PathBuf::from(&job.media_id);

                self.segment_generator
                    .get_segment(
                        job_id,
                        &media_path,
                        &job.output_dir,
                        segment_number,
                        &job.profile,
                    )
                    .await
            }
            _ => Err(anyhow::anyhow!("Job not found")),
        }
    }

    /// Get job status
    pub async fn get_job_status(&self, job_id: &str) -> Option<TranscodingJob> {
        let response = self
            .queue_handle
            .send_command(JobMessage::GetStatus(job_id.to_string()))
            .await
            .ok()?;

        match response {
            JobResponse::Status(Some(mut job)) => {
                // If this is a master job, aggregate variant statuses
                if let JobType::Master {
                    ref variant_job_ids,
                } = job.job_type
                {
                    job.status = self.aggregate_variant_statuses(variant_job_ids).await;
                }
                Some(job)
            }
            JobResponse::Status(None) => None,
            _ => None,
        }
    }

    /// Aggregate statuses from variant jobs for a master job
    async fn aggregate_variant_statuses(&self, variant_job_ids: &[String]) -> TranscodingStatus {
        info!(
            "Aggregating status for {} variant jobs",
            variant_job_ids.len()
        );

        let mut all_completed = true;
        let mut any_failed = false;
        let mut any_processing = false;
        let mut total_progress = 0.0;
        let mut active_jobs = 0; // Only count jobs that have started (not Pending)

        info!(
            "Aggregating status for master job with {} variant IDs: {:?}",
            variant_job_ids.len(),
            variant_job_ids
        );

        for variant_id in variant_job_ids {
            if let Some(variant_job) = self.get_variant_job_status(variant_id).await {
                info!(
                    "Found variant job {}: status = {:?}",
                    variant_id, variant_job.status
                );
                match &variant_job.status {
                    TranscodingStatus::Completed => {
                        active_jobs += 1;
                        total_progress += 1.0; // Use 1.0 for completed (100%)
                    }
                    TranscodingStatus::Failed { .. } => {
                        active_jobs += 1;
                        any_failed = true;
                        all_completed = false;
                    }
                    TranscodingStatus::Processing { progress } => {
                        active_jobs += 1;
                        any_processing = true;
                        all_completed = false;
                        total_progress += progress; // progress is already 0.0-1.0
                    }
                    TranscodingStatus::Pending | TranscodingStatus::Queued => {
                        // Don't count pending/queued jobs in progress calculation
                        // This prevents unstarted variants from diluting the progress
                        all_completed = false;
                    }
                    _ => {
                        all_completed = false;
                    }
                }
            } else {
                warn!("Could not find variant job: {}", variant_id);
            }
        }

        if any_failed {
            TranscodingStatus::Failed {
                error: "One or more variants failed".to_string(),
            }
        } else if all_completed && active_jobs > 0 {
            TranscodingStatus::Completed
        } else if any_processing || active_jobs > 0 {
            // Only average across jobs that have actually started
            let avg_progress = if active_jobs > 0 {
                (total_progress / active_jobs as f32).min(1.0).max(0.0) // Clamp to 0.0-1.0
            } else {
                0.0
            };
            info!(
                "Master job aggregation: {} active variant jobs (out of {}), total_progress={}, avg={}",
                active_jobs,
                variant_job_ids.len(),
                total_progress,
                avg_progress
            );
            TranscodingStatus::Processing {
                progress: avg_progress,
            }
        } else {
            TranscodingStatus::Pending
        }
    }

    /// Get status of a variant job without aggregation
    async fn get_variant_job_status(&self, job_id: &str) -> Option<TranscodingJob> {
        let response = self
            .queue_handle
            .send_command(JobMessage::GetStatus(job_id.to_string()))
            .await
            .ok()?;

        match response {
            JobResponse::Status(job) => job,
            _ => None,
        }
    }

    /// Get detailed progress information for a job
    pub async fn get_job_progress(&self, job_id: &str) -> Option<JobProgress> {
        let progress_map = self.progress_store.read().await;
        progress_map.get(job_id).cloned()
    }

    /// Get aggregated progress details for a master job from its variant jobs
    pub async fn get_master_job_progress(&self, job_id: &str) -> Option<JobProgress> {
        // First check if this is a master job
        if let Some(job) = self.get_variant_job_status(job_id).await {
            if let JobType::Master {
                ref variant_job_ids,
            } = job.job_type
            {
                // Aggregate progress from all variant jobs
                let progress_map = self.progress_store.read().await;

                let mut total_frames_processed = 0u64;
                let mut total_frames = 0u64;
                let mut avg_fps = 0.0f32;
                let mut avg_speed = 0.0f32;
                let mut valid_progress_count = 0;
                let mut total_progress = 0.0f32;

                for variant_id in variant_job_ids {
                    if let Some(variant_progress) = progress_map.get(variant_id) {
                        valid_progress_count += 1;

                        // Add up frames
                        if let Some(frames) = variant_progress.current_frame {
                            total_frames_processed += frames;
                        }
                        if let Some(total) = variant_progress.total_frames {
                            total_frames += total;
                        }

                        // Average FPS and speed
                        if let Some(fps) = variant_progress.fps {
                            avg_fps += fps;
                        }
                        if let Some(speed) = variant_progress.speed {
                            avg_speed += speed;
                        }

                        // Calculate progress from status
                        match &variant_progress.status {
                            TranscodingStatus::Processing { progress } => {
                                total_progress += progress;
                            }
                            TranscodingStatus::Completed => {
                                total_progress += 1.0;
                            }
                            _ => {
                                // Pending, Queued, Failed, Cancelled count as 0.0
                            }
                        }
                    }
                }

                if valid_progress_count > 0 {
                    let avg_progress = total_progress / valid_progress_count as f32;

                    return Some(JobProgress {
                        job_id: job_id.to_string(),
                        status: TranscodingStatus::Processing {
                            progress: avg_progress,
                        },
                        current_frame: if total_frames_processed > 0 {
                            Some(total_frames_processed / valid_progress_count as u64)
                        } else {
                            None
                        },
                        total_frames: if total_frames > 0 {
                            Some(total_frames / valid_progress_count as u64)
                        } else {
                            None
                        },
                        fps: if avg_fps > 0.0 {
                            Some(avg_fps / valid_progress_count as f32)
                        } else {
                            None
                        },
                        bitrate: None, // Difficult to aggregate meaningfully
                        speed: if avg_speed > 0.0 {
                            Some(avg_speed / valid_progress_count as f32)
                        } else {
                            None
                        },
                        eta: None, // Would need to calculate based on slowest variant
                    });
                }
            }
        }

        // Fall back to regular progress lookup
        self.get_job_progress(job_id).await
    }

    /// Update job progress (called by workers)
    pub async fn update_job_progress(&self, progress: JobProgress) {
        info!(
            "Updating progress for job {}: {:?}",
            progress.job_id, progress.status
        );

        // Store in progress map
        let mut progress_map = self.progress_store.write().await;
        progress_map.insert(progress.job_id.clone(), progress.clone());
        drop(progress_map); // Release lock early

        // Also update the job status in the queue
        let _ = self
            .queue_handle
            .send_command(JobMessage::UpdateStatus {
                job_id: progress.job_id,
                status: progress.status,
            })
            .await;
    }

    /// Cancel a job
    pub async fn cancel_job(&self, job_id: &str) -> Result<()> {
        let response = self
            .queue_handle
            .send_command(JobMessage::Cancel(job_id.to_string()))
            .await?;

        match response {
            JobResponse::Cancelled => Ok(()),
            JobResponse::Error(e) => Err(anyhow::anyhow!(e)),
            _ => Err(anyhow::anyhow!("Unexpected response")),
        }
    }

    /// Get playlist URL for a transcoded media
    pub async fn get_playlist_url(&self, media_id: &Uuid, profile_name: &str) -> Option<PathBuf> {
        let cache_path = self
            .cache_manager
            .get_cache_path(&media_id.to_string(), profile_name);
        let playlist_path = cache_path.join("playlist.m3u8");

        if tokio::fs::try_exists(&playlist_path).await.unwrap_or(false) {
            Some(playlist_path)
        } else {
            None
        }
    }

    /// Get master playlist for adaptive bitrate streaming
    pub async fn get_master_playlist(&self, media_id: &str) -> Option<PathBuf> {
        // Master playlist is stored directly under media_id
        let cache_path = self.config.transcode_cache_dir.join(media_id);
        let playlist_path = cache_path.join("master.m3u8");

        // Use async fs operations to check existence
        match tokio::fs::metadata(&playlist_path).await {
            Ok(_) => {
                info!("Master playlist found at: {:?}", playlist_path);
                Some(playlist_path)
            }
            Err(e) => {
                warn!("Master playlist not found at {:?}: {}", playlist_path, e);
                None
            }
        }
    }

    /// Get cache statistics
    pub async fn get_cache_stats(&self) -> Result<cache::CacheStats> {
        self.cache_manager.get_stats().await
    }

    /// Clean up cache
    pub async fn cleanup_cache(&self) -> Result<cache::CleanupResult> {
        self.cache_manager.cleanup().await
    }

    /// Get queue statistics
    pub async fn get_queue_stats(&self) -> queue::QueueStats {
        // TODO: Implement via queue handle
        queue::QueueStats::default()
    }

    /// Get available hardware encoders
    pub fn get_hardware_encoders(&self) -> &[hardware::HardwareEncoder] {
        &[]
    }

    /// Ensure master playlist exists for cached variants
    async fn ensure_master_playlist_exists(
        &self,
        media_id: &str,
        profile: &AdaptiveBitrateProfile,
    ) -> Result<()> {
        let master_dir = self.config.transcode_cache_dir.join(media_id);
        let master_path = master_dir.join("master.m3u8");

        // If master playlist already exists, we're done
        if tokio::fs::try_exists(&master_path).await.unwrap_or(false) {
            return Ok(());
        }

        // Create directory if needed
        fs::create_dir_all(&master_dir).await?;

        // Generate master playlist content
        let mut master_content = "#EXTM3U\n#EXT-X-VERSION:3\n\n".to_string();

        for variant in &profile.variants {
            let profile_name = format!("adaptive_{}", variant.name);

            // Only include variants that are actually cached
            if self
                .cache_manager
                .has_cached_version(media_id, &profile_name)
                .await
            {
                let (width, height) = {
                    let parts: Vec<&str> = variant.resolution.split('x').collect();
                    if parts.len() == 2 {
                        (parts[0].parse().unwrap_or(0), parts[1].parse().unwrap_or(0))
                    } else {
                        (0, 0)
                    }
                };

                let bandwidth = if variant.video_bitrate.ends_with('M') {
                    variant
                        .video_bitrate
                        .trim_end_matches('M')
                        .parse::<u32>()
                        .unwrap_or(0)
                        * 1000000 // Convert Mbps to bps
                } else if variant.video_bitrate.ends_with('k') {
                    variant
                        .video_bitrate
                        .trim_end_matches('k')
                        .parse::<u32>()
                        .unwrap_or(0)
                        * 1000 // Convert kbps to bps
                } else {
                    variant.video_bitrate.parse::<u32>().unwrap_or(0)
                };

                master_content.push_str(&format!(
                    "#EXT-X-STREAM-INF:BANDWIDTH={},RESOLUTION={}x{}\nvariant/adaptive_{}/playlist.m3u8\n\n",
                    bandwidth, width, height, variant.name
                ));
            }
        }

        // Write master playlist
        fs::write(&master_path, &master_content).await?;
        info!(
            "Created master playlist for cached media: {:?}",
            master_path
        );

        Ok(())
    }

    /// Shutdown the service
    pub async fn shutdown(self) {
        info!("Shutting down transcoding service");

        // TODO: Cancel all pending jobs

        // Shutdown worker pool
        if let Ok(pool) = Arc::try_unwrap(self.worker_pool) {
            pool.shutdown().await;
        }

        info!("Transcoding service shutdown complete");
    }
}

impl Clone for TranscodingService {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            db: self.db.clone(),
            queue_handle: self.queue_handle.clone(),
            worker_pool: self.worker_pool.clone(),
            segment_generator: self.segment_generator.clone(),
            cache_manager: self.cache_manager.clone(),
            hardware_selector: self.hardware_selector.clone(),
            progress_store: self.progress_store.clone(),
            media_cache: self.media_cache.clone(),
        }
    }
}
