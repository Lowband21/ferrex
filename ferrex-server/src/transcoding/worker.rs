use super::hardware::{HardwareEncoder, HardwareEncoderType, HardwareSelector};
use super::job::{
    JobMessage, JobProgress, JobResponse, JobType, SourceVideoMetadata, TranscodingJob,
    TranscodingStatus,
};
use super::queue::JobQueue;
use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{RwLock, mpsc, oneshot};
use tokio::time::timeout;
use tracing::{debug, error, info, trace, warn};

/// Worker pool for concurrent transcoding
pub struct WorkerPool {
    workers: Vec<Worker>,
    job_queue: JobQueue,
    hardware_selector: Arc<HardwareSelector>,
    config: WorkerConfig,
    progress_tx: mpsc::Sender<JobProgress>,
}

#[derive(Clone)]
pub struct WorkerConfig {
    pub worker_count: usize,
    pub ffmpeg_path: String,
    pub max_retries: u32,
    pub retry_delay: Duration,
    pub progress_update_interval: Duration,
    pub job_timeout: Duration,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            worker_count: 2,
            ffmpeg_path: "ffmpeg".to_string(),
            max_retries: 3,
            retry_delay: Duration::from_secs(5),
            progress_update_interval: Duration::from_secs(1),
            job_timeout: Duration::from_secs(3600), // 1 hour
        }
    }
}

struct Worker {
    id: usize,
    handle: tokio::task::JoinHandle<()>,
    shutdown_tx: mpsc::Sender<()>,
}

impl WorkerPool {
    pub fn new(
        config: WorkerConfig,
        job_queue: JobQueue,
        hardware_selector: Arc<HardwareSelector>,
        progress_tx: mpsc::Sender<JobProgress>,
    ) -> Self {
        let mut workers = Vec::with_capacity(config.worker_count);

        for id in 0..config.worker_count {
            let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
            let queue_clone = job_queue.clone();
            let selector_clone = hardware_selector.clone();
            let config_clone = config.clone();
            let progress_tx_clone = progress_tx.clone();

            let handle = tokio::spawn(async move {
                Self::worker_loop(
                    id,
                    config_clone,
                    queue_clone,
                    selector_clone,
                    progress_tx_clone,
                    shutdown_rx,
                )
                .await;
            });

            workers.push(Worker {
                id,
                handle,
                shutdown_tx,
            });
        }

        info!("Started {} transcoding workers", config.worker_count);

        Self {
            workers,
            job_queue,
            hardware_selector,
            config,
            progress_tx,
        }
    }

    /// Main worker loop
    async fn worker_loop(
        id: usize,
        config: WorkerConfig,
        queue: JobQueue,
        hardware_selector: Arc<HardwareSelector>,
        progress_tx: mpsc::Sender<JobProgress>,
        mut shutdown_rx: mpsc::Receiver<()>,
    ) {
        info!("Worker {} started", id);

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    info!("Worker {} shutting down", id);
                    break;
                }
                job = Self::request_job(&queue) => {
                    if let Some(job) = job {
                        info!("Worker {} processing job {}", id, job.id);

                        let result = timeout(
                            config.job_timeout,
                            Self::process_job(job.clone(), &config, &hardware_selector, &progress_tx)
                        ).await;

                        match result {
                            Ok(Ok(())) => {
                                info!("Worker {} completed job {}", id, job.id);
                                // Update job status to completed (unless it's a master job)
                                match &job.job_type {
                                    JobType::Master { .. } => {
                                        // Master jobs don't get marked as completed here
                                        // Their status is determined by variant jobs
                                        info!("Master job {} processed, status will be determined by variants", job.id);
                                    }
                                    JobType::Regular => {
                                        // Mark regular job as completed
                                        Self::update_job_status(job.id.clone(), TranscodingStatus::Completed, &queue).await;
                                    }
                                }
                            }
                            Ok(Err(e)) => {
                                error!("Worker {} failed job {}: {}", id, job.id, e);
                                Self::handle_job_failure(job, &queue, &config).await;
                            }
                            Err(_) => {
                                error!("Worker {} job {} timed out", id, job.id);
                                Self::handle_job_timeout(job, &queue).await;
                            }
                        }
                    } else {
                        // No job available, wait a bit
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        }
    }

