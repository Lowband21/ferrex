use std::path::Path;
use std::process::Command;

use crate::error::{MediaError, Result};

#[derive(Debug, Default)]
pub struct HdrInfo {
    pub bit_depth: Option<u32>,
    pub color_primaries: Option<String>,
    pub color_transfer: Option<String>,
    pub color_space: Option<String>,
}

#[derive(Debug)]
pub struct HdrMetadataExtractor;

impl HdrMetadataExtractor {
    /// Extract HDR metadata using ffprobe command
    pub fn extract_hdr_metadata<P: AsRef<Path>>(file_path: P) -> Result<HdrInfo> {
        let file_path = file_path.as_ref();
        let mut hdr_info = HdrInfo::default();

        // Run ffprobe to get stream information in JSON format
        let output = Command::new("ffprobe")
            .args([
                "-v",
                "quiet",
                "-print_format",
                "json",
                "-show_streams",
                "-select_streams",
                "v:0", // First video stream
                file_path
                    .to_str()
                    .ok_or_else(|| MediaError::InvalidMedia("Invalid file path".to_string()))?,
            ])
            .output()
            .map_err(MediaError::Io)?;

        if !output.status.success() {
            return Err(MediaError::InvalidMedia("ffprobe failed".to_string()));
        }

        // Parse JSON output
        let json_str = std::str::from_utf8(&output.stdout)
            .map_err(|_| MediaError::InvalidMedia("Invalid ffprobe output".to_string()))?;

        let json: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|_| MediaError::InvalidMedia("Failed to parse ffprobe JSON".to_string()))?;

        // Extract stream information
        if let Some(streams) = json["streams"].as_array()
            && let Some(stream) = streams.first()
        {
            // Bit depth from pix_fmt (e.g., yuv420p10le -> 10 bit)
            if let Some(pix_fmt) = stream["pix_fmt"].as_str() {
                if pix_fmt.contains("p10") || pix_fmt.contains("10le") || pix_fmt.contains("10be") {
                    hdr_info.bit_depth = Some(10);
                } else if pix_fmt.contains("p12")
                    || pix_fmt.contains("12le")
                    || pix_fmt.contains("12be")
                {
                    hdr_info.bit_depth = Some(12);
                } else {
                    hdr_info.bit_depth = Some(8);
                }
            }

            // Color information
            if let Some(color_primaries) = stream["color_primaries"].as_str() {
                hdr_info.color_primaries = Some(color_primaries.to_string());
            }

            if let Some(color_transfer) = stream["color_transfer"].as_str() {
                hdr_info.color_transfer = Some(color_transfer.to_string());
            }

            if let Some(color_space) = stream["color_space"].as_str() {
                hdr_info.color_space = Some(color_space.to_string());
            }

            // Also check side_data_list for HDR metadata
            if let Some(side_data_list) = stream["side_data_list"].as_array() {
                for side_data in side_data_list {
                    if let Some(side_data_type) = side_data["side_data_type"].as_str()
                        && (side_data_type.contains("Mastering display metadata")
                            || side_data_type.contains("Content light level metadata"))
                    {
                        // This indicates HDR content
                        if hdr_info.color_transfer.is_none() {
                            hdr_info.color_transfer = Some("smpte2084".to_string());
                        }
                        if hdr_info.color_primaries.is_none() {
                            hdr_info.color_primaries = Some("bt2020".to_string());
                        }
                    }
                }
            }
        }

        Ok(hdr_info)
    }
}
