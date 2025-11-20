use crate::player::subtitle_types::*;
use iced::Color;
use log::debug;

/// Parse WebVTT subtitle format
pub fn parse_webvtt(data: &[u8]) -> Option<ParsedSubtitle> {
    let text = String::from_utf8_lossy(data);
    
    // Skip WebVTT header
    let content = if text.starts_with("WEBVTT") {
        text.lines().skip(1).collect::<Vec<_>>().join("\n")
    } else {
        text.to_string()
    };
    
    let mut segments = Vec::new();
    let mut position = SubtitlePosition::default();
    
    // Parse the actual subtitle text (skip timing line)
    let mut in_text = false;
    for line in content.lines() {
        if line.contains("-->") {
            // This is a timing line, check for positioning
            if let Some(pos_part) = line.split("-->").nth(1) {
                position = parse_webvtt_position(pos_part);
            }
            in_text = true;
        } else if in_text && !line.trim().is_empty() {
            // Parse the subtitle text with WebVTT styling
            let parsed_segments = parse_webvtt_text(line);
            segments.extend(parsed_segments);
        } else if line.trim().is_empty() {
            in_text = false;
        }
    }
    
    if segments.is_empty() {
        return None;
    }
    
    debug!("Parsed WebVTT subtitle with {} segments", segments.len());
    
    Some(ParsedSubtitle::Text(TextSubtitle {
        segments,
        position: Some(position),
        background: None,
    }))
}

/// Parse WebVTT positioning settings
fn parse_webvtt_position(settings: &str) -> SubtitlePosition {
    let mut position = SubtitlePosition::default();
    
    for setting in settings.split_whitespace() {
        if let Some((key, value)) = setting.split_once(':') {
            match key {
                "line" => {
                    // Line position (percentage or number)
                    if let Some(percent) = value.strip_suffix('%') {
                        if let Ok(v) = percent.parse::<f32>() {
                            position.v_align = v / 100.0;
                        }
                    }
                }
                "position" => {
                    // Horizontal position
                    if let Some(percent) = value.strip_suffix('%') {
                        if let Ok(h) = percent.parse::<f32>() {
                            position.h_align = h / 100.0;
                        }
                    }
                }
                "align" => {
                    // Text alignment
                    match value {
                        "start" | "left" => position.h_align = 0.0,
                        "middle" | "center" => position.h_align = 0.5,
                        "end" | "right" => position.h_align = 1.0,
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
    
    position
}

/// Parse WebVTT text with styling tags
fn parse_webvtt_text(text: &str) -> Vec<StyledSegment> {
    let mut segments = Vec::new();
    let mut current_segment = StyledSegment {
        text: String::new(),
        style: TextStyle::default(),
    };
    
    let mut chars = text.chars().peekable();
    
    while let Some(ch) = chars.next() {
        if ch == '<' {
            // WebVTT tag
            let mut tag = String::new();
            let mut found_closing = false;
            
            while let Some(&next_ch) = chars.peek() {
                if next_ch == '>' {
                    chars.next();
                    found_closing = true;
                    break;
                }
                tag.push(chars.next().unwrap());
            }
            
            if found_closing {
                // Save current text
                if !current_segment.text.is_empty() {
                    segments.push(current_segment.clone());
                    current_segment.text.clear();
                }
                
                // Process tag
                let is_closing = tag.starts_with('/');
                let tag_content = if is_closing { &tag[1..] } else { &tag };
                
                // Parse tag name and classes
                let parts: Vec<&str> = tag_content.split('.').collect();
                let tag_name = parts[0].to_lowercase();
                
                if is_closing {
                    match tag_name.as_str() {
                        "i" => current_segment.style.italic = false,
                        "b" => current_segment.style.bold = false,
                        "u" => current_segment.style.underline = false,
                        "c" => {
                            // Voice span closed, reset color
                            current_segment.style.color = None;
                        }
                        _ => {}
                    }
                } else {
                    match tag_name.as_str() {
                        "i" => current_segment.style.italic = true,
                        "b" => current_segment.style.bold = true,
                        "u" => current_segment.style.underline = true,
                        "c" => {
                            // Voice span, check for color class
                            for class in parts.iter().skip(1) {
                                if let Some(color) = parse_webvtt_color_class(class) {
                                    current_segment.style.color = Some(color);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            } else {
                // Not a valid tag
                current_segment.text.push(ch);
                current_segment.text.push_str(&tag);
            }
        } else {
            current_segment.text.push(ch);
        }
    }
    
    // Add remaining text
    if !current_segment.text.is_empty() {
        segments.push(current_segment);
    }
    
    segments
}

/// Parse WebVTT color class
fn parse_webvtt_color_class(class: &str) -> Option<Color> {
    match class {
        "white" => Some(Color::WHITE),
        "lime" => Some(Color::from_rgb(0.0, 1.0, 0.0)),
        "cyan" => Some(Color::from_rgb(0.0, 1.0, 1.0)),
        "red" => Some(Color::from_rgb(1.0, 0.0, 0.0)),
        "yellow" => Some(Color::from_rgb(1.0, 1.0, 0.0)),
        "magenta" => Some(Color::from_rgb(1.0, 0.0, 1.0)),
        "blue" => Some(Color::from_rgb(0.0, 0.0, 1.0)),
        "black" => Some(Color::BLACK),
        _ => None,
    }
}