    /// Request a job from the queue
    async fn request_job(queue: &JobQueue) -> Option<TranscodingJob> {
        let (response_tx, response_rx) = oneshot::channel();
        let _ = queue.job_request_tx.send(response_tx).await;
        response_rx.await.ok().flatten()
    }

    /// Process a transcoding job
    async fn process_job(
        job: TranscodingJob,
        config: &WorkerConfig,
        hardware_selector: &HardwareSelector,
        progress_tx: &mpsc::Sender<JobProgress>,
    ) -> Result<()> {
        // Handle master jobs differently
        let job_type = job.job_type.clone();
        match job_type {
            JobType::Master { variant_job_ids } => {
                return Self::process_master_job(job, variant_job_ids, progress_tx).await;
            }
            JobType::Regular => {
                // Continue with regular job processing
            }
        }

        // Validate input file exists
        let input_path = std::path::Path::new(&job.media_id);
        if !tokio::fs::try_exists(input_path).await.unwrap_or(false) {
            return Err(anyhow::anyhow!(
                "Input file does not exist: {}",
                job.media_id
            ));
        }

        // Create output directory
        tokio::fs::create_dir_all(&job.output_dir)
            .await
            .context("Failed to create output directory")?;

        // Select hardware encoder if available
        let codec = extract_codec_from_profile(&job.profile.video_codec);
        let hw_encoder = hardware_selector.select_encoder(&codec);

        // Try hardware encoding first if available, then fallback to software
        let result = if let Some(hw_enc) = hw_encoder {
            info!("Attempting hardware transcoding with {}", hw_enc.name);
            match Self::run_ffmpeg_command(
                &config.ffmpeg_path,
                &job,
                Some(hw_enc),
                progress_tx,
                job.source_metadata.as_ref(),
            )
            .await
            {
                Ok(()) => Ok(()),
                Err(e)
                    if e.to_string().contains("Device creation failed")
                        || e.to_string().contains("No device available")
                        || e.to_string()
                            .contains("Generic error in an external library")
                        || e.to_string().contains("Invalid data found")
                        || e.to_string().contains("qsv") =>
                {
                    warn!(
                        "Hardware encoding failed for job {}, falling back to software: {}",
                        job.id, e
                    );
                    Self::run_ffmpeg_command(
                        &config.ffmpeg_path,
                        &job,
                        None,
                        progress_tx,
                        job.source_metadata.as_ref(),
                    )
                    .await
                }
                Err(e) => Err(e),
            }
        } else {
            info!("Using software encoding for job {}", job.id);
            Self::run_ffmpeg_command(
                &config.ffmpeg_path,
                &job,
                None,
                progress_tx,
                job.source_metadata.as_ref(),
            )
            .await
        };

        result
    }

