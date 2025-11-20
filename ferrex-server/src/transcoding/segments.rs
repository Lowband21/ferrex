use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::process::Command;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

/// On-the-fly segment generator
pub struct SegmentGenerator {
    ffmpeg_path: String,
    segment_duration: u32,
    cache: Arc<SegmentCache>,
    generation_tasks: Arc<RwLock<HashMap<String, SegmentGenerationTask>>>,
}

/// Segment cache with LRU eviction
pub struct SegmentCache {
    segments: Arc<RwLock<HashMap<String, CachedSegment>>>,
    max_size_bytes: u64,
    current_size_bytes: Arc<RwLock<u64>>,
}

#[derive(Clone)]
struct CachedSegment {
    path: PathBuf,
    size: u64,
    last_accessed: Instant,
    access_count: u32,
}

struct SegmentGenerationTask {
    job_id: String,
    segment_number: u32,
    status: GenerationStatus,
    started_at: Instant,
}

#[derive(Clone, PartialEq)]
enum GenerationStatus {
    Pending,
    Generating,
    Completed,
    Failed(String),
}

impl SegmentGenerator {
    pub fn new(ffmpeg_path: String, segment_duration: u32, max_cache_size_mb: u64) -> Self {
        let cache = Arc::new(SegmentCache::new(max_cache_size_mb * 1024 * 1024));

        Self {
            ffmpeg_path,
            segment_duration,
            cache,
            generation_tasks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get or generate a segment on demand
    pub async fn get_segment(
        &self,
        job_id: &str,
        media_path: &Path,
        output_dir: &Path,
        segment_number: u32,
        profile: &super::profiles::TranscodingProfile,
    ) -> Result<PathBuf> {
        let segment_key = format!("{}_seg_{:03}", job_id, segment_number);
        let segment_path = output_dir.join(format!("segment_{:03}.ts", segment_number));

        // Check cache first
        if let Some(cached) = self.cache.get(&segment_key).await {
            debug!("Segment {} found in cache", segment_key);
            return Ok(cached.path);
        }

        // Check if file exists on disk
        if segment_path.exists() {
            // Add to cache
            if let Ok(metadata) = fs::metadata(&segment_path).await {
                self.cache
                    .put(segment_key, segment_path.clone(), metadata.len())
                    .await?;
            }
            return Ok(segment_path);
        }

        // Check if generation is already in progress
        {
            let tasks = self.generation_tasks.read().await;
            if let Some(task) = tasks.get(&segment_key) {
                match &task.status {
                    GenerationStatus::Generating => {
                        // Wait for generation to complete
                        drop(tasks);
                        return self.wait_for_segment(&segment_key, &segment_path).await;
                    }
                    GenerationStatus::Completed => {
                        return Ok(segment_path);
                    }
                    GenerationStatus::Failed(error) => {
                        return Err(anyhow::anyhow!("Segment generation failed: {}", error));
                    }
                    _ => {}
                }
            }
        }

        // Start generation
        self.generate_segment(
            job_id,
            media_path,
            output_dir,
            segment_number,
            profile,
            segment_key,
        )
        .await
    }

    /// Generate a specific segment
    async fn generate_segment(
        &self,
        job_id: &str,
        media_path: &Path,
        output_dir: &Path,
        segment_number: u32,
        profile: &super::profiles::TranscodingProfile,
        segment_key: String,
    ) -> Result<PathBuf> {
        info!("Generating segment {} for job {}", segment_number, job_id);

        // Mark as generating
        {
            let mut tasks = self.generation_tasks.write().await;
            tasks.insert(
                segment_key.clone(),
                SegmentGenerationTask {
                    job_id: job_id.to_string(),
                    segment_number,
                    status: GenerationStatus::Generating,
                    started_at: Instant::now(),
                },
            );
        }

        // Calculate time range
        let start_time = segment_number as f64 * self.segment_duration as f64;
        let duration = self.segment_duration as f64;

        // Build FFmpeg command for single segment
        let mut cmd = Command::new(&self.ffmpeg_path);
        
        // Seek to start time
        cmd.arg("-ss").arg(format!("{:.2}", start_time));
        
        // Input file
        cmd.arg("-i").arg(media_path);
        
        // Duration
        cmd.arg("-t").arg(format!("{:.2}", duration));
        
        // Video codec and settings
        cmd.arg("-c:v").arg(&profile.video_codec);
        cmd.arg("-b:v").arg(&profile.video_bitrate);
        cmd.arg("-preset").arg(&profile.preset);
        
        // Audio codec and settings
        cmd.arg("-c:a").arg(&profile.audio_codec);
        cmd.arg("-b:a").arg(&profile.audio_bitrate);
        
        // Resolution if specified
        if let Some(resolution) = &profile.resolution {
            cmd.arg("-s").arg(resolution);
        }
        
        // Force keyframe at start
        cmd.arg("-force_key_frames").arg("expr:gte(t,0)");
        
        // Output format
        cmd.arg("-f").arg("mpegts");
        
        // Output file
        let segment_path = output_dir.join(format!("segment_{:03}.ts", segment_number));
        cmd.arg(&segment_path);
        
        // Execute command
        let output = cmd.output().await.context("Failed to execute FFmpeg")?;
        
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            error!("FFmpeg failed for segment {}: {}", segment_key, error);
            
            // Mark as failed
            let mut tasks = self.generation_tasks.write().await;
            if let Some(task) = tasks.get_mut(&segment_key) {
                task.status = GenerationStatus::Failed(error.to_string());
            }
            
            return Err(anyhow::anyhow!("FFmpeg failed: {}", error));
        }
        
        // Mark as completed
        {
            let mut tasks = self.generation_tasks.write().await;
            if let Some(task) = tasks.get_mut(&segment_key) {
                task.status = GenerationStatus::Completed;
            }
        }
        
        // Add to cache
        if let Ok(metadata) = fs::metadata(&segment_path).await {
            self.cache
                .put(segment_key.clone(), segment_path.clone(), metadata.len())
                .await?;
        }
        
        info!("Generated segment {} successfully", segment_key);
        Ok(segment_path)
    }

    /// Wait for a segment that's being generated
    async fn wait_for_segment(&self, segment_key: &str, segment_path: &Path) -> Result<PathBuf> {
        let start = Instant::now();
        let timeout = Duration::from_secs(30); // 1 minute timeout

        loop {
            if start.elapsed() > timeout {
                return Err(anyhow::anyhow!("Timeout waiting for segment"));
            }

            // Check task status
            {
                let tasks = self.generation_tasks.read().await;
                if let Some(task) = tasks.get(segment_key) {
                    match &task.status {
                        GenerationStatus::Completed => {
                            return Ok(segment_path.to_path_buf());
                        }
                        GenerationStatus::Failed(error) => {
                            return Err(anyhow::anyhow!("Segment generation failed: {}", error));
                        }
                        _ => {}
                    }
                }
            }

            // Wait a bit before checking again
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Pre-generate segments ahead of current position
    pub async fn pregenerate_segments(
        &self,
        job_id: &str,
        media_path: &Path,
        output_dir: &Path,
        current_segment: u32,
        segments_ahead: u32,
        profile: &super::profiles::TranscodingProfile,
    ) {
        let generator = self.clone();
        let job_id = job_id.to_string();
        let media_path = media_path.to_path_buf();
        let output_dir = output_dir.to_path_buf();
        let profile = profile.clone();

        tokio::spawn(async move {
            for i in 1..=segments_ahead {
                let segment_number = current_segment + i;
                let segment_key = format!("{}_seg_{:03}", job_id, segment_number);

                // Skip if already generated or generating
                {
                    let tasks = generator.generation_tasks.read().await;
                    if tasks.contains_key(&segment_key) {
                        continue;
                    }
                }

                // Check if file already exists
                let segment_path = output_dir.join(format!("segment_{:03}.ts", segment_number));
                if segment_path.exists() {
                    continue;
                }

                // Generate segment
                if let Err(e) = generator
                    .generate_segment(
                        &job_id,
                        &media_path,
                        &output_dir,
                        segment_number,
                        &profile,
                        segment_key,
                    )
                    .await
                {
                    warn!("Failed to pregenerate segment {}: {}", segment_number, e);
                }
            }
        });
    }

    /// Clean up old generation tasks
    pub async fn cleanup_tasks(&self, max_age: Duration) {
        let mut tasks = self.generation_tasks.write().await;
        let now = Instant::now();

        tasks.retain(|_, task| {
            let age = now.duration_since(task.started_at);
            age < max_age
        });
    }
}

impl Clone for SegmentGenerator {
    fn clone(&self) -> Self {
        Self {
            ffmpeg_path: self.ffmpeg_path.clone(),
            segment_duration: self.segment_duration,
            cache: self.cache.clone(),
            generation_tasks: self.generation_tasks.clone(),
        }
    }
}

impl SegmentCache {
    fn new(max_size_bytes: u64) -> Self {
        Self {
            segments: Arc::new(RwLock::new(HashMap::new())),
            max_size_bytes,
            current_size_bytes: Arc::new(RwLock::new(0)),
        }
    }

    /// Get a segment from cache
    async fn get(&self, key: &str) -> Option<CachedSegment> {
        let mut segments = self.segments.write().await;
        if let Some(segment) = segments.get_mut(key) {
            segment.last_accessed = Instant::now();
            segment.access_count += 1;
            Some(segment.clone())
        } else {
            None
        }
    }

    /// Put a segment in cache
    async fn put(&self, key: String, path: PathBuf, size: u64) -> Result<()> {
        // Check if we need to evict
        let mut current_size = self.current_size_bytes.write().await;
        if *current_size + size > self.max_size_bytes {
            drop(current_size);
            self.evict_lru(size).await?;
            current_size = self.current_size_bytes.write().await;
        }

        // Add to cache
        let segment = CachedSegment {
            path,
            size,
            last_accessed: Instant::now(),
            access_count: 1,
        };

        let mut segments = self.segments.write().await;
        segments.insert(key, segment);
        *current_size += size;

        Ok(())
    }

    /// Evict least recently used segments
    async fn evict_lru(&self, needed_space: u64) -> Result<()> {
        let mut segments = self.segments.write().await;
        let mut current_size = self.current_size_bytes.write().await;

        // Sort by last accessed time
        let mut entries: Vec<_> = segments.iter().collect();
        entries.sort_by_key(|(_, seg)| seg.last_accessed);

        let mut freed = 0u64;
        let mut to_remove = Vec::new();

        for (key, segment) in entries {
            if freed >= needed_space || *current_size <= self.max_size_bytes / 2 {
                break;
            }

            to_remove.push(key.clone());
            freed += segment.size;
            *current_size -= segment.size;

            // Try to delete the file
            if let Err(e) = fs::remove_file(&segment.path).await {
                warn!("Failed to delete cached segment: {}", e);
            }
        }

        for key in to_remove {
            segments.remove(&key);
        }

        Ok(())
    }

    /// Clear all cached segments
    pub async fn clear(&self) -> Result<()> {
        let mut segments = self.segments.write().await;
        let mut current_size = self.current_size_bytes.write().await;

        for (_, segment) in segments.iter() {
            if let Err(e) = fs::remove_file(&segment.path).await {
                warn!("Failed to delete cached segment: {}", e);
            }
        }

        segments.clear();
        *current_size = 0;

        Ok(())
    }
}