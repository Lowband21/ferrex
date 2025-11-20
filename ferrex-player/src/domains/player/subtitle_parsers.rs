use super::subtitle_types::*;
use iced::Color;
use log::{debug, warn};

mod srt;
mod webvtt;
mod ass;
mod pgs;

pub use srt::parse_srt;
pub use webvtt::parse_webvtt;
pub use ass::parse_ass;
pub use pgs::parse_pgs;

/// Parse subtitle data into a renderable format
pub fn parse_subtitle(data: &SubtitleData) -> Option<ParsedSubtitle> {
    match data.format {
        SubtitleFormat::PlainText => parse_plain_text(&data.data),
        SubtitleFormat::Srt => parse_srt(&data.data),
        SubtitleFormat::WebVtt => parse_webvtt(&data.data),
        SubtitleFormat::Ass | SubtitleFormat::Ssa => parse_ass(&data.data, data.format == SubtitleFormat::Ssa),
        SubtitleFormat::Html => parse_html(&data.data),
        SubtitleFormat::Pgs => parse_pgs(&data.data),
        SubtitleFormat::Dvb => parse_dvb(&data.data),
        SubtitleFormat::DvdSub => parse_dvd_sub(&data.data),
        SubtitleFormat::Unknown => {
            warn!("Unknown subtitle format, attempting plain text parse");
            parse_plain_text(&data.data)
        }
    }
}

/// Parse plain text subtitle
fn parse_plain_text(data: &[u8]) -> Option<ParsedSubtitle> {
    let text = String::from_utf8_lossy(data).to_string();
    if text.trim().is_empty() {
        return None;
    }

    Some(ParsedSubtitle::Text(TextSubtitle {
        segments: vec![StyledSegment {
            text,
            style: TextStyle::default(),
        }],
        position: None,
        background: None,
    }))
}

/// Parse HTML formatted subtitle (our existing parser adapted)
fn parse_html(data: &[u8]) -> Option<ParsedSubtitle> {
    let html = String::from_utf8_lossy(data).to_string();
    let segments = parse_html_to_segments(&html);

    if segments.is_empty() {
        return None;
    }

    Some(ParsedSubtitle::Text(TextSubtitle {
        segments,
        position: None,
        background: None,
    }))
}

/// Parse HTML-styled text into segments (adapted from our existing parser)
fn parse_html_to_segments(html: &str) -> Vec<StyledSegment> {
    let mut segments = Vec::new();
    let mut current_segment = StyledSegment {
        text: String::new(),
        style: TextStyle::default(),
    };

    let mut chars = html.chars().peekable();
    let mut style_stack: Vec<String> = Vec::new();

    while let Some(ch) = chars.next() {
        if ch == '<' {
            // Look ahead to see if this is really a tag
            let mut lookahead = chars.clone();
            let mut tag_content = String::new();
            let mut found_closing = false;

            // Look for closing > within reasonable distance (50 chars)
            for _ in 0..50 {
                if let Some(next_ch) = lookahead.next() {
                    if next_ch == '>' {
                        found_closing = true;
                        break;
                    }
                    tag_content.push(next_ch);
                }
            }

            // If we found a closing > and it looks like a valid tag, process it
            if found_closing && (tag_content.starts_with('/') ||
                                tag_content.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false)) {
                // Save current segment if it has text
                if !current_segment.text.is_empty() {
                    segments.push(current_segment.clone());
                    current_segment.text.clear();
                }

                // Parse tag
                let mut tag = String::new();
                let mut is_closing = false;

                if chars.peek() == Some(&'/') {
                    is_closing = true;
                    chars.next();
                }

                while let Some(tag_ch) = chars.next() {
                    if tag_ch == '>' {
                        break;
                    }
                    tag.push(tag_ch);
                }

                // Handle tag
                let tag_parts: Vec<&str> = tag.split_whitespace().collect();
                let tag_name = tag_parts.get(0).unwrap_or(&"").to_lowercase();

                if is_closing {
                    // Pop style from stack
                    if let Some(last_tag) = style_stack.last() {
                        if last_tag == &tag_name {
                            style_stack.pop();
                            match tag_name.as_str() {
                                "i" | "em" => current_segment.style.italic = false,
                                "b" | "strong" => current_segment.style.bold = false,
                                "u" => current_segment.style.underline = false,
                                "s" | "strike" => current_segment.style.strikethrough = false,
                                _ => {}
                            }
                        }
                    }
                } else {
                    // Push style to stack
                    style_stack.push(tag_name.clone());
                    match tag_name.as_str() {
                        "i" | "em" => current_segment.style.italic = true,
                        "b" | "strong" => current_segment.style.bold = true,
                        "u" => current_segment.style.underline = true,
                        "s" | "strike" => current_segment.style.strikethrough = true,
                        "br" | "br/" => {
                            // Add line break
                            current_segment.text.push('\n');
                        }
                        "font" => {
                            // Parse font attributes
                            for attr in tag_parts.iter().skip(1) {
                                if let Some(color) = attr.strip_prefix("color=") {
                                    current_segment.style.color = parse_html_color(color.trim_matches('"'));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            } else {
                // Not a valid tag, treat < as a regular character
                current_segment.text.push(ch);
            }
        } else {
            current_segment.text.push(ch);
        }
    }

    // Add final segment
    if !current_segment.text.is_empty() {
        segments.push(current_segment);
    }

    segments
}

/// Parse HTML color string to Color
fn parse_html_color(color: &str) -> Option<Color> {
    // Handle hex colors
    if let Some(hex) = color.strip_prefix('#') {
        if hex.len() == 6 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&hex[0..2], 16),
                u8::from_str_radix(&hex[2..4], 16),
                u8::from_str_radix(&hex[4..6], 16),
            ) {
                return Some(Color::from_rgb8(r, g, b));
            }
        }
    }

    // Handle named colors
    match color.to_lowercase().as_str() {
        "white" => Some(Color::WHITE),
        "black" => Some(Color::BLACK),
        "red" => Some(Color::from_rgb(1.0, 0.0, 0.0)),
        "green" => Some(Color::from_rgb(0.0, 1.0, 0.0)),
        "blue" => Some(Color::from_rgb(0.0, 0.0, 1.0)),
        "yellow" => Some(Color::from_rgb(1.0, 1.0, 0.0)),
        "cyan" => Some(Color::from_rgb(0.0, 1.0, 1.0)),
        "magenta" => Some(Color::from_rgb(1.0, 0.0, 1.0)),
        _ => None,
    }
}

/// Parse DVB subtitle (placeholder - needs proper implementation)
fn parse_dvb(data: &[u8]) -> Option<ParsedSubtitle> {
    warn!("DVB subtitle parsing not yet implemented");
    debug!("DVB data length: {} bytes", data.len());
    None
}

/// Parse DVD subtitle (placeholder - needs proper implementation)
fn parse_dvd_sub(data: &[u8]) -> Option<ParsedSubtitle> {
    warn!("DVD subtitle parsing not yet implemented");
    debug!("DVD sub data length: {} bytes", data.len());
    None
}
