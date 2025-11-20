use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscodingProfile {
    pub name: String,
    pub video_codec: String,
    pub audio_codec: String,
    pub video_bitrate: String,
    pub audio_bitrate: String,
    pub resolution: Option<String>,
    pub preset: String,
    pub apply_tone_mapping: bool,
}

impl TranscodingProfile {
    /// Default HDR to SDR profile for 1080p
    pub fn hdr_to_sdr_1080p() -> Self {
        Self {
            name: "hdr_to_sdr_1080p".to_string(),
            video_codec: "libx264".to_string(),
            audio_codec: "copy".to_string(), // Pass through original audio
            video_bitrate: "8M".to_string(),
            audio_bitrate: "0".to_string(), // Not used with copy codec
            resolution: Some("1920x1080".to_string()),
            preset: "fast".to_string(),
            apply_tone_mapping: true,
        }
    }

    /// Default HDR to SDR profile for 4K
    pub fn hdr_to_sdr_4k() -> Self {
        Self {
            name: "hdr_to_sdr_4k".to_string(),
            video_codec: "libx265".to_string(),
            audio_codec: "copy".to_string(), // Pass through original audio
            video_bitrate: "20M".to_string(),
            audio_bitrate: "0".to_string(), // Not used with copy codec
            resolution: None, // Keep original resolution
            preset: "veryfast".to_string(),
            apply_tone_mapping: true,
        }
    }
    
    /// HDR to SDR profile that preserves original resolution and quality
    pub fn hdr_to_sdr_original() -> Self {
        Self {
            name: "hdr_to_sdr_original".to_string(),
            video_codec: "libx265".to_string(),
            audio_codec: "copy".to_string(), // Passthrough audio
            video_bitrate: "30M".to_string(), // High bitrate for quality preservation
            audio_bitrate: "0".to_string(), // Not used with copy codec
            resolution: None, // Keep original resolution
            preset: "fast".to_string(), // Better quality than veryfast
            apply_tone_mapping: true,
        }
    }
    
    /// Minimal transcoding profile for SDR content (passthrough where possible)
    pub fn minimal_transcode() -> Self {
        Self {
            name: "minimal_transcode".to_string(),
            video_codec: "copy".to_string(), // Try to passthrough video
            audio_codec: "copy".to_string(), // Passthrough audio
            video_bitrate: "0".to_string(), // Not used with copy codec
            audio_bitrate: "0".to_string(), // Not used with copy codec
            resolution: None, // Keep original
            preset: "".to_string(), // Not used with copy codec
            apply_tone_mapping: false,
        }
    }
}

