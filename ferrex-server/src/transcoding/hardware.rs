use anyhow::{Context, Result};
use std::process::Command as StdCommand;
use tokio::process::Command;
use tracing::{debug, info};

#[derive(Debug, Clone)]
pub struct HardwareEncoder {
    pub name: String,
    pub encoder_type: HardwareEncoderType,
    pub supported_codecs: Vec<String>,
    pub max_streams: usize,
    pub available: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardwareEncoderType {
    Vaapi,
    Nvenc,
    Qsv,
    VideoToolbox,
    Amf,
}

impl HardwareEncoderType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Vaapi => "vaapi",
            Self::Nvenc => "nvenc",
            Self::Qsv => "qsv",
            Self::VideoToolbox => "videotoolbox",
            Self::Amf => "amf",
        }
    }

    pub fn ffmpeg_hwaccel(&self) -> &'static str {
        match self {
            Self::Vaapi => "vaapi",
            Self::Nvenc => "cuda",
            Self::Qsv => "qsv",
            Self::VideoToolbox => "videotoolbox",
            Self::Amf => "d3d11va",
        }
    }
}

pub struct HardwareDetector {
    ffmpeg_path: String,
}

impl HardwareDetector {
    pub fn new(ffmpeg_path: String) -> Self {
        Self { ffmpeg_path }
    }

    /// Detect all available hardware encoders
    pub async fn detect_hardware_encoders(&self) -> Result<Vec<HardwareEncoder>> {
        let mut encoders = Vec::new();

        // Check VAAPI (Linux)
        if let Ok(vaapi) = self.check_vaapi().await {
            if vaapi.available {
                encoders.push(vaapi);
            }
        }

        // Check NVENC (NVIDIA)
        if let Ok(nvenc) = self.check_nvenc().await {
            if nvenc.available {
                encoders.push(nvenc);
            }
        }

        // Check QSV (Intel)
        if let Ok(qsv) = self.check_qsv().await {
            if qsv.available {
                encoders.push(qsv);
            }
        }

        // Check VideoToolbox (macOS)
        #[cfg(target_os = "macos")]
        if let Ok(vt) = self.check_videotoolbox().await {
            if vt.available {
                encoders.push(vt);
            }
        }

        // Check AMF (AMD)
        #[cfg(target_os = "windows")]
        if let Ok(amf) = self.check_amf().await {
            if amf.available {
                encoders.push(amf);
            }
        }

        info!("Detected {} hardware encoders", encoders.len());
        for encoder in &encoders {
            info!("  - {} ({})", encoder.name, encoder.encoder_type.as_str());
        }

        Ok(encoders)
    }

    /// Check for VAAPI support
    async fn check_vaapi(&self) -> Result<HardwareEncoder> {
        debug!("Checking for VAAPI support");

        // Check if vainfo exists and works
        let vainfo = StdCommand::new("vainfo")
            .output()
            .ok()
            .filter(|output| output.status.success());

        if vainfo.is_none() {
            return Ok(HardwareEncoder {
                name: "VAAPI".to_string(),
                encoder_type: HardwareEncoderType::Vaapi,
                supported_codecs: vec![],
                max_streams: 0,
                available: false,
            });
        }

        // Check FFmpeg VAAPI encoders
        let mut supported_codecs = Vec::new();

        let encoders = &[
            ("h264_vaapi", "h264"),
            ("hevc_vaapi", "h265"),
            ("vp9_vaapi", "vp9"),
            ("av1_vaapi", "av1"),
        ];

        for (encoder, codec) in encoders {
            if self.check_encoder(encoder).await? {
                supported_codecs.push(codec.to_string());
            }
        }

        Ok(HardwareEncoder {
            name: "VAAPI".to_string(),
            encoder_type: HardwareEncoderType::Vaapi,
            supported_codecs: supported_codecs.clone(),
            max_streams: 4, // Conservative default
            available: !supported_codecs.is_empty(),
        })
    }