    /// Process a master adaptive streaming job
    async fn process_master_job(
        job: TranscodingJob,
        variant_job_ids: Vec<String>,
        progress_tx: &mpsc::Sender<JobProgress>,
    ) -> Result<()> {
        info!(
            "Processing master job {} with {} variants",
            job.id,
            variant_job_ids.len()
        );
        info!("Variant job IDs: {:?}", variant_job_ids);

        // Wait a bit to ensure variant jobs are registered in the queue
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Create output directory for master playlist
        tokio::fs::create_dir_all(&job.output_dir)
            .await
            .context("Failed to create master output directory")?;

        // Generate master playlist content
        let mut master_content =
            "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-START:TIME-OFFSET=0,PRECISE=YES\n\n".to_string();

        // Add variants to the master playlist
        // Note: In a real implementation, we'd look up the actual variant details
        // For now, we'll use hardcoded values based on common profiles
        let variants = vec![
            ("4k", 3840, 2160, 20000000),
            ("1080p", 1920, 1080, 8000000),
            ("720p", 1280, 720, 4000000),
            ("480p", 854, 480, 2000000),
            ("360p", 640, 360, 1000000),
        ];

        for (name, width, height, bandwidth) in variants {
            if variant_job_ids.iter().any(|id| id.contains(name)) {
                master_content.push_str(&format!(
                    "#EXT-X-STREAM-INF:BANDWIDTH={},RESOLUTION={}x{}\nvariant/adaptive_{}/playlist.m3u8\n\n",
                    bandwidth, width, height, name
                ));
            }
        }

        // Write master playlist
        let master_path = job.playlist_path.clone();
        tokio::fs::write(&master_path, &master_content)
            .await
            .context("Failed to write master playlist")?;

        info!("Master playlist written to: {:?}", master_path);

        // Don't mark master job as completed here - let the status aggregation handle it
        // The master job's status will be determined by the variant jobs' statuses
        info!(
            "Master job {} setup complete, variant jobs will determine final status",
            job.id
        );

        Ok(())
    }

    /// Run FFmpeg command with given encoder
    async fn run_ffmpeg_command(
        ffmpeg_path: &str,
        job: &TranscodingJob,
        hw_encoder: Option<&HardwareEncoder>,
        progress_tx: &mpsc::Sender<JobProgress>,
        source_metadata: Option<&SourceVideoMetadata>,
    ) -> Result<()> {
        // Build FFmpeg command
        let mut cmd = build_ffmpeg_command(ffmpeg_path, job, hw_encoder)?;

        // Log the command for debugging
        info!("Starting FFmpeg with command: {:?}", cmd);

        // Start FFmpeg process
        let mut child = cmd
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to spawn FFmpeg")?;

        // Store process PID
        let _pid = child.id();

        // Collect stderr for error reporting
        let stderr_reader = child.stderr.take();
        let error_output = Arc::new(RwLock::new(String::new()));

        // Monitor progress
        let progress_handle = if let Some(stderr) = stderr_reader {
            let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(100);
            let job_id_clone = job.id.clone();
            let error_output_clone = error_output.clone();

            // Spawn error collector
            tokio::spawn(async move {
                while let Some(line) = rx.recv().await {
                    let mut output = error_output_clone.write().await;
                    output.push_str(&line);
                    output.push('\n');
                }
            });

            let progress_tx_clone = progress_tx.clone();
            let metadata_clone = source_metadata.cloned();
            Some(tokio::spawn(async move {
                monitor_ffmpeg_progress_with_errors(
                    job_id_clone,
                    stderr,
                    Duration::from_secs(1),
                    tx,
                    progress_tx_clone,
                    metadata_clone,
                )
                .await
            }))
        } else {
            None
        };

        // Wait for completion
        let status = child.wait().await?;

        // Cancel progress monitoring
        if let Some(handle) = progress_handle {
            handle.abort();
        }

        if !status.success() {
            let exit_code = status.code().unwrap_or(-1);
            let error_msg = error_output.read().await;
            error!("FFmpeg failed with exit code {}: {}", exit_code, error_msg);
            return Err(anyhow::anyhow!(
                "FFmpeg exited with status {}: {}",
                exit_code,
                error_msg.lines().take(10).collect::<Vec<_>>().join("\n")
            ));
        }

        Ok(())
    }

    /// Update job status in the queue
    async fn update_job_status(job_id: String, status: TranscodingStatus, queue: &JobQueue) {
        let (response_tx, response_rx) = oneshot::channel();
        let command = JobMessage::UpdateStatus {
            job_id: job_id.clone(),
            status,
        };

        if let Err(e) = queue.command_tx.send((command, response_tx)).await {
            warn!("Failed to send status update for job {}: {}", job_id, e);
            return;
        }

        match response_rx.await {
            Ok(JobResponse::Status(Some(status))) => {
                debug!("Updated job {} status to {:#?}", job_id, status);
            }
            _ => {
                warn!("Failed to update job {} status", job_id);
            }
        }
    }

