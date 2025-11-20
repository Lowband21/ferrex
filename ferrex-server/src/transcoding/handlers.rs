use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{Json, Response},
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Stdio;
use tokio_util::io::ReaderStream;
use tracing::{debug, error, info, warn};

use crate::{
    transcoding::{self, profiles, TranscodingService},
    AppState,
};

// HLS Streaming handlers
pub async fn hls_playlist_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response, StatusCode> {
    info!("HLS playlist request for media ID: {}", id);

    // Check if we have a transcoded version
    let profile_name = params
        .get("profile")
        .cloned()
        .unwrap_or_else(|| "hdr_to_sdr_1080p".to_string());

    // First check if we have a cached version
    if let Some(playlist_path) = state
        .transcoding_service
        .get_playlist_url(&id, &profile_name)
        .await
    {
        // Serve the cached playlist
        match tokio::fs::read_to_string(&playlist_path).await {
            Ok(content) => {
                let mut response = Response::new(content.into());
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static("application/vnd.apple.mpegurl"),
                );
                Ok(response)
            }
            Err(e) => {
                warn!("Failed to read playlist file: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    } else {
        // Check if media is HDR and needs transcoding
        match state.db.backend().get_media(&id).await {
            Ok(Some(media)) => {
                if TranscodingService::is_hdr_content(&media).await {
                    // Start transcoding job if not already running
                    let profile = profiles::TranscodingProfile::hdr_to_sdr_1080p();
                    match state
                        .transcoding_service
                        .start_transcoding(&id, profile.clone(), None, None)
                        .await
                    {
                        Ok(job_id) => {
                            info!(
                                "Started on-the-fly transcoding job {} for media {}",
                                job_id, id
                            );

                            // Wait for first segment to be available (up to 10 seconds)
                            let mut retries = 0;
                            const MAX_RETRIES: u32 = 20;
                            const RETRY_DELAY_MS: u64 = 500;

                            loop {
                                // Check if we have a playlist with segments
                                if let Some(playlist_path) = state
                                    .transcoding_service
                                    .get_playlist_url(&id, &profile_name)
                                    .await
                                {
                                    if let Ok(content) =
                                        tokio::fs::read_to_string(&playlist_path).await
                                    {
                                        // Check if playlist has at least one segment
                                        if content.contains("#EXTINF:") {
                                            let mut response = Response::new(content.into());
                                            response.headers_mut().insert(
                                                header::CONTENT_TYPE,
                                                header::HeaderValue::from_static(
                                                    "application/vnd.apple.mpegurl",
                                                ),
                                            );
                                            response.headers_mut().insert(
                                                header::CACHE_CONTROL,
                                                header::HeaderValue::from_static("no-cache"),
                                            );
                                            return Ok(response);
                                        }
                                    }
                                }

                                retries += 1;
                                if retries >= MAX_RETRIES {
                                    warn!(
                                        "Timeout waiting for first segment after {} retries",
                                        retries
                                    );
                                    break;
                                }

                                tokio::time::sleep(tokio::time::Duration::from_millis(
                                    RETRY_DELAY_MS,
                                ))
                                .await;
                            }

                            // If we're here, no segments were generated in time
                            // Return error to trigger fallback
                            Err(StatusCode::SERVICE_UNAVAILABLE)
                        }
                        Err(e) => {
                            warn!("Failed to start transcoding: {}", e);
                            Err(StatusCode::INTERNAL_SERVER_ERROR)
                        }
                    }
                } else {
                    // Not HDR content, redirect to direct stream
                    Ok(Response::builder()
                        .status(StatusCode::TEMPORARY_REDIRECT)
                        .header("Location", format!("/stream/{}", id))
                        .body(axum::body::Body::empty())
                        .unwrap())
                }
            }
            Ok(None) => Err(StatusCode::NOT_FOUND),
            Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}

// Hardware encoder detection
#[derive(Debug, Clone)]
enum HardwareEncoder {
    AMF,
    VAAPI,
    None,
}

pub async fn detect_hardware_encoder(ffmpeg_path: &str) -> HardwareEncoder {
    // Check for available encoders
    let output = tokio::process::Command::new(ffmpeg_path)
        .arg("-hide_banner")
        .arg("-encoders")
        .output()
        .await
        .ok();

    if let Some(output) = output {
        let encoders = String::from_utf8_lossy(&output.stdout);

        // Check for AMF first (preferred for AMD)
        if encoders.contains("h264_amf") {
            // Verify AMF runtime is available by testing encoding
            let test_output = tokio::process::Command::new(ffmpeg_path)
                .arg("-f")
                .arg("lavfi")
                .arg("-i")
                .arg("testsrc2=duration=0.1:size=320x240:rate=30")
                .arg("-c:v")
                .arg("h264_amf")
                .arg("-f")
                .arg("null")
                .arg("-")
                .output()
                .await
                .ok();

            if let Some(test) = test_output {
                if test.status.success() {
                    info!("AMF hardware encoder detected and verified");
                    return HardwareEncoder::AMF;
                } else {
                    let stderr = String::from_utf8_lossy(&test.stderr);
                    if stderr.contains("libamfrt64.so.1") {
                        warn!("AMF encoder found but runtime libraries missing");
                    }
                }
            }
        }

        // Check for VAAPI
        if encoders.contains("h264_vaapi") && std::path::Path::new("/dev/dri/renderD128").exists() {
            info!("VAAPI hardware encoder detected");
            return HardwareEncoder::VAAPI;
        }
    }

    info!("No hardware encoder detected, using software encoding");
    HardwareEncoder::None
}

// Direct transcoding stream handler - pipes FFmpeg output directly to client
pub async fn stream_transcode_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response<Body>, StatusCode> {
    info!("Direct transcode stream request for media ID: {}", id);

    // Decode the percent-encoded ID
    let decoded_id = urlencoding::decode(&id).map_err(|e| {
        error!("Failed to decode media ID: {}", e);
        StatusCode::BAD_REQUEST
    })?;

    let profile_name = params
        .get("profile")
        .cloned()
        .unwrap_or_else(|| "hdr_to_sdr_1080p".to_string());

    // Determine target resolution and bitrate based on profile
    let (target_width, video_bitrate, max_bitrate, buffer_size) = match profile_name.as_str() {
        "hdr_to_sdr_4k" => (3840, "25M", "30M", "10M"), // 4K needs higher bitrate for quality
        "hdr_to_sdr_1440p" => (2560, "15M", "18M", "6M"), // 1440p medium bitrate
        _ => (1920, "10M", "12M", "4M"),                // 1080p default
    };

    // Get media file
    let media = match state.db.backend().get_media(&decoded_id).await {
        Ok(Some(media)) => media,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    // Build FFmpeg command for direct streaming
    let mut cmd = tokio::process::Command::new(&state.config.ffmpeg_path);

    // Check if this is HDR content (including Dolby Vision)
    let is_hdr = if let Some(metadata) = &media.media_file_metadata {
        metadata.bit_depth.map(|b| b > 8).unwrap_or(false)
            || metadata
                .color_transfer
                .as_ref()
                .map(|t| {
                    t.contains("smpte2084") || t.contains("arib-std-b67") || t.contains("smpte2086")
                })
                .unwrap_or(false)
            || metadata
                .color_primaries
                .as_ref()
                .map(|p| p.contains("bt2020"))
                .unwrap_or(false)
    } else {
        false
    };

    // Detect available hardware acceleration options
    let hardware_encoder = detect_hardware_encoder(&state.config.ffmpeg_path).await;

    // Fast startup options - balance speed and reliability
    cmd.arg("-probesize").arg("1048576"); // 1MB probe size for fast but reliable startup
    cmd.arg("-analyzeduration").arg("500000"); // 0.5 second analysis
    cmd.arg("-fpsprobesize").arg("0"); // Skip FPS probing
    cmd.arg("-flags").arg("low_delay"); // Low latency mode
    cmd.arg("-movflags").arg("+faststart"); // Low latency mode
    cmd.arg("-tune").arg("zerolatency"); // Low latency mode
    cmd.arg("-strict").arg("experimental"); // Allow experimental features

    // Apply hardware decoding options BEFORE input file
    if matches!(hardware_encoder, HardwareEncoder::VAAPI) {
        cmd.arg("-hwaccel").arg("vaapi");
        cmd.arg("-hwaccel_device").arg("/dev/dri/renderD128");
        cmd.arg("-hwaccel_output_format").arg("vaapi");
    }

    // Input file
    cmd.arg("-i").arg(&media.path);

    // Build filter chain and encoder selection based on detected hardware
    match (is_hdr, &hardware_encoder) {
        (true, HardwareEncoder::AMF) => {
            // HDR with AMF: Optimized for performance
            // Simplified filter chain for faster processing
            cmd.arg("-vf").arg(format!("setpts=PTS-STARTPTS,scale={}:-2:flags=fast_bilinear,format=p010le,tonemap=hable:desat=0,format=nv12", target_width));

            // Use AMF H.264 encoder
            cmd.arg("-c:v").arg("h264_amf");
            cmd.arg("-usage").arg("lowlatency");
            cmd.arg("-quality").arg("speed");
            cmd.arg("-b:v").arg(video_bitrate);
            cmd.arg("-maxrate").arg(max_bitrate);
            cmd.arg("-bufsize").arg(buffer_size);
            cmd.arg("-rc").arg("cbr");
        }
        (true, HardwareEncoder::VAAPI) => {
            // HDR with VAAPI: Simplified for performance
            cmd.arg("-vf").arg(format!("hwdownload,format=p010le,setpts=PTS-STARTPTS,scale={}:-2:flags=fast_bilinear,tonemap=hable:desat=0,format=nv12,hwupload", target_width));

            // Use VAAPI H.264 encoder
            cmd.arg("-c:v").arg("h264_vaapi");
            cmd.arg("-b:v").arg(video_bitrate);
            cmd.arg("-maxrate").arg(max_bitrate);
            cmd.arg("-bufsize").arg(buffer_size);
        }
        (true, HardwareEncoder::None) => {
            // HDR software only - simplified for real-time performance
            cmd.arg("-vf").arg(format!("setpts=PTS-STARTPTS,scale={}:-2:flags=fast_bilinear,format=p010le,tonemap=hable:desat=0,format=yuv420p", target_width));

            cmd.arg("-c:v").arg("libx264");
            cmd.arg("-preset").arg("ultrafast");
            cmd.arg("-tune").arg("zerolatency");
            cmd.arg("-b:v").arg(video_bitrate);
            cmd.arg("-maxrate").arg(max_bitrate);
            cmd.arg("-bufsize").arg(buffer_size);
            cmd.arg("-x264opts").arg("bframes=0:threads=16:rc-lookahead=0:weightp=0:aq-mode=0:ref=1:me=dia:subme=0:trellis=0:no-deblock:no-cabac:scenecut=0:sync-lookahead=0:aud=1");
        }
        (false, HardwareEncoder::AMF) => {
            // SDR with AMF - keep original resolution for SDR
            cmd.arg("-vf")
                .arg(format!("scale={}:-2,format=nv12", target_width));
            cmd.arg("-c:v").arg("h264_amf");
            cmd.arg("-usage").arg("lowlatency");
            cmd.arg("-quality").arg("balanced");
            cmd.arg("-b:v").arg("6M");
            cmd.arg("-rc").arg("vbr");
        }
        (false, HardwareEncoder::VAAPI) => {
            // SDR with VAAPI - filters only, hwaccel already set
            cmd.arg("-vf")
                .arg(format!("scale_vaapi=w={}:h=-2:format=nv12", target_width));
            cmd.arg("-c:v").arg("h264_vaapi");
            cmd.arg("-quality").arg("0");
            cmd.arg("-b:v").arg("6M");
        }
        (false, HardwareEncoder::None) => {
            // SDR software
            cmd.arg("-vf")
                .arg(format!("scale={}:-2,format=yuv420p", target_width));
            cmd.arg("-c:v").arg("libx264");
            cmd.arg("-preset").arg("veryfast");
            cmd.arg("-crf").arg("22");
            cmd.arg("-x264opts").arg("threads=16");
        }
    }

    // Stream mapping - select first video and best audio stream
    cmd.arg("-map").arg("0:v:0"); // First video stream
    cmd.arg("-map").arg("0:a:0"); // First audio stream (FFmpeg will handle TrueHD)

    /*
    // Audio transcoding settings with better buffering
    cmd.arg("-c:a").arg("aac");
    cmd.arg("-profile:a").arg("aac_low");
    cmd.arg("-b:a").arg("192k");
    cmd.arg("-ac").arg("2"); // Stereo
    cmd.arg("-ar").arg("48000"); // Standard sample rate
    // Simpler audio filter for better stability
    cmd.arg("-af").arg("aresample=async=1:first_pts=0");
    */

    // Threading optimization for AMD 7950X (16 cores, 32 threads)
    cmd.arg("-threads").arg("16"); // Use half the threads for good balance

    // Use MPEGTS format optimized for streaming
    //cmd.arg("-f").arg("mpegts");
    //cmd.arg("-mpegts_copyts").arg("0"); // Don't copy timestamps to ensure they start at 0
    //cmd.arg("-pes_payload_size").arg("0"); // Let FFmpeg decide PES payload size

    // Better timestamp handling (fflags already set above with nobuffer)
    cmd.arg("-fflags").arg("+genpts+discardcorrupt+nobuffer");
    cmd.arg("-avoid_negative_ts").arg("make_zero");
    cmd.arg("-fps_mode").arg("cfr"); // Constant frame rate for consistent timing
    cmd.arg("-start_at_zero").arg("1"); // Force timestamps to start at 0

    // GOP settings for better seeking
    cmd.arg("-g").arg("48"); // Keyframe every 2 seconds
    cmd.arg("-keyint_min").arg("24");
    cmd.arg("-sc_threshold").arg("0");

    // Remove H.264 SEI data that can cause timing issues
    // Use dump_extra to remove SEI NAL units
    cmd.arg("-bsf:v").arg("dump_extra");

    // Buffer settings for preventing audio cutouts
    cmd.arg("-max_delay").arg("500000");
    cmd.arg("-muxdelay").arg("0.1");
    cmd.arg("-muxpreload").arg("0.5");
    cmd.arg("-max_muxing_queue_size").arg("1024");

    // Output to stdout
    cmd.arg("pipe:1");
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped()); // Capture stderr for debugging

    // Log the FFmpeg command for debugging
    info!("FFmpeg command: {:?}", cmd);
    info!("HDR content detected: {}", is_hdr);
    info!("Hardware encoder: {:?}", hardware_encoder);

    // Spawn FFmpeg
    let mut child = cmd.spawn().map_err(|e| {
        error!("Failed to spawn FFmpeg: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Spawn a task to log FFmpeg stderr
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let reader = tokio::io::BufReader::new(stderr);
            let mut lines = tokio::io::AsyncBufReadExt::lines(reader);
            while let Ok(Some(line)) = lines.next_line().await {
                if line.contains("error") || line.contains("Error") {
                    error!("FFmpeg: {}", line);
                } else {
                    debug!("FFmpeg: {}", line);
                }
            }
        });
    }

    // Get stdout
    let stdout = child.stdout.take().ok_or_else(|| {
        error!("Failed to get FFmpeg stdout");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Create async stream from stdout
    let stream = tokio_util::io::ReaderStream::new(stdout);
    let body = Body::from_stream(stream);

    // Build response
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "video/mp2t")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(body)
        .unwrap())
}

pub async fn hls_segment_handler(
    State(state): State<AppState>,
    Path((id, segment)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response, StatusCode> {
    info!(
        "HLS segment request for media ID: {}, segment: {}",
        id, segment
    );

    let profile_name = params
        .get("profile")
        .cloned()
        .unwrap_or_else(|| "hdr_to_sdr_1080p".to_string());

    // Build segment path
    let segment_path = state
        .config
        .transcode_cache_dir
        .join(&id)
        .join(&profile_name)
        .join(&segment);

    // Serve the segment file
    match tokio::fs::read(&segment_path).await {
        Ok(bytes) => {
            let mut response = Response::new(bytes.into());
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("video/mp2t"),
            );
            Ok(response)
        }
        Err(e) => {
            warn!("Failed to read segment file: {}", e);
            Err(StatusCode::NOT_FOUND)
        }
    }
}

#[derive(Deserialize)]
pub struct TranscodeRequest {
    profile: Option<String>,
    tone_mapping: Option<transcoding::config::ToneMappingConfig>,
}

pub async fn start_transcode_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<TranscodeRequest>,
) -> Result<Json<Value>, StatusCode> {
    info!("Transcode request for media ID: {}", id);

    // Get media to check if it exists and is HDR
    let media = match state.db.backend().get_media(&id).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            return Ok(Json(json!({
                "status": "error",
                "error": "Media not found"
            })));
        }
        Err(e) => {
            return Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })));
        }
    };

    // Determine profile
    let profile = if let Some(profile_name) = request.profile {
        match profile_name.as_str() {
            "hdr_to_sdr_1080p" => transcoding::profiles::TranscodingProfile::hdr_to_sdr_1080p(),
            "hdr_to_sdr_4k" => transcoding::profiles::TranscodingProfile::hdr_to_sdr_4k(),
            _ => {
                return Ok(Json(json!({
                    "status": "error",
                    "error": "Unknown profile"
                })));
            }
        }
    } else if transcoding::TranscodingService::is_hdr_content(&media).await {
        transcoding::profiles::TranscodingProfile::hdr_to_sdr_1080p()
    } else {
        return Ok(Json(json!({
            "status": "error",
            "error": "Media is not HDR content"
        })));
    };

    // Start transcoding
    match state
        .transcoding_service
        .start_transcoding(&id, profile, request.tone_mapping, None)
        .await
    {
        Ok(job_id) => Ok(Json(json!({
            "status": "success",
            "job_id": job_id,
            "message": "Transcoding started"
        }))),
        Err(e) => Ok(Json(json!({
            "status": "error",
            "error": e.to_string()
        }))),
    }
}