    /// Check for NVENC support
    async fn check_nvenc(&self) -> Result<HardwareEncoder> {
        debug!("Checking for NVENC support");

        // Check if nvidia-smi exists
        let nvidia_smi = StdCommand::new("nvidia-smi")
            .arg("--query-gpu=name")
            .arg("--format=csv,noheader")
            .output()
            .ok()
            .filter(|output| output.status.success());

        if nvidia_smi.is_none() {
            return Ok(HardwareEncoder {
                name: "NVENC".to_string(),
                encoder_type: HardwareEncoderType::Nvenc,
                supported_codecs: vec![],
                max_streams: 0,
                available: false,
            });
        }

        let mut supported_codecs = Vec::new();

        let encoders = &[
            ("h264_nvenc", "h264"),
            ("hevc_nvenc", "h265"),
            ("av1_nvenc", "av1"),
        ];

        for (encoder, codec) in encoders {
            if self.check_encoder(encoder).await? {
                supported_codecs.push(codec.to_string());
            }
        }

        // Get max concurrent streams
        let max_streams = self.get_nvenc_max_streams().await.unwrap_or(2);

        Ok(HardwareEncoder {
            name: "NVENC".to_string(),
            encoder_type: HardwareEncoderType::Nvenc,
            supported_codecs: supported_codecs.clone(),
            max_streams,
            available: !supported_codecs.is_empty(),
        })
    }

    /// Check for Intel QSV support
    async fn check_qsv(&self) -> Result<HardwareEncoder> {
        debug!("Checking for QSV support");

        let mut supported_codecs = Vec::new();

        let encoders = &[
            ("h264_qsv", "h264"),
            ("hevc_qsv", "h265"),
            ("vp9_qsv", "vp9"),
            ("av1_qsv", "av1"),
        ];

        for (encoder, codec) in encoders {
            if self.check_encoder(encoder).await? {
                supported_codecs.push(codec.to_string());
            }
        }

        Ok(HardwareEncoder {
            name: "Intel Quick Sync".to_string(),
            encoder_type: HardwareEncoderType::Qsv,
            supported_codecs: supported_codecs.clone(),
            max_streams: 4,
            available: !supported_codecs.is_empty(),
        })
    }

    /// Check for VideoToolbox support (macOS)
    #[cfg(target_os = "macos")]
    async fn check_videotoolbox(&self) -> Result<HardwareEncoder> {
        debug!("Checking for VideoToolbox support");

        let mut supported_codecs = Vec::new();

        let encoders = &[
            ("h264_videotoolbox", "h264"),
            ("hevc_videotoolbox", "h265"),
        ];

        for (encoder, codec) in encoders {
            if self.check_encoder(encoder).await? {
                supported_codecs.push(codec.to_string());
            }
        }

        Ok(HardwareEncoder {
            name: "VideoToolbox".to_string(),
            encoder_type: HardwareEncoderType::VideoToolbox,
            supported_codecs,
            max_streams: 8,
            available: !supported_codecs.is_empty(),
        })
    }

    /// Check for AMD AMF support (Windows)
    #[cfg(target_os = "windows")]
    async fn check_amf(&self) -> Result<HardwareEncoder> {
        debug!("Checking for AMF support");

        let mut supported_codecs = Vec::new();

        let encoders = &[("h264_amf", "h264"), ("hevc_amf", "h265")];

        for (encoder, codec) in encoders {
            if self.check_encoder(encoder).await? {
                supported_codecs.push(codec.to_string());
            }
        }

        Ok(HardwareEncoder {
            name: "AMD AMF".to_string(),
            encoder_type: HardwareEncoderType::Amf,
            supported_codecs,
            max_streams: 4,
            available: !supported_codecs.is_empty(),
        })
    }

    /// Check if a specific encoder is available in FFmpeg
    async fn check_encoder(&self, encoder_name: &str) -> Result<bool> {
        let output = Command::new(&self.ffmpeg_path)
            .arg("-encoders")
            .output()
            .await
            .context("Failed to run ffmpeg")?;

        if !output.status.success() {
            return Ok(false);
        }

        let encoders = String::from_utf8_lossy(&output.stdout);
        Ok(encoders.contains(encoder_name))
    }

