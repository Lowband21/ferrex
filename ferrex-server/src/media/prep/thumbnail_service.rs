use anyhow::{Context, Result};
use ferrex_core::database::ports::media_files::MediaFilesReadPort;
use ffmpeg_next as ffmpeg;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use uuid::Uuid;

pub struct ThumbnailService {
    cache_dir: PathBuf,
    media_files: Arc<dyn MediaFilesReadPort>,
}

impl fmt::Debug for ThumbnailService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThumbnailService")
            .field("cache_dir", &self.cache_dir)
            .field("media_files_repo", &"dyn MediaFilesReadPort")
            .finish()
    }
}

impl ThumbnailService {
    pub fn new(cache_dir: PathBuf, media_files: Arc<dyn MediaFilesReadPort>) -> Result<Self> {
        // Initialize ffmpeg
        ffmpeg::init().context("Failed to initialize ffmpeg")?;

        Ok(Self {
            cache_dir,
            media_files,
        })
    }

    /// Get the path to a cached thumbnail
    pub fn get_thumbnail_path(&self, media_id: &Uuid) -> PathBuf {
        self.cache_dir
            .join("thumbnails")
            .join(format!("{}_thumb.jpg", media_id))
    }

    /// Check if a thumbnail is already cached
    pub async fn has_cached_thumbnail(&self, media_id: &Uuid) -> bool {
        self.get_thumbnail_path(media_id).exists()
    }

    /// Extract and cache a thumbnail from a video file
    pub async fn extract_thumbnail(&self, media_id: &Uuid, video_path: &str) -> Result<PathBuf> {
        let thumbnail_path = self.get_thumbnail_path(media_id);

        // Check if already cached
        if thumbnail_path.exists() {
            tracing::debug!("Thumbnail already cached for {}", media_id);
            return Ok(thumbnail_path);
        }

        // Ensure thumbnails directory exists
        if let Some(parent) = thumbnail_path.parent() {
            fs::create_dir_all(parent)
                .await
                .context("Failed to create thumbnails directory")?;
        }

        // Extract thumbnail using ffmpeg
        tracing::info!("Extracting thumbnail for {} from {}", media_id, video_path);

        // Run extraction in blocking task since ffmpeg is not async
        let video_path = video_path.to_string();
        let thumbnail_path_clone = thumbnail_path.clone();

        tokio::task::spawn_blocking(move || {
            // Catch panics that might occur from FFmpeg operations
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                extract_frame_at_percentage(&video_path, &thumbnail_path_clone, 0.1)
            })) {
                Ok(result) => result,
                Err(_) => Err(anyhow::anyhow!(
                    "Thumbnail extraction panicked, likely due to corrupt video file"
                )),
            }
        })
        .await
        .context("Failed to spawn blocking task")?
        .context("Failed to extract thumbnail")?;

        Ok(thumbnail_path)
    }

    /// Get cached thumbnail or extract if needed
    pub async fn get_or_extract_thumbnail(&self, media_id: &Uuid) -> Result<PathBuf> {
        // Check if already cached
        let thumbnail_path = self.get_thumbnail_path(media_id);
        if thumbnail_path.exists() {
            return Ok(thumbnail_path);
        }

        // Get media file path from database
        let media = self
            .media_files
            .get_by_id(media_id)
            .await
            .context("Failed to get media from repository")?
            .ok_or_else(|| anyhow::anyhow!("Media not found"))?;

        let video_path = media.path.to_string_lossy().to_string();

        // Extract thumbnail
        self.extract_thumbnail(media_id, &video_path).await
    }
}