/// Adaptive bitrate profile variants
#[derive(Debug, Clone)]
pub struct AdaptiveBitrateProfile {
    pub name: String,
    pub variants: Vec<ProfileVariant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileVariant {
    pub name: String,
    pub resolution: String,
    pub video_bitrate: String,
    pub audio_bitrate: String,
    pub video_codec: String,
    pub audio_codec: String,
    pub preset: String,
    pub bandwidth: u64, // in bits per second
}

impl AdaptiveBitrateProfile {
    /// Generate adaptive bitrate profiles for a given source resolution
    pub fn generate_for_resolution(width: u32, height: u32) -> Self {
        let mut variants = Vec::new();
        
        // Original quality variant - always include the source resolution
        // This ensures we preserve quality, especially important for HDR tone mapping
        let original_bitrate = match (width, height) {
            (w, h) if w >= 3840 && h >= 2160 => "30M", // 4K
            (w, h) if w >= 2560 && h >= 1440 => "20M", // 1440p
            (w, h) if w >= 1920 && h >= 1080 => "15M", // 1080p
            _ => "10M", // 720p and below
        };
        
        variants.push(ProfileVariant {
            name: "original".to_string(),
            resolution: format!("{}x{}", width, height),
            video_bitrate: original_bitrate.to_string(),
            audio_bitrate: "0".to_string(), // Not used with copy codec
            video_codec: "libx265".to_string(),
            audio_codec: "copy".to_string(), // Pass through original audio
            preset: "fast".to_string(), // Better quality than veryfast
            bandwidth: match original_bitrate {
                "30M" => 30_400_000,
                "20M" => 20_400_000,
                "15M" => 15_400_000,
                _ => 10_400_000,
            },
        });

        // 4K variant (if source is 4K but different from original)
        if width >= 3840 && height >= 2160 && !(width == 3840 && height == 2160) {
            variants.push(ProfileVariant {
                name: "4k".to_string(),
                resolution: "3840x2160".to_string(),
                video_bitrate: "20M".to_string(),
                audio_bitrate: "0".to_string(), // Not used with copy codec
                video_codec: "libx265".to_string(),
                audio_codec: "copy".to_string(), // Pass through original audio
                preset: "veryfast".to_string(),
                bandwidth: 20_500_000,
            });
        }

        // 1080p variant
        if width >= 1920 && height >= 1080 {
            variants.push(ProfileVariant {
                name: "1080p".to_string(),
                resolution: "1920x1080".to_string(),
                video_bitrate: "8M".to_string(),
                audio_bitrate: "0".to_string(), // Not used with copy codec
                video_codec: "libx264".to_string(),
                audio_codec: "copy".to_string(), // Pass through original audio
                preset: "fast".to_string(),
                bandwidth: 8_200_000,
            });
        }

        // 720p variant
        if width >= 1280 && height >= 720 {
            variants.push(ProfileVariant {
                name: "720p".to_string(),
                resolution: "1280x720".to_string(),
                video_bitrate: "4M".to_string(),
                audio_bitrate: "0".to_string(), // Not used with copy codec
                video_codec: "libx264".to_string(),
                audio_codec: "copy".to_string(), // Pass through original audio
                preset: "fast".to_string(),
                bandwidth: 4_100_000,
            });
        }

        // 480p variant
        variants.push(ProfileVariant {
            name: "480p".to_string(),
            resolution: "854x480".to_string(),
            video_bitrate: "2M".to_string(),
            audio_bitrate: "0".to_string(), // Not used with copy codec
            video_codec: "libx264".to_string(),
            audio_codec: "copy".to_string(), // Pass through original audio
            preset: "fast".to_string(),
            bandwidth: 2_100_000,
        });

        // 360p variant (low bandwidth)
        variants.push(ProfileVariant {
            name: "360p".to_string(),
            resolution: "640x360".to_string(),
            video_bitrate: "800k".to_string(),
            audio_bitrate: "0".to_string(), // Not used with copy codec
            video_codec: "libx264".to_string(),
            audio_codec: "copy".to_string(), // Pass through original audio
            preset: "fast".to_string(),
            bandwidth: 865_000,
        });

        Self {
            name: "adaptive".to_string(),
            variants,
        }
    }
}

/// Profile selector based on client capabilities
pub struct ProfileSelector;

impl ProfileSelector {
    /// Select appropriate profile based on client bandwidth and device capabilities
    pub fn select_profile<'a>(
        available_bandwidth: u64,
        device_type: &str,
        max_resolution: Option<(u32, u32)>,
        profiles: &'a [ProfileVariant],
    ) -> Option<&'a ProfileVariant> {
        let mut suitable_profiles: Vec<&ProfileVariant> = profiles
            .iter()
            .filter(|p| p.bandwidth <= available_bandwidth)
            .collect();

        // Filter by max resolution if specified
        if let Some((max_width, max_height)) = max_resolution {
            suitable_profiles.retain(|p| {
                if let Some((width, height)) = parse_resolution(&p.resolution) {
                    width <= max_width && height <= max_height
                } else {
                    true
                }
            });
        }

        // Sort by bandwidth (highest first)
        suitable_profiles.sort_by(|a, b| b.bandwidth.cmp(&a.bandwidth));

        suitable_profiles.into_iter().next()
    }
}

fn parse_resolution(resolution: &str) -> Option<(u32, u32)> {
    let parts: Vec<&str> = resolution.split('x').collect();
    if parts.len() == 2 {
        if let (Ok(width), Ok(height)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
            return Some((width, height));
        }
    }
    None
}