    /// Handle job failure
    async fn handle_job_failure(mut job: TranscodingJob, queue: &JobQueue, config: &WorkerConfig) {
        job.increment_retry();

        if job.retry_count < config.max_retries {
            warn!(
                "Job {} failed, retrying ({}/{})",
                job.id, job.retry_count, config.max_retries
            );

            // Wait before retry
            tokio::time::sleep(config.retry_delay).await;

            // Re-queue the job
            job.status = TranscodingStatus::Queued;
            if let Err(e) = queue.submit_tx.send((job, oneshot::channel().0)).await {
                error!("Failed to re-queue job: {}", e);
            }
        } else {
            error!("Job {} failed after {} retries", job.id, config.max_retries);
            job.status = TranscodingStatus::Failed {
                error: "Max retries exceeded".to_string(),
            };
            // Update job status in queue
            update_job_status(queue, job).await;
        }
    }

    /// Handle job timeout
    async fn handle_job_timeout(mut job: TranscodingJob, queue: &JobQueue) {
        error!("Job {} timed out", job.id);

        // Kill the process if it's still running
        if let Some(pid) = job.process_pid {
            let _ = kill_process(pid).await;
        }

        job.status = TranscodingStatus::Failed {
            error: "Job timed out".to_string(),
        };

        update_job_status(queue, job).await;
    }

    /// Shutdown all workers
    pub async fn shutdown(self) {
        info!("Shutting down worker pool");

        // Send shutdown signal to all workers
        for worker in &self.workers {
            let _ = worker.shutdown_tx.send(()).await;
        }

        // Wait for all workers to finish
        for worker in self.workers {
            let _ = worker.handle.await;
        }

        info!("Worker pool shutdown complete");
    }
}

