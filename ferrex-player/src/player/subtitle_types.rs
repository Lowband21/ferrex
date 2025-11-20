use iced::{Color, Point, Size};

/// Raw subtitle data with format information
#[derive(Debug, Clone)]
pub struct SubtitleData {
    /// Raw subtitle bytes
    pub data: Vec<u8>,
    /// Detected format
    pub format: SubtitleFormat,
    /// Presentation timestamp in seconds
    pub pts: f64,
    /// Duration in seconds
    pub duration: f64,
}

/// Detected subtitle format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleFormat {
    /// Plain text (no formatting)
    PlainText,
    /// SubRip format (.srt)
    Srt,
    /// WebVTT format
    WebVtt,
    /// Advanced SubStation Alpha
    Ass,
    /// SubStation Alpha
    Ssa,
    /// HTML formatted text
    Html,
    /// PGS (Presentation Graphic Stream) - Blu-ray bitmap
    Pgs,
    /// DVB (Digital Video Broadcasting) bitmap
    Dvb,
    /// DVD bitmap subtitles
    DvdSub,
    /// Unknown format
    Unknown,
}

impl SubtitleFormat {
    /// Detect format from raw data and codec info
    pub fn detect(data: &[u8], codec: Option<&str>) -> Self {
        // Check codec first if available
        if let Some(codec) = codec {
            let codec_lower = codec.to_lowercase();
            if codec_lower.contains("pgs") || codec_lower.contains("hdmv") {
                return SubtitleFormat::Pgs;
            } else if codec_lower.contains("dvb") {
                return SubtitleFormat::Dvb;
            } else if codec_lower.contains("dvd") || codec_lower.contains("vobsub") {
                return SubtitleFormat::DvdSub;
            } else if codec_lower.contains("ass") {
                return SubtitleFormat::Ass;
            } else if codec_lower.contains("ssa") {
                return SubtitleFormat::Ssa;
            } else if codec_lower.contains("webvtt") || codec_lower.contains("vtt") {
                return SubtitleFormat::WebVtt;
            } else if codec_lower.contains("srt") || codec_lower.contains("subrip") {
                return SubtitleFormat::Srt;
            }
        }
        
        // Try to detect from content
        if let Ok(text) = std::str::from_utf8(data) {
            // Check for text-based formats
            if text.starts_with("WEBVTT") {
                SubtitleFormat::WebVtt
            } else if text.contains("-->") && !text.starts_with("WEBVTT") {
                // SRT format has timestamps with -->
                SubtitleFormat::Srt
            } else if text.contains("[Script Info]") || text.contains("[V4+ Styles]") {
                SubtitleFormat::Ass
            } else if text.contains("[Script Info]") || text.contains("[V4 Styles]") {
                SubtitleFormat::Ssa
            } else if text.contains('<') && text.contains('>') {
                SubtitleFormat::Html
            } else {
                SubtitleFormat::PlainText
            }
        } else {
            // Binary format - check magic bytes
            if data.len() >= 2 {
                match (data[0], data[1]) {
                    (0x50, 0x47) => SubtitleFormat::Pgs, // "PG"
                    (0x00, 0x00) if data.len() > 4 && data[2] == 0x01 => SubtitleFormat::DvdSub,
                    _ => SubtitleFormat::Unknown,
                }
            } else {
                SubtitleFormat::Unknown
            }
        }
    }
    
    /// Check if this is a text-based format
    pub fn is_text_based(&self) -> bool {
        match self {
            SubtitleFormat::PlainText
            | SubtitleFormat::Srt
            | SubtitleFormat::WebVtt
            | SubtitleFormat::Ass
            | SubtitleFormat::Ssa
            | SubtitleFormat::Html => true,
            SubtitleFormat::Pgs
            | SubtitleFormat::Dvb
            | SubtitleFormat::DvdSub
            | SubtitleFormat::Unknown => false,
        }
    }
    
    /// Check if this is an image-based format
    pub fn is_image_based(&self) -> bool {
        !self.is_text_based()
    }
}

/// Parsed subtitle ready for rendering
#[derive(Debug, Clone)]
pub enum ParsedSubtitle {
    /// Text subtitle with optional styling
    Text(TextSubtitle),
    /// Image subtitle (bitmap)
    Image(ImageSubtitle),
}

/// Text subtitle with segments and styling
#[derive(Debug, Clone)]
pub struct TextSubtitle {
    /// Styled text segments
    pub segments: Vec<StyledSegment>,
    /// Overall position (if specified)
    pub position: Option<SubtitlePosition>,
    /// Background color (if any)
    pub background: Option<Color>,
}

/// Styled text segment
#[derive(Debug, Clone)]
pub struct StyledSegment {
    pub text: String,
    pub style: TextStyle,
}

/// Text styling information
#[derive(Debug, Clone)]
pub struct TextStyle {
    pub italic: bool,
    pub bold: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub color: Option<Color>,
    pub font_size: Option<f32>,
    pub font_name: Option<String>,
    /// For karaoke effects (ASS/SSA)
    pub karaoke_start: Option<f32>,
    pub karaoke_duration: Option<f32>,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            italic: false,
            bold: false,
            underline: false,
            strikethrough: false,
            color: None,
            font_size: None,
            font_name: None,
            karaoke_start: None,
            karaoke_duration: None,
        }
    }
}

/// Image subtitle (PGS/DVB/DVD)
#[derive(Debug, Clone)]
pub struct ImageSubtitle {
    /// RGBA bitmap data
    pub rgba_data: Vec<u8>,
    /// Image dimensions
    pub width: u32,
    pub height: u32,
    /// Position on screen
    pub position: Point,
    /// Palette (for formats that use indexed colors)
    pub palette: Option<Vec<Color>>,
}

/// Subtitle positioning
#[derive(Debug, Clone, Copy)]
pub struct SubtitlePosition {
    /// Horizontal alignment (0.0 = left, 0.5 = center, 1.0 = right)
    pub h_align: f32,
    /// Vertical alignment (0.0 = top, 0.5 = center, 1.0 = bottom)
    pub v_align: f32,
    /// Margin from edges in pixels
    pub margin: SubtitleMargin,
}

impl Default for SubtitlePosition {
    fn default() -> Self {
        Self {
            h_align: 0.5,  // Center
            v_align: 1.0,  // Bottom
            margin: SubtitleMargin::default(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SubtitleMargin {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

impl Default for SubtitleMargin {
    fn default() -> Self {
        Self {
            left: 20.0,
            right: 20.0,
            top: 20.0,
            bottom: 80.0,  // More space at bottom for controls
        }
    }
}