    /// Get maximum concurrent NVENC streams
    async fn get_nvenc_max_streams(&self) -> Result<usize> {
        // Try to query NVENC session limits
        let output = StdCommand::new("nvidia-smi")
            .arg("--query-gpu=encoder.stats.sessionCount")
            .arg("--format=csv,noheader")
            .output()
            .ok()
            .filter(|output| output.status.success());

        if let Some(output) = output {
            let result = String::from_utf8_lossy(&output.stdout);
            if let Ok(count) = result.trim().parse::<usize>() {
                return Ok(count);
            }
        }

        // Default based on GPU generation
        // Consumer GPUs typically support 3 concurrent streams
        // Professional GPUs support more
        Ok(3)
    }
}

/// Hardware encoder selector with fallback support
pub struct HardwareSelector {
    available_encoders: Vec<HardwareEncoder>,
    preferences: Vec<HardwareEncoderType>,
}

impl HardwareSelector {
    pub fn new(available_encoders: Vec<HardwareEncoder>) -> Self {
        // Default preference order
        let preferences = vec![
            HardwareEncoderType::Nvenc,
            HardwareEncoderType::Qsv,
            HardwareEncoderType::Vaapi,
            HardwareEncoderType::VideoToolbox,
            HardwareEncoderType::Amf,
        ];

        Self {
            available_encoders,
            preferences,
        }
    }

    /// Select the best hardware encoder for a codec
    pub fn select_encoder(&self, codec: &str) -> Option<&HardwareEncoder> {
        for pref in &self.preferences {
            if let Some(encoder) = self
                .available_encoders
                .iter()
                .find(|e| e.encoder_type == *pref && e.supported_codecs.contains(&codec.to_string()))
            {
                return Some(encoder);
            }
        }
        None
    }

    /// Get all available encoders for a codec
    pub fn get_encoders_for_codec(&self, codec: &str) -> Vec<&HardwareEncoder> {
        self.available_encoders
            .iter()
            .filter(|e| e.supported_codecs.contains(&codec.to_string()))
            .collect()
    }
}

/// Build hardware-specific FFmpeg arguments
pub struct HardwareArgs;

impl HardwareArgs {
    pub fn build_args(encoder: &HardwareEncoder, codec: &str) -> Vec<String> {
        let mut args = vec![];

        match encoder.encoder_type {
            HardwareEncoderType::Vaapi => {
                args.extend_from_slice(&[
                    "-hwaccel".to_string(),
                    "vaapi".to_string(),
                    "-hwaccel_device".to_string(),
                    "/dev/dri/renderD128".to_string(),
                    "-hwaccel_output_format".to_string(),
                    "vaapi".to_string(),
                ]);
            }
            HardwareEncoderType::Nvenc => {
                args.extend_from_slice(&[
                    "-hwaccel".to_string(),
                    "cuda".to_string(),
                    "-hwaccel_output_format".to_string(),
                    "cuda".to_string(),
                ]);
            }
            HardwareEncoderType::Qsv => {
                args.extend_from_slice(&[
                    "-hwaccel".to_string(),
                    "qsv".to_string(),
                    "-hwaccel_output_format".to_string(),
                    "qsv".to_string(),
                ]);
            }
            HardwareEncoderType::VideoToolbox => {
                args.extend_from_slice(&[
                    "-hwaccel".to_string(),
                    "videotoolbox".to_string(),
                ]);
            }
            HardwareEncoderType::Amf => {
                args.extend_from_slice(&[
                    "-hwaccel".to_string(),
                    "d3d11va".to_string(),
                ]);
            }
        }

        // Add codec-specific encoder
        let encoder_name = match (encoder.encoder_type, codec) {
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
            _ => return args, // Unsupported combination
        };

        args.extend_from_slice(&["-c:v".to_string(), encoder_name.to_string()]);

        args
    }
}