/// Build FFmpeg command for transcoding
fn build_ffmpeg_command(
    ffmpeg_path: &str,
    job: &TranscodingJob,
    hw_encoder: Option<&HardwareEncoder>,
) -> Result<Command> {
    let mut cmd = Command::new(ffmpeg_path);

    // Global options
    cmd.arg("-hide_banner");
    cmd.arg("-y"); // Overwrite output files

    // Add probesize and analyzeduration to handle subtitle streams better
    cmd.arg("-probesize").arg("50M");
    cmd.arg("-analyzeduration").arg("100M");

    // Hardware acceleration args MUST come before input file
    if let Some(encoder) = hw_encoder {
        // Get hwaccel args (these are input options)
        match encoder.encoder_type {
            HardwareEncoderType::Vaapi => {
                cmd.arg("-hwaccel").arg("vaapi");
                cmd.arg("-hwaccel_device").arg("/dev/dri/renderD128");
                cmd.arg("-hwaccel_output_format").arg("vaapi");
            }
            HardwareEncoderType::Nvenc => {
                cmd.arg("-hwaccel").arg("cuda");
                cmd.arg("-hwaccel_output_format").arg("cuda");
            }
            HardwareEncoderType::Qsv => {
                // For QSV, use auto device selection
                cmd.arg("-hwaccel").arg("qsv");
                // Try different device paths
                if std::path::Path::new("/dev/dri/renderD129").exists() {
                    cmd.arg("-qsv_device").arg("/dev/dri/renderD129");
                } else if std::path::Path::new("/dev/dri/renderD128").exists() {
                    cmd.arg("-qsv_device").arg("/dev/dri/renderD128");
                }
                // Let FFmpeg handle the init_hw_device internally
                cmd.arg("-init_hw_device").arg("qsv=hw");
                cmd.arg("-filter_hw_device").arg("hw");
            }
            HardwareEncoderType::VideoToolbox => {
                cmd.arg("-hwaccel").arg("videotoolbox");
            }
            HardwareEncoderType::Amf => {
                cmd.arg("-hwaccel").arg("d3d11va");
            }
        }
    }

    // Seek to start of file to ensure we begin from the beginning
    cmd.arg("-ss").arg("0");

    // Input file (media_id contains the actual file path)
    cmd.arg("-i").arg(&job.media_id);

    // Video encoder settings (output options)
    if let Some(encoder) = hw_encoder {
        // Set the hardware encoder
        let encoder_name = match (
            encoder.encoder_type,
            extract_codec_from_profile(&job.profile.video_codec).as_str(),
        ) {
            (HardwareEncoderType::Vaapi, "h264") => "h264_vaapi",
            (HardwareEncoderType::Vaapi, "h265") => "hevc_vaapi",
            (HardwareEncoderType::Nvenc, "h264") => "h264_nvenc",
            (HardwareEncoderType::Nvenc, "h265") => "hevc_nvenc",
            (HardwareEncoderType::Qsv, "h264") => "h264_qsv",
            (HardwareEncoderType::Qsv, "h265") => "hevc_qsv",
            (HardwareEncoderType::VideoToolbox, "h264") => "h264_videotoolbox",
            (HardwareEncoderType::VideoToolbox, "h265") => "hevc_videotoolbox",
            (HardwareEncoderType::Amf, "h264") => "h264_amf",
            (HardwareEncoderType::Amf, "h265") => "hevc_amf",
            _ => &job.profile.video_codec, // Fallback to software encoder
        };
        cmd.arg("-c:v").arg(encoder_name);
    } else {
        // Software encoding
        cmd.arg("-c:v").arg(&job.profile.video_codec);
    }

    // Video settings
    cmd.arg("-b:v").arg(&job.profile.video_bitrate);

    // Only add preset for x264/x265 or hardware encoders that support it
    let encoder_supports_preset = match hw_encoder {
        Some(encoder) => matches!(
            encoder.encoder_type,
            HardwareEncoderType::Nvenc | HardwareEncoderType::Qsv
        ),
        None => job.profile.video_codec.contains("264") || job.profile.video_codec.contains("265"),
    };

    if encoder_supports_preset {
        cmd.arg("-preset").arg(&job.profile.preset);
    }

    // Audio settings
    cmd.arg("-c:a").arg(&job.profile.audio_codec);
    cmd.arg("-b:a").arg(&job.profile.audio_bitrate);

    // Video filter chain
    let mut vf_chain = Vec::new();

    // Resolution scaling
    if let Some(resolution) = &job.profile.resolution {
        if hw_encoder.is_some() {
            // For hardware encoding, use format-specific scaling
            match hw_encoder.unwrap().encoder_type {
                HardwareEncoderType::Vaapi => {
                    vf_chain.push(format!("scale_vaapi={}", resolution));
                }
                HardwareEncoderType::Nvenc => {
                    vf_chain.push(format!("scale_cuda={}", resolution));
                }
                HardwareEncoderType::Qsv => {
                    vf_chain.push(format!("scale_qsv={}", resolution));
                }
                _ => {
                    vf_chain.push(format!("scale={}", resolution));
                }
            }
        } else {
            vf_chain.push(format!("scale={}", resolution));
        }
    }

    // Apply video filters if any
    if !vf_chain.is_empty() {
        cmd.arg("-vf").arg(vf_chain.join(","));
    }

    // Ensure all streams are mapped
    cmd.arg("-map").arg("0:v:0"); // First video stream
    cmd.arg("-map").arg("0:a:0"); // First audio stream

    // HLS output settings
    cmd.arg("-f").arg("hls");
    cmd.arg("-hls_time").arg("4");
    cmd.arg("-hls_list_size").arg("0");
    cmd.arg("-hls_segment_type").arg("mpegts");
    // Remove append_list flag to avoid discontinuity
    cmd.arg("-hls_playlist_type").arg("event"); // Event allows streaming while transcoding
    cmd.arg("-start_number").arg("0"); // Ensure segments start from 0

    cmd.arg("-hls_segment_filename").arg(
        job.output_dir
            .join("segment_%03d.ts")
            .to_string_lossy()
            .to_string(),
    );

    // Force key frames for consistent segmentation
    cmd.arg("-force_key_frames").arg("expr:gte(t,n_forced*4)");

    // Output playlist
    cmd.arg(job.playlist_path.to_string_lossy().to_string());

    Ok(cmd)
}