pub async fn transcode_status_handler(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<ferrex_core::TranscodingJobResponse>, StatusCode> {
    if let Some(job) = state.transcoding_service.get_job_status(&job_id).await {
        // Use stored source metadata duration instead of re-extracting
        let duration = job.source_metadata.as_ref().map(|m| m.duration);

        if let Some(dur) = duration {
            debug!("Job {} has stored duration: {} seconds", job_id, dur);
        } else {
            debug!("Job {} has no stored duration metadata", job_id);
        }

        // Get latest progress information if available
        // For master jobs, we need to aggregate progress from variant jobs
        let progress_details = if matches!(&job.job_type, transcoding::job::JobType::Master { .. })
        {
            state
                .transcoding_service
                .get_master_job_progress(&job_id)
                .await
        } else {
            state.transcoding_service.get_job_progress(&job_id).await
        };

        // Log job type and status for debugging
        info!(
            "Job {} - Type: {:?}, Status: {:?}",
            job_id, job.job_type, job.status
        );

        // Convert server TranscodingStatus to shared TranscodingStatus
        let status = match &job.status {
            transcoding::job::TranscodingStatus::Pending => ferrex_core::TranscodingStatus::Pending,
            transcoding::job::TranscodingStatus::Queued => ferrex_core::TranscodingStatus::Queued,
            transcoding::job::TranscodingStatus::Processing { progress } => {
                ferrex_core::TranscodingStatus::Processing {
                    progress: *progress,
                }
            }
            transcoding::job::TranscodingStatus::Completed => {
                ferrex_core::TranscodingStatus::Completed
            }
            transcoding::job::TranscodingStatus::Failed { error } => {
                ferrex_core::TranscodingStatus::Failed {
                    error: error.clone(),
                }
            }
            transcoding::job::TranscodingStatus::Cancelled => {
                ferrex_core::TranscodingStatus::Cancelled
            }
        };

        // Log progress details for debugging
        if let Some(ref p) = progress_details {
            info!(
                "Progress details for job {}: status={:?}, frames={:?}/{:?}, fps={:?}",
                job_id, p.status, p.current_frame, p.total_frames, p.fps
            );
        } else {
            info!("No progress details available for job {}", job_id);
        }

        // Convert progress details if available
        let shared_progress_details = progress_details.map(|p| {
            ferrex_core::TranscodingProgressDetails {
                percentage: match &status {
                    // Use the aggregated status, not p.status
                    ferrex_core::TranscodingStatus::Processing { progress } => *progress * 100.0,
                    _ => 0.0,
                },
                time_elapsed: None, // Could calculate from job.started_at if available
                estimated_time_remaining: p.eta.map(|d| d.as_secs_f64()),
                frames_processed: p.current_frame,
                current_fps: p.fps.map(|f| f as f64),
                current_bitrate: p.bitrate.as_ref().and_then(|b| b.parse::<u64>().ok()),
            }
        });

        Ok(Json(ferrex_core::TranscodingJobResponse {
            id: job.id.clone(),
            media_id: job.media_id.clone(),
            media_path: job.media_id.clone(), // The actual file path
            profile: job.profile.name.clone(),
            status,
            created_at: job.created_at.elapsed().as_secs(),
            output_path: Some(job.output_dir.to_string_lossy().to_string()),
            playlist_path: Some(job.playlist_path.to_string_lossy().to_string()),
            error: job.error.clone(),
            progress_details: shared_progress_details,
            duration,
        }))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// Start adaptive bitrate transcoding for a media file
/// This creates multiple quality variants for adaptive streaming
pub async fn start_adaptive_transcode_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    info!("Adaptive transcode request for media ID: {}", id);

    // Decode the percent-encoded ID
    let decoded_id = urlencoding::decode(&id).map_err(|e| {
        error!("Failed to decode media ID: {}", e);
        StatusCode::BAD_REQUEST
    })?;

    match state
        .transcoding_service
        .start_adaptive_transcoding(&decoded_id, None)
        .await
    {
        Ok(job_id) => Ok(Json(json!({
            "status": "success",
            "master_job_id": job_id,
            "message": "Adaptive bitrate transcoding started",
            "info": "Use /transcode/{id}/master.m3u8 to get the master playlist once ready"
        }))),
        Err(e) => Ok(Json(json!({
            "status": "error",
            "error": e.to_string()
        }))),
    }
}

/// Get a specific segment on-the-fly
/// This endpoint generates segments on demand for true streaming
pub async fn get_segment_handler(
    State(state): State<AppState>,
    Path((job_id, segment_number)): Path<(String, u32)>,
) -> Result<Response<Body>, StatusCode> {
    match state
        .transcoding_service
        .get_segment(&job_id, segment_number)
        .await
    {
        Ok(segment_path) => {
            // Stream the segment file
            match tokio::fs::File::open(&segment_path).await {
                Ok(file) => {
                    let stream = ReaderStream::new(file);
                    let body = Body::from_stream(stream);

                    Ok(Response::builder()
                        .header(header::CONTENT_TYPE, "video/MP2T")
                        .header(header::CACHE_CONTROL, "public, max-age=3600")
                        .body(body)
                        .unwrap())
                }
                Err(_) => Err(StatusCode::NOT_FOUND),
            }
        }
        Err(e) => {
            error!("Failed to get segment: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Get master playlist for adaptive bitrate streaming
pub async fn get_master_playlist_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response<Body>, StatusCode> {
    info!("Master playlist request for media ID: {}", id);

    // Decode the percent-encoded ID
    let decoded_id = urlencoding::decode(&id).map_err(|e| {
        error!("Failed to decode media ID: {}", e);
        StatusCode::BAD_REQUEST
    })?;

    match state
        .transcoding_service
        .get_master_playlist(&decoded_id)
        .await
    {
        Some(playlist_path) => {
            info!("Found master playlist at: {:?}", playlist_path);
            match tokio::fs::read_to_string(&playlist_path).await {
                Ok(content) => {
                    info!(
                        "Successfully read master playlist, size: {} bytes",
                        content.len()
                    );
                    Ok(Response::builder()
                        .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
                        .header(header::CACHE_CONTROL, "no-cache")
                        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                        .body(Body::from(content))
                        .unwrap())
                }
                Err(e) => {
                    warn!("Failed to read master playlist: {}", e);
                    Err(StatusCode::NOT_FOUND)
                }
            }
        }
        None => {
            warn!("Master playlist not found for media ID: {}", id);
            // Check if variant directories exist and generate master playlist on the fly
            let cache_dir = state.config.transcode_cache_dir.join(&id);
            match tokio::fs::read_dir(&cache_dir).await {
                Ok(mut entries) => {
                    let mut variants = Vec::new();

                    // Scan for variant directories
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        if let Ok(file_type) = entry.file_type().await {
                            if file_type.is_dir() {
                                let name = entry.file_name();
                                if let Some(name_str) = name.to_str() {
                                    if name_str.starts_with("adaptive_") {
                                        variants.push(name_str.to_string());
                                    }
                                }
                            }
                        }
                    }

                    if !variants.is_empty() {
                        info!(
                            "Found {} variants, generating master playlist on the fly",
                            variants.len()
                        );

                        // Generate master playlist content
                        let mut master_content = "#EXTM3U\n#EXT-X-VERSION:3\n\n".to_string();

                        // Sort variants by quality
                        variants.sort();

                        for variant_name in variants {
                            // Extract resolution from variant name (e.g., "adaptive_720p" -> "720p")
                            let quality = variant_name.trim_start_matches("adaptive_");

                            let (width, height, bandwidth) = match quality {
                                "360p" => (640, 360, 1000000),
                                "480p" => (854, 480, 2000000),
                                "720p" => (1280, 720, 3000000),
                                "1080p" => (1920, 1080, 5000000),
                                "4k" => (3840, 2160, 20000000),
                                "original" => {
                                    // For original quality, we should use actual source dimensions
                                    // For now, use high values that will be sorted last
                                    (7680, 4320, 50000000) // 8K placeholder with very high bitrate
                                }
                                _ => continue,
                            };

                            // Check if playlist exists in this variant
                            let playlist_path = cache_dir.join(&variant_name).join("playlist.m3u8");
                            if tokio::fs::metadata(&playlist_path).await.is_ok() {
                                master_content.push_str(&format!(
                                    "#EXT-X-STREAM-INF:BANDWIDTH={},RESOLUTION={}x{}\nvariant/{}/playlist.m3u8\n\n",
                                    bandwidth, width, height, variant_name
                                ));
                            }
                        }

                        // Save the generated master playlist for future use
                        let master_path = cache_dir.join("master.m3u8");
                        if let Err(e) = tokio::fs::write(&master_path, &master_content).await {
                            warn!("Failed to save generated master playlist: {}", e);
                        }

                        Ok(Response::builder()
                            .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
                            .header(header::CACHE_CONTROL, "no-cache")
                            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                            .body(Body::from(master_content))
                            .unwrap())
                    } else {
                        warn!("No variant directories found for media ID: {}", id);
                        Err(StatusCode::NOT_FOUND)
                    }
                }
                Err(e) => {
                    warn!("Failed to read cache directory: {}", e);
                    Err(StatusCode::NOT_FOUND)
                }
            }
        }
    }
}

/// Get variant playlist for a specific quality profile
pub async fn get_variant_playlist_handler(
    State(state): State<AppState>,
    Path((id, profile)): Path<(String, String)>,
) -> Result<Response<Body>, StatusCode> {
    match state
        .transcoding_service
        .get_playlist_url(&id, &profile)
        .await
    {
        Some(playlist_path) => {
            match tokio::fs::read_to_string(&playlist_path).await {
                Ok(mut content) => {
                    // FFmpeg generates segment files with relative paths like "segment_000.ts"
                    // We need to update these to include the full path for our server
                    // Replace segment references to use our segment endpoint
                    content = content.replace(
                        "segment_",
                        &format!("/transcode/{}/variant/{}/segment_", id, profile),
                    );

                    Ok(Response::builder()
                        .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
                        .header(header::CACHE_CONTROL, "no-cache")
                        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                        .body(Body::from(content))
                        .unwrap())
                }
                Err(_) => Err(StatusCode::NOT_FOUND),
            }
        }
        None => {
            // Variant doesn't exist - check if we should start on-demand transcoding
            info!(
                "Variant {} not found for media {}, checking for on-demand transcoding",
                profile, id
            );

            // Extract quality from profile (e.g., "adaptive_720p" -> "720p")
            if profile.starts_with("adaptive_") {
                let quality = profile.trim_start_matches("adaptive_");

                // Check if this is a valid quality we support
                let valid_qualities = ["360p", "480p", "720p", "1080p", "4k", "original"];
                if valid_qualities.contains(&quality) {
                    // Get media info to create proper transcoding profile
                    match state.db.backend().get_media(&id).await {
                        Ok(Some(media)) => {
                            // Create transcoding profile for this specific variant
                            let transcode_profile = match quality {
                                "360p" => transcoding::profiles::TranscodingProfile {
                                    name: profile.clone(),
                                    video_codec: "libx264".to_string(),
                                    audio_codec: "copy".to_string(), // Pass through original audio
                                    video_bitrate: "800k".to_string(),
                                    audio_bitrate: "0".to_string(), // Not used with copy codec
                                    resolution: Some("640x360".to_string()),
                                    preset: "fast".to_string(),
                                    apply_tone_mapping: false,
                                },
                                "480p" => transcoding::profiles::TranscodingProfile {
                                    name: profile.clone(),
                                    video_codec: "libx264".to_string(),
                                    audio_codec: "copy".to_string(), // Pass through original audio
                                    video_bitrate: "2M".to_string(),
                                    audio_bitrate: "0".to_string(), // Not used with copy codec
                                    resolution: Some("854x480".to_string()),
                                    preset: "fast".to_string(),
                                    apply_tone_mapping: false,
                                },
                                "1080p" => transcoding::profiles::TranscodingProfile {
                                    name: profile.clone(),
                                    video_codec: "libx264".to_string(),
                                    audio_codec: "copy".to_string(), // Pass through original audio
                                    video_bitrate: "8M".to_string(),
                                    audio_bitrate: "0".to_string(), // Not used with copy codec
                                    resolution: Some("1920x1080".to_string()),
                                    preset: "fast".to_string(),
                                    apply_tone_mapping: false,
                                },
                                _ => {
                                    warn!("Unsupported on-demand quality: {}", quality);
                                    return Err(StatusCode::NOT_FOUND);
                                }
                            };

                            // Start on-demand transcoding
                            info!(
                                "Starting on-demand transcoding for {} variant of media {}",
                                quality, id
                            );
                            match state
                                .transcoding_service
                                .start_transcoding(
                                    &id,
                                    transcode_profile,
                                    None,
                                    Some(transcoding::job::JobPriority::High),
                                )
                                .await
                            {
                                Ok(job_id) => {
                                    info!(
                                        "Started on-demand transcoding job {} for variant {}",
                                        job_id, profile
                                    );
                                    // Return 202 Accepted to indicate transcoding has started
                                    Ok(Response::builder()
                                        .status(StatusCode::ACCEPTED)
                                        .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
                                        .header(header::CACHE_CONTROL, "no-cache")
                                        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                                        .header("X-Transcode-Status", "started")
                                        .header("X-Transcode-Job-Id", job_id)
                                        .body(Body::from("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-PLAYLIST-TYPE:EVENT\n# Transcoding in progress..."))
                                        .unwrap())
                                }
                                Err(e) => {
                                    warn!("Failed to start on-demand transcoding: {}", e);
                                    Err(StatusCode::SERVICE_UNAVAILABLE)
                                }
                            }
                        }
                        _ => {
                            warn!("Media {} not found for on-demand transcoding", id);
                            Err(StatusCode::NOT_FOUND)
                        }
                    }
                } else {
                    Err(StatusCode::NOT_FOUND)
                }
            } else {
                Err(StatusCode::NOT_FOUND)
            }
        }
    }
}

/// Serve variant segment files
pub async fn get_variant_segment_handler(
    State(state): State<AppState>,
    Path((id, profile, segment)): Path<(String, String, String)>,
) -> Result<Response<Body>, StatusCode> {
    let segment_path = state
        .config
        .transcode_cache_dir
        .join(&id)
        .join(&profile)
        .join(&segment);

    match tokio::fs::File::open(&segment_path).await {
        Ok(file) => {
            let stream = ReaderStream::new(file);
            let body = Body::from_stream(stream);

            Ok(Response::builder()
                .header(header::CONTENT_TYPE, "video/MP2T")
                .header(header::CACHE_CONTROL, "public, max-age=3600")
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .body(body)
                .unwrap())
        }
        Err(_) => Err(StatusCode::NOT_FOUND),
    }
}

/// Cancel a transcoding job
pub async fn cancel_transcode_handler(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    match state.transcoding_service.cancel_job(&job_id).await {
        Ok(()) => Ok(Json(json!({
            "status": "success",
            "message": "Job cancelled"
        }))),
        Err(e) => Ok(Json(json!({
            "status": "error",
            "error": e.to_string()
        }))),
    }
}

/// List available transcoding profiles
pub async fn list_transcode_profiles_handler(
    State(_state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    // Return available profiles
    Ok(Json(json!({
        "status": "success",
        "profiles": [
            {
                "name": "hdr_to_sdr_1080p",
                "description": "HDR to SDR conversion at 1080p",
                "video_codec": "libx264",
                "audio_codec": "aac",
                "resolution": "1920x1080"
            },
            {
                "name": "hdr_to_sdr_4k",
                "description": "HDR to SDR conversion at 4K",
                "video_codec": "libx265",
                "audio_codec": "aac",
                "resolution": "3840x2160"
            },
            {
                "name": "adaptive",
                "description": "Adaptive bitrate with multiple quality variants",
                "variants": ["360p", "480p", "720p", "1080p", "4k"]
            }
        ]
    })))
}

/// Get transcoding cache statistics
pub async fn transcode_cache_stats_handler(
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    match state.transcoding_service.get_cache_stats().await {
        Ok(stats) => Ok(Json(json!({
            "status": "success",
            "cache_stats": {
                "total_size_mb": stats.total_size_mb,
                "file_count": stats.file_count,
                "media_count": stats.media_count,
                "oldest_file_age_days": stats.oldest_file_age_days
            }
        }))),
        Err(e) => Ok(Json(json!({
            "status": "error",
            "error": e.to_string()
        }))),
    }
}

/// Clear transcoding cache for a specific media file
pub async fn clear_transcode_cache_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    info!("Clear cache request for media ID: {}", id);

    // Clear all cached variants for this media
    let cache_dir = state.config.transcode_cache_dir.join(&id);
    if cache_dir.exists() {
        match tokio::fs::remove_dir_all(&cache_dir).await {
            Ok(()) => {
                info!("Cleared transcode cache for media: {}", id);
                Ok(Json(json!({
                    "status": "success",
                    "message": "Cache cleared successfully"
                })))
            }
            Err(e) => {
                error!("Failed to clear cache: {}", e);
                Ok(Json(json!({
                    "status": "error",
                    "error": e.to_string()
                })))
            }
        }
    } else {
        Ok(Json(json!({
            "status": "success",
            "message": "No cache to clear"
        })))
    }
}