/// Extract a frame from video at given percentage (0.0 to 1.0)
fn extract_frame_at_percentage(
    input_path: &str,
    output_path: &Path,
    percentage: f64,
) -> Result<()> {
    tracing::info!(
        "Extracting thumbnail from {} at {}% to {:?}",
        input_path,
        percentage * 100.0,
        output_path
    );

    // Open input with error handling for corrupt files
    let mut input_ctx = match ffmpeg::format::input(&input_path) {
        Ok(ctx) => ctx,
        Err(e) => {
            tracing::error!("Failed to open input file '{}': {}", input_path, e);
            return Err(anyhow::anyhow!(
                "Cannot open video file, it may be corrupt or unsupported: {}",
                e
            ));
        }
    };

    // Find video stream
    let video_stream = match input_ctx.streams().best(ffmpeg::media::Type::Video) {
        Some(stream) => stream,
        None => {
            tracing::error!("No video stream found in '{}'", input_path);
            return Err(anyhow::anyhow!("No video stream found in file"));
        }
    };

    let video_stream_index = video_stream.index();

    // Get video stream info and create decoder
    let (time_base, stream_duration, codec_params) = {
        let video_stream = input_ctx.stream(video_stream_index).unwrap();
        let time_base = video_stream.time_base();
        let duration = video_stream.duration();
        let codec_params = video_stream.parameters();
        tracing::debug!(
            "Video stream: duration={}, time_base={}/{}",
            duration,
            time_base.numerator(),
            time_base.denominator()
        );
        (time_base, duration, codec_params)
    };

    // Try to seek to the target position
    let mut seek_succeeded = false;

    // First try to get duration from the format context
    let format_duration = input_ctx.duration();
    tracing::debug!("Format duration: {}", format_duration);

    if format_duration > 0 && percentage > 0.0 {
        // Duration is in AV_TIME_BASE units (microseconds)
        let seek_position = (format_duration as f64 * percentage) as i64;
        tracing::info!(
            "Seeking to {}% = {} microseconds",
            percentage * 100.0,
            seek_position
        );

        if input_ctx.seek(seek_position, ..).is_ok() {
            seek_succeeded = true;
            tracing::info!("Seek succeeded");
        } else {
            tracing::warn!("Seek failed, will extract from beginning");
        }
    } else if stream_duration > 0 && percentage > 0.0 {
        // Fall back to stream duration with time base conversion
        let target_ts = (stream_duration as f64 * percentage) as i64;
        let seek_seconds =
            (target_ts as f64 * time_base.numerator() as f64) / time_base.denominator() as f64;
        let seek_position = (seek_seconds * 1_000_000.0) as i64;

        tracing::info!(
            "Seeking using stream duration to {}% = {} microseconds",
            percentage * 100.0,
            seek_position
        );

        if input_ctx.seek(seek_position, ..).is_ok() {
            seek_succeeded = true;
            tracing::info!("Seek succeeded");
        } else {
            tracing::warn!("Seek failed, will extract from beginning");
        }
    } else {
        tracing::warn!("No valid duration found, extracting from beginning");
    }

    // Create decoder with error handling
    let codec = match ffmpeg::codec::context::Context::from_parameters(codec_params) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to create codec context: {}", e);
            return Err(anyhow::anyhow!(
                "Invalid or corrupt video codec parameters: {}",
                e
            ));
        }
    };

    let mut decoder = match codec.decoder().video() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("Failed to create video decoder: {}", e);
            return Err(anyhow::anyhow!(
                "Cannot decode video stream, file may be corrupt: {}",
                e
            ));
        }
    };

    // Validate decoder dimensions
    let original_width = decoder.width();
    let original_height = decoder.height();

    if original_width == 0 || original_height == 0 {
        tracing::error!(
            "Invalid video dimensions: {}x{}",
            original_width,
            original_height
        );
        return Err(anyhow::anyhow!(
            "Video has invalid dimensions: {}x{}",
            original_width,
            original_height
        ));
    }

    // Validate pixel format
    let pixel_format = decoder.format();
    if pixel_format == ffmpeg::format::Pixel::None {
        tracing::error!("Video has unspecified pixel format");
        return Err(anyhow::anyhow!(
            "Video has unspecified pixel format, cannot process"
        ));
    }

    let aspect_ratio = original_width as f32 / original_height as f32;

    tracing::info!(
        "Video info: {}x{} (aspect ratio: {:.2}), format: {:?})",
        original_width,
        original_height,
        aspect_ratio,
        pixel_format
    );

    // Calculate target dimensions preserving aspect ratio
    // Base target: max width 320px, max height 180px
    let (target_width, target_height) = if aspect_ratio > (320.0 / 180.0) {
        // Video is wider than target aspect ratio, constrain by width
        (320, (320.0 / aspect_ratio).round() as u32)
    } else {
        // Video is taller than target aspect ratio, constrain by height
        ((180.0 * aspect_ratio).round() as u32, 180)
    };

    // Ensure minimum dimensions
    let target_width = target_width.max(120);
    let target_height = target_height.max(90);

    tracing::info!("Thumbnail dimensions: {}x{}", target_width, target_height);

    // Create scaler with calculated dimensions and error handling
    let mut scaler = match ffmpeg::software::scaling::context::Context::get(
        decoder.format(),
        decoder.width(),
        decoder.height(),
        ffmpeg::format::Pixel::RGB24,
        target_width,
        target_height,
        ffmpeg::software::scaling::flag::Flags::BILINEAR,
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to create scaler: {}", e);
            return Err(anyhow::anyhow!(
                "Cannot create video scaler, format may be unsupported: {}",
                e
            ));
        }
    };

    // Decode frames until we get one
    let mut decoded_frame = ffmpeg::util::frame::video::Video::empty();
    let mut scaled_frame = ffmpeg::util::frame::video::Video::empty();
    let mut frame_count = 0;
    let mut packet_count = 0;
    let max_packets = 500; // Limit packet processing to avoid infinite loops

    for (stream, packet) in input_ctx.packets() {
        if stream.index() == video_stream_index {
            packet_count += 1;

            if packet_count > max_packets {
                tracing::warn!(
                    "Processed {} packets without finding a suitable frame",
                    max_packets
                );
                break;
            }

            if let Err(e) = decoder.send_packet(&packet) {
                tracing::debug!("Failed to send packet: {}", e);
                continue;
            }

            // Try to receive frames (there might be multiple)
            loop {
                match decoder.receive_frame(&mut decoded_frame) {
                    Ok(_) => {
                        frame_count += 1;

                        // Validate decoded frame before processing
                        if decoded_frame.width() == 0 || decoded_frame.height() == 0 {
                            tracing::warn!(
                                "Decoded frame {} has invalid dimensions: {}x{}",
                                frame_count,
                                decoded_frame.width(),
                                decoded_frame.height()
                            );
                            continue;
                        }

                        if decoded_frame.format() == ffmpeg::format::Pixel::None {
                            tracing::warn!(
                                "Decoded frame {} has invalid pixel format",
                                frame_count
                            );
                            continue;
                        }

                        // Validate frame dimensions match decoder expectations
                        if decoded_frame.width() != original_width
                            || decoded_frame.height() != original_height
                        {
                            tracing::warn!(
                                "Frame dimensions {}x{} don't match expected {}x{}",
                                decoded_frame.width(),
                                decoded_frame.height(),
                                original_width,
                                original_height
                            );
                            // Try to continue, but this might indicate corruption
                        }

                        // Try to estimate the frame position
                        let pts = decoded_frame.pts().unwrap_or(0);
                        let time_seconds = if pts > 0 && time_base.denominator() > 0 {
                            (pts as f64 * time_base.numerator() as f64)
                                / time_base.denominator() as f64
                        } else {
                            0.0
                        };

                        tracing::info!(
                            "Successfully decoded frame {} at {}x{}, format: {:?}, pts: {}, time: {:.2}s",
                            frame_count,
                            decoded_frame.width(),
                            decoded_frame.height(),
                            decoded_frame.format(),
                            pts,
                            time_seconds
                        );

                        // Skip frames only if seek failed and we're trying to avoid black frames at the start
                        if !seek_succeeded && frame_count < 10 {
                            tracing::debug!(
                                "Seek failed, skipping frame {} to avoid black frames at start",
                                frame_count
                            );
                            continue;
                        }

                        // Scale the frame with error handling
                        match scaler.run(&decoded_frame, &mut scaled_frame) {
                            Ok(_) => {
                                // Frame scaled successfully
                            }
                            Err(e) => {
                                tracing::error!("Failed to scale frame {}: {}", frame_count, e);
                                // Try next frame instead of failing immediately
                                if frame_count < 50 {
                                    tracing::warn!("Trying next frame after scale error");
                                    continue;
                                } else {
                                    return Err(anyhow::anyhow!(
                                        "Failed to scale frame after {} attempts: {}",
                                        frame_count,
                                        e
                                    ));
                                }
                            }
                        }

                        tracing::info!(
                            "Frame scaled to {}x{}, format: {:?}",
                            scaled_frame.width(),
                            scaled_frame.height(),
                            scaled_frame.format()
                        );

                        // Check if frame data looks valid (not all black)
                        let data = scaled_frame.data(0);
                        let sample_size = std::cmp::min(100, data.len());
                        let non_zero_count = data[..sample_size].iter().filter(|&&b| b > 0).count();
                        tracing::debug!(
                            "Frame data sample: {} non-zero bytes out of {}",
                            non_zero_count,
                            sample_size
                        );

                        if non_zero_count == 0 && !seek_succeeded && frame_count < 50 {
                            tracing::warn!(
                                "Frame {} appears to be all black, trying next frame",
                                frame_count
                            );
                            continue;
                        }

                        // Save as JPEG
                        if let Err(e) = save_frame_as_jpeg(&scaled_frame, output_path) {
                            tracing::error!("Failed to save thumbnail: {}", e);
                            return Err(e);
                        }

                        tracing::info!("Thumbnail saved successfully to {:?}", output_path);
                        return Ok(());
                    }
                    Err(ffmpeg::Error::Other { errno: -11 }) => {
                        // EAGAIN - need more packets
                        break; // Break inner loop to get more packets
                    }
                    Err(e) => {
                        tracing::debug!("Failed to receive frame: {}", e);
                        break; // Break inner loop to get more packets
                    }
                }
            }
        }
    }

    Err(anyhow::anyhow!(
        "Failed to extract any frame after {} packets",
        frame_count
    ))
}