/// Monitor FFmpeg progress from stderr
async fn monitor_ffmpeg_progress(
    job_id: String,
    stderr: tokio::process::ChildStderr,
    update_interval: Duration,
) {
    let reader = BufReader::new(stderr);
    let mut lines = reader.lines();
    let mut last_update = tokio::time::Instant::now();
    let mut duration_seconds: Option<f64> = None;

    while let Ok(Some(line)) = lines.next_line().await {
        // Parse duration from input info
        if line.contains("Duration:") && duration_seconds.is_none() {
            if let Some(duration) = parse_duration_from_line(&line) {
                duration_seconds = Some(duration);
                debug!("Job {} detected duration: {} seconds", job_id, duration);
            }
        }

        if line.contains("frame=") && last_update.elapsed() >= update_interval {
            // Parse progress information
            if let Some(progress) = parse_ffmpeg_progress(&line, &job_id, duration_seconds, None) {
                debug!("Job {} progress: {:?}", job_id, progress);
                // TODO: Update job progress in queue
            }
            last_update = tokio::time::Instant::now();
        }
    }
}

/// Monitor FFmpeg progress and collect error messages
async fn monitor_ffmpeg_progress_with_errors(
    job_id: String,
    stderr: tokio::process::ChildStderr,
    update_interval: Duration,
    error_tx: mpsc::Sender<String>,
    progress_tx: mpsc::Sender<JobProgress>,
    source_metadata: Option<SourceVideoMetadata>,
) {
    let reader = BufReader::new(stderr);
    let mut lines = reader.lines();
    let mut last_update = tokio::time::Instant::now();

    // Use metadata if available, otherwise try to parse from FFmpeg
    let mut duration_seconds = source_metadata.as_ref().map(|m| m.duration);
    let total_frames = source_metadata.as_ref().map(|m| m.total_frames);

    info!(
        "Job {} starting progress monitoring - duration: {:?}s, total_frames: {:?}",
        job_id, duration_seconds, total_frames
    );

    while let Ok(Some(line)) = lines.next_line().await {
        // Send all lines to error collector
        let _ = error_tx.send(line.clone()).await;

        // Log errors and warnings
        if line.contains("[error]") || line.contains("Error") {
            error!("FFmpeg error in job {}: {}", job_id, line);
        } else if line.contains("[warning]") || line.contains("Warning") {
            warn!("FFmpeg warning in job {}: {}", job_id, line);
        }

        // Parse duration from input info if we don't have it from metadata
        if line.contains("Duration:") && duration_seconds.is_none() {
            if let Some(duration) = parse_duration_from_line(&line) {
                duration_seconds = Some(duration);
                info!(
                    "Job {} detected duration from FFmpeg output: {} seconds",
                    job_id, duration
                );
            } else {
                warn!(
                    "Job {} found Duration line but failed to parse: {}",
                    job_id, line
                );
            }
        }

        if line.contains("frame=") && last_update.elapsed() >= update_interval {
            // Debug log the raw line
            trace!("FFmpeg progress line: {}", line.trim());

            // Parse progress information
            if let Some(progress) =
                parse_ffmpeg_progress(&line, &job_id, duration_seconds, total_frames)
            {
                info!(
                    "Job {} progress parsed: status={:?}, fps={:?}, frame={:?}/{:?}",
                    job_id,
                    progress.status,
                    progress.fps,
                    progress.current_frame,
                    progress.total_frames
                );

                // Send progress update
                if let Err(e) = progress_tx.send(progress.clone()).await {
                    error!("Job {} failed to send progress update: {}", job_id, e);
                } else {
                    debug!("Job {} progress sent successfully", job_id);
                }

                // Also update job status in the queue
                // This is passed in via the worker context
            } else {
                debug!(
                    "Job {} failed to parse progress from line: {}",
                    job_id,
                    line.trim()
                );
            }
            last_update = tokio::time::Instant::now();
        }
    }
}

