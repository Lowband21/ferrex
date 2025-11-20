use crate::error::Result;
use ffmpeg_next as ffmpeg;
use std::path::Path;
use tracing::{debug, info};

use crate::error::MediaError;

#[derive(Debug, Default)]
pub struct TechnicalMetadata {
    pub duration: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub bitrate: Option<u64>,
    pub framerate: Option<f64>,

    // HDR metadata
    pub color_primaries: Option<String>,
    pub color_transfer: Option<String>,
    pub color_space: Option<String>,
    pub bit_depth: Option<u32>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct TechnicalMetadataExtractor {
    initialized: bool,
}

impl TechnicalMetadataExtractor {
    pub fn new() -> Self {
        Self { initialized: false }
    }

    /// Initialize FFmpeg (call once per application)
    pub fn init(&mut self) -> Result<()> {
        if !self.initialized {
            ffmpeg::init().map_err(MediaError::Ffmpeg)?;
            self.initialized = true;
            info!("FFmpeg initialized successfully");
        }
        Ok(())
    }

    /// Extract technical metadata using FFmpeg
    pub fn extract_metadata<P: AsRef<Path>>(
        &mut self,
        file_path: P,
    ) -> Result<TechnicalMetadata> {
        let file_path = file_path.as_ref();

        // Ensure FFmpeg is initialized
        self.init()?;

        debug!("Opening file with FFmpeg: {}", file_path.display());

        let input =
            ffmpeg::format::input(file_path).map_err(MediaError::Ffmpeg)?;

        let mut technical = TechnicalMetadata::default();

        // Get duration
        if input.duration() != ffmpeg::ffi::AV_NOPTS_VALUE {
            technical.duration = Some(
                input.duration() as f64 / ffmpeg::ffi::AV_TIME_BASE as f64,
            );
        }

        // Get bitrate
        if input.bit_rate() > 0 {
            technical.bitrate = Some(input.bit_rate() as u64);
        }

        // Find video and audio streams - prioritize main streams over thumbnails
        let mut best_video_stream = None;
        let mut best_video_width = 0;
        let mut best_audio_stream = None;

        for (i, stream) in input.streams().enumerate() {
            let codec = ffmpeg::codec::context::Context::from_parameters(
                stream.parameters(),
            )
            .map_err(MediaError::Ffmpeg)?;

            match codec.medium() {
                ffmpeg::media::Type::Video => {
                    if let Ok(video) = codec.decoder().video() {
                        let width = video.width();
                        let height = video.height();
                        let codec_name = video
                            .codec()
                            .map(|c| c.name().to_string())
                            .unwrap_or_default();

                        debug!(
                            "Found video stream {} - {}x{} {}",
                            i, width, height, codec_name
                        );

                        // Skip thumbnail streams (MJPEG, small dimensions, attached pictures)
                        let is_thumbnail = codec_name == "mjpeg"
                            || width < 400
                            || height < 400
                            || stream
                                .disposition()
                                .contains(ffmpeg::format::stream::Disposition::ATTACHED_PIC);

                        // Use this stream if it's not a thumbnail and has better resolution
                        if !is_thumbnail && width > best_video_width {
                            best_video_width = width;
                            best_video_stream = Some((stream, video));
                        }
                    }
                }
                ffmpeg::media::Type::Audio => {
                    debug!("Found audio stream {}", i);

                    if let Ok(audio) = codec.decoder().audio() {
                        // Use first audio stream (could be improved to select best quality)
                        if best_audio_stream.is_none() {
                            best_audio_stream = Some(audio);
                        }
                    }
                }
                _ => {
                    debug!("Found other stream type: {:?}", codec.medium());
                }
            }
        }

        // Extract metadata from the best video stream
        if let Some((stream, video)) = best_video_stream {
            technical.width = Some(video.width());
            technical.height = Some(video.height());

            // Get frame rate
            let frame_rate = stream.avg_frame_rate();
            if frame_rate.denominator() != 0 {
                technical.framerate = Some(
                    frame_rate.numerator() as f64
                        / frame_rate.denominator() as f64,
                );
            }

            // Get codec name
            if let Some(codec) = video.codec() {
                technical.video_codec = Some(codec.name().to_string());
            }

            debug!(
                "Selected video stream: {}x{} {} {} fps",
                video.width(),
                video.height(),
                technical
                    .video_codec
                    .as_ref()
                    .unwrap_or(&"unknown".to_string()),
                technical.framerate.unwrap_or(0.0)
            );
        }

        // Extract metadata from the best audio stream
        if let Some(audio) = best_audio_stream
            && let Some(codec) = audio.codec()
        {
            technical.audio_codec = Some(codec.name().to_string());
        }

        debug!("Technical metadata extracted: {:?}", technical);
        Ok(technical)
    }
}