/// Save a frame as JPEG
fn save_frame_as_jpeg(frame: &ffmpeg::util::frame::video::Video, output_path: &Path) -> Result<()> {
    use image::{ImageBuffer, Rgb};

    let width = frame.width();
    let height = frame.height();
    let stride = frame.stride(0);
    let data = frame.data(0);

    tracing::debug!(
        "Saving frame: {}x{}, stride: {}, data len: {}",
        width,
        height,
        stride,
        data.len()
    );

    // Check if we need to handle stride differently
    let image_data = if stride as u32 == width * 3 {
        // No padding, use data directly
        data.to_vec()
    } else {
        // Remove padding from each row
        let mut clean_data = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            let row_start = (y as usize) * stride;
            let row_end = row_start + (width as usize) * 3;
            clean_data.extend_from_slice(&data[row_start..row_end]);
        }
        clean_data
    };

    // Create image from RGB data
    let img = ImageBuffer::<Rgb<u8>, _>::from_raw(width, height, image_data)
        .ok_or_else(|| anyhow::anyhow!("Failed to create image buffer {}x{}", width, height))?;

    // Save as JPEG with quality 85
    img.save(output_path).context("Failed to save thumbnail")?;

    tracing::debug!("Thumbnail saved to {:?}", output_path);
    Ok(())
}