/// Parse FFmpeg progress line
fn parse_ffmpeg_progress(
    line: &str,
    job_id: &str,
    total_duration: Option<f64>,
    total_frames: Option<u64>,
) -> Option<JobProgress> {
    // Example: frame= 1234 fps= 30.0 q=28.0 size=   12345kB time=00:00:41.36 bitrate= 244.8kbits/s speed=1.23x

    // Parse current time to calculate progress
    let current_time = if let Some(time_str) = extract_value(line, "time=") {
        parse_time_to_seconds(time_str.trim())
    } else {
        None
    };

    // Calculate progress as 0.0-1.0
    let progress_fraction = match (current_time, total_duration) {
        (Some(current), Some(total)) if total > 0.0 => (current / total).min(1.0),
        _ => {
            // Fallback: If we don't have duration but have frame info, estimate progress
            // For HLS with 4-second segments, we can estimate based on frame count
            // Assume 25fps as default, so ~100 frames per segment
            if let (Some(current_frame), Some(total)) = (
                extract_value(line, "frame=").and_then(|s| s.trim().parse::<u64>().ok()),
                total_frames,
            ) {
                if total > 0 {
                    (current_frame as f64 / total as f64).min(1.0)
                } else if current_frame > 0 {
                    // No total frames, but we have current frame
                    // For HLS, assume we need at least 200 frames (2 segments @ 25fps)
                    // to start playback, so report 1% when we have that
                    if current_frame >= 200 {
                        0.01_f64.max((current_frame as f64 / 20000.0).min(1.0)) // Rough estimate
                    } else {
                        0.0
                    }
                } else {
                    0.0
                }
            } else {
                0.0
            }
        }
    };

    let mut progress = JobProgress {
        job_id: job_id.to_string(),
        status: TranscodingStatus::Processing {
            progress: progress_fraction as f32,
        },
        current_frame: None,
        total_frames, // Use the actual metadata
        fps: None,
        bitrate: None,
        speed: None,
        eta: None,
    };

    // Parse frame number
    if let Some(frame_str) = extract_value(line, "frame=") {
        progress.current_frame = frame_str.trim().parse().ok();
    }

    // Parse FPS (this is encoding FPS, not source FPS)
    if let Some(fps_str) = extract_value(line, "fps=") {
        progress.fps = fps_str.trim().parse().ok();
    }

    // Parse bitrate - handle "N/A" and various suffixes
    if let Some(bitrate_str) = extract_value(line, "bitrate=") {
        let bitrate_clean = bitrate_str.trim();
        if bitrate_clean != "N/A" && !bitrate_clean.is_empty() {
            // Extract numeric value and unit
            let parts: Vec<&str> = bitrate_clean.split_whitespace().collect();
            if let Some(value) = parts.first() {
                let unit = parts.get(1).copied().unwrap_or("kbits/s");
                if let Ok(num_value) = value.parse::<f64>() {
                    // Normalize to kbps
                    let kbps = match unit {
                        "bits/s" | "bit/s" => num_value / 1000.0,
                        "kbits/s" | "kbit/s" | "kb/s" => num_value,
                        "mbits/s" | "mbit/s" | "mb/s" => num_value * 1000.0,
                        _ => num_value, // Assume kbps
                    };
                    progress.bitrate = Some(format!("{:.0} kbps", kbps));
                }
            }
        }
    }

    // Parse speed - more robust parsing
    if let Some(speed_str) = extract_value(line, "speed=") {
        // Remove any trailing 'x' and parse
        let speed_clean = speed_str.trim().trim_end_matches(['x', 'X', ' ']);

        if let Ok(speed_val) = speed_clean.parse::<f32>() {
            progress.speed = Some(speed_val);

            // Calculate ETA based on remaining time and speed
            if let (Some(current), Some(total)) = (current_time, total_duration) {
                if speed_val > 0.0 && current < total {
                    let remaining_time = (total - current) / speed_val as f64;
                    progress.eta = Some(Duration::from_secs_f64(remaining_time));
                }
            }
        }
    }

    Some(progress)
}

