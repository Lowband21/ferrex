use super::{
    filename_parser::FilenameParser, hdr_metadata::HdrMetadataExtractor,
    technical_metadata::TechnicalMetadataExtractor,
};
use crate::{LibraryType, MediaError, MediaFileMetadata, Result};
use std::path::Path;
use tracing::{info, warn};

pub struct MetadataExtractor {
    /// Library context for type-specific parsing
    library_type: Option<LibraryType>,
    /// Technical metadata extractor
    technical_extractor: TechnicalMetadataExtractor,
    /// Filename parser
    filename_parser: FilenameParser,
}

impl Default for MetadataExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl MetadataExtractor {
    pub fn new() -> Self {
        Self {
            library_type: None,
            technical_extractor: TechnicalMetadataExtractor::new(),
            filename_parser: FilenameParser::new(),
        }
    }

    /// Create a new extractor with library context
    pub fn with_library_type(library_type: LibraryType) -> Self {
        Self {
            library_type: Some(library_type),
            technical_extractor: TechnicalMetadataExtractor::new(),
            filename_parser: FilenameParser::with_library_type(library_type),
        }
    }

    /// Set library type for context-aware parsing
    pub fn set_library_type(&mut self, library_type: Option<LibraryType>) {
        self.library_type = library_type;
        self.filename_parser.set_library_type(library_type);
    }

    /// Check if a file is likely a sample based on duration and file size
    pub fn is_sample(&self, metadata: &MediaFileMetadata) -> bool {
        // Define thresholds for sample detection
        const MAX_SAMPLE_DURATION_SECONDS: f64 = 180.0; // 3 minutes
        const MAX_SAMPLE_SIZE_BYTES: u64 = 50 * 1024 * 1024; // 50 MB
        const MIN_SAMPLE_DURATION_SECONDS: f64 = 10.0; // 10 seconds minimum for samples

        // Check duration threshold
        if let Some(duration) = metadata.duration {
            // Very short files (under 10 seconds) are likely not samples but other media
            if duration < MIN_SAMPLE_DURATION_SECONDS {
                return false;
            }

            // Files between 10 seconds and 3 minutes are likely samples
            if duration <= MAX_SAMPLE_DURATION_SECONDS {
                return true;
            }
        }

        // Check file size threshold for very small files
        if metadata.file_size <= MAX_SAMPLE_SIZE_BYTES {
            // If we have duration info and it's reasonable, don't filter by size alone
            if let Some(duration) = metadata.duration {
                // Allow small files if they're short but not sample-short
                if duration > MAX_SAMPLE_DURATION_SECONDS {
                    return false;
                }
            } else {
                // No duration info and small file - likely a sample
                return true;
            }
        }

        false
    }

    /// Extract complete metadata from a media file
    pub fn extract_metadata<P: AsRef<Path>>(&mut self, file_path: P) -> Result<MediaFileMetadata> {
        let file_path = file_path.as_ref();

        info!("Extracting metadata from: {}", file_path.display());

        // Extract technical metadata with FFmpeg
        let technical_metadata = self.technical_extractor.extract_metadata(file_path)?;

        // Parse filename for show/episode info
        let parsed_info = self.filename_parser.parse_filename_with_type(file_path);

        // Get file size
        let file_size = file_path.metadata().map_err(MediaError::Io)?.len();

        // Extract HDR metadata if available
        let hdr_metadata = match HdrMetadataExtractor::extract_hdr_metadata(file_path) {
            Ok(hdr_info) => {
                info!("HDR metadata extracted via ffprobe: {:?}", hdr_info);
                hdr_info
            }
            Err(e) => {
                warn!("Failed to extract HDR metadata via ffprobe: {}", e);
                // DO NOT guess HDR metadata - this causes false positives
                // If we can't extract proper metadata, assume SDR
                super::hdr_metadata::HdrInfo {
                    bit_depth: Some(8), // Default to 8-bit
                    color_primaries: None,
                    color_transfer: None,
                    color_space: None,
                }
            }
        };

        Ok(MediaFileMetadata {
            duration: technical_metadata.duration,
            width: technical_metadata.width,
            height: technical_metadata.height,
            video_codec: technical_metadata.video_codec,
            audio_codec: technical_metadata.audio_codec,
            bitrate: technical_metadata.bitrate,
            framerate: technical_metadata.framerate,
            file_size,
            // HDR metadata
            color_primaries: hdr_metadata
                .color_primaries
                .or(technical_metadata.color_primaries),
            color_transfer: hdr_metadata
                .color_transfer
                .or(technical_metadata.color_transfer),
            color_space: hdr_metadata.color_space.or(technical_metadata.color_space),
            bit_depth: hdr_metadata.bit_depth.or(technical_metadata.bit_depth),
            parsed_info,
        })
    }
}