/// Extract value from FFmpeg output line
fn extract_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    if let Some(start) = line.find(key) {
        let start = start + key.len();
        let rest = &line[start..];

        // Simple approach: find the next space followed by a word with '='
        let end = rest.char_indices().find_map(|(idx, ch)| {
            if ch.is_whitespace() {
                // Look ahead to see if the next non-whitespace starts a new key
                let remaining = &rest[idx..];
                let next_word_start = remaining.trim_start();

                // Check if next word contains '=' (indicating a new key)
                if let Some(space_pos) =
                    next_word_start.find(|c: char| c.is_whitespace() || c == '=')
                {
                    if next_word_start[..space_pos].contains('=') {
                        return Some(idx);
                    }
                }

                // Special handling for known patterns
                if next_word_start.starts_with("fps=")
                    || next_word_start.starts_with("q=")
                    || next_word_start.starts_with("size=")
                    || next_word_start.starts_with("time=")
                    || next_word_start.starts_with("bitrate=")
                    || next_word_start.starts_with("speed=")
                    || next_word_start.starts_with("frame=")
                {
                    return Some(idx);
                }
            }
            None
        });

        if let Some(end) = end {
            Some(rest[..end].trim())
        } else {
            Some(rest.trim())
        }
    } else {
        None
    }
}

/// Parse time string to seconds (handles 00:00:41.36 format)
fn parse_time_to_seconds(time_str: &str) -> Option<f64> {
    let parts: Vec<&str> = time_str.split(':').collect();
    if parts.len() == 3 {
        let hours = parts[0].parse::<f64>().ok()?;
        let minutes = parts[1].parse::<f64>().ok()?;
        let seconds = parts[2].parse::<f64>().ok()?;
        Some(hours * 3600.0 + minutes * 60.0 + seconds)
    } else {
        None
    }
}

/// Parse duration from FFmpeg info line
fn parse_duration_from_line(line: &str) -> Option<f64> {
    // Example: Duration: 00:42:30.48, start: 0.000000, bitrate: 5823 kb/s
    if let Some(duration_part) = line.split("Duration:").nth(1) {
        if let Some(time_str) = duration_part.split(',').next() {
            return parse_time_to_seconds(time_str.trim());
        }
    }
    None
}

/// Extract codec name from profile codec string
fn extract_codec_from_profile(codec: &str) -> String {
    match codec {
        "libx264" => "h264",
        "libx265" => "h265",
        "libvpx-vp9" => "vp9",
        "libaom-av1" => "av1",
        _ => "h264", // Default
    }
    .to_string()
}

/// Update job status in queue
async fn update_job_status(queue: &JobQueue, job: TranscodingJob) {
    let mut jobs = queue.jobs.write().await;
    if let Some(stored_job) = jobs.get_mut(&job.id) {
        *stored_job = job;
    }
}

/// Kill a process by PID
async fn kill_process(pid: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;

        kill(Pid::from_raw(pid as i32), Signal::SIGTERM).context("Failed to kill process")?;
    }

    #[cfg(windows)]
    {
        use std::process::Command;
        Command::new("taskkill")
            .args(&["/PID", &pid.to_string(), "/F"])
            .status()
            .context("Failed to kill process")?;
    }

    Ok(())
}
