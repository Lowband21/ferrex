use crate::player::subtitle_types::*;
use iced::Color;
use log::{debug, warn};
use std::collections::HashMap;

/// ASS/SSA style definition
#[derive(Debug, Clone)]
struct AssStyle {
    font_name: String,
    font_size: f32,
    primary_color: Color,
    secondary_color: Color,
    bold: bool,
    italic: bool,
    underline: bool,
    strikeout: bool,
    alignment: i32,
    margin_l: i32,
    margin_r: i32,
    margin_v: i32,
}

/// Parse ASS/SSA subtitle format
pub fn parse_ass(data: &[u8], is_ssa: bool) -> Option<ParsedSubtitle> {
    let text = String::from_utf8_lossy(data);
    let mut styles: HashMap<String, AssStyle> = HashMap::new();
    let mut current_section = String::new();
    let mut dialogue_text = String::new();
    
    // Parse ASS file sections
    for line in text.lines() {
        let line = line.trim();
        
        // Section headers
        if line.starts_with('[') && line.ends_with(']') {
            current_section = line[1..line.len()-1].to_string();
            continue;
        }
        
        match current_section.as_str() {
            "V4 Styles" | "V4+ Styles" => {
                if line.starts_with("Style:") {
                    if let Some(style) = parse_style_line(&line[6..], is_ssa) {
                        styles.insert(style.0, style.1);
                    }
                }
            }
            "Events" => {
                if line.starts_with("Dialogue:") {
                    if let Some(text) = parse_dialogue_line(&line[9..]) {
                        dialogue_text = text;
                        break; // For now, just handle the first dialogue
                    }
                }
            }
            _ => {}
        }
    }
    
    if dialogue_text.is_empty() {
        return None;
    }
    
    // Parse the dialogue text with override tags
    let segments = parse_ass_text(&dialogue_text, &styles);
    
    if segments.is_empty() {
        return None;
    }
    
    debug!("Parsed ASS subtitle with {} segments", segments.len());
    
    Some(ParsedSubtitle::Text(TextSubtitle {
        segments,
        position: None, // TODO: Calculate from alignment
        background: None,
    }))
}

/// Parse ASS style line
fn parse_style_line(line: &str, is_ssa: bool) -> Option<(String, AssStyle)> {
    let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
    
    if parts.len() < 23 {
        warn!("Invalid style line: not enough fields");
        return None;
    }
    
    let name = parts[0].to_string();
    
    // Parse colors (ASS uses ABGR format)
    let primary_color = parse_ass_color(parts[2])?;
    let secondary_color = parse_ass_color(parts[3])?;
    
    let style = AssStyle {
        font_name: parts[1].to_string(),
        font_size: parts[2].parse().unwrap_or(20.0),
        primary_color,
        secondary_color,
        bold: parts[7] == "-1",
        italic: parts[8] == "-1",
        underline: parts[9] == "-1",
        strikeout: parts[10] == "-1",
        alignment: parts[18].parse().unwrap_or(2),
        margin_l: parts[19].parse().unwrap_or(0),
        margin_r: parts[20].parse().unwrap_or(0),
        margin_v: parts[21].parse().unwrap_or(0),
    };
    
    Some((name, style))
}

/// Parse ASS color format (&HAABBGGRR)
fn parse_ass_color(color_str: &str) -> Option<Color> {
    let color_str = color_str.trim_start_matches("&H");
    
    if color_str.len() < 6 {
        return None;
    }
    
    // Parse as ABGR (ignoring alpha)
    let bgr = &color_str[color_str.len()-6..];
    let b = u8::from_str_radix(&bgr[0..2], 16).ok()?;
    let g = u8::from_str_radix(&bgr[2..4], 16).ok()?;
    let r = u8::from_str_radix(&bgr[4..6], 16).ok()?;
    
    Some(Color::from_rgb8(r, g, b))
}

/// Parse dialogue line
fn parse_dialogue_line(line: &str) -> Option<String> {
    let parts: Vec<&str> = line.splitn(10, ',').collect();
    
    if parts.len() < 10 {
        return None;
    }
    
    // The text is in the last field
    Some(parts[9].to_string())
}

/// Parse ASS text with override tags
fn parse_ass_text(text: &str, styles: &HashMap<String, AssStyle>) -> Vec<StyledSegment> {
    let mut segments = Vec::new();
    let mut current_segment = StyledSegment {
        text: String::new(),
        style: TextStyle::default(),
    };
    
    // Set default color to white for subtitles
    current_segment.style.color = Some(Color::WHITE);
    
    let mut chars = text.chars().peekable();
    
    while let Some(ch) = chars.next() {
        if ch == '{' {
            // Override tag block
            let mut tag_content = String::new();
            let mut found_closing = false;
            
            while let Some(&next_ch) = chars.peek() {
                if next_ch == '}' {
                    chars.next();
                    found_closing = true;
                    break;
                }
                tag_content.push(chars.next().unwrap());
            }
            
            if found_closing {
                // Save current text
                if !current_segment.text.is_empty() {
                    segments.push(current_segment.clone());
                    current_segment.text.clear();
                }
                
                // Process override tags
                for tag in tag_content.split('\\').filter(|s| !s.is_empty()) {
                    process_ass_override_tag(tag, &mut current_segment.style);
                }
            } else {
                // Not a valid tag
                current_segment.text.push(ch);
                current_segment.text.push_str(&tag_content);
            }
        } else if ch == '\\' && chars.peek() == Some(&'N') {
            // \N is newline in ASS
            chars.next();
            current_segment.text.push('\n');
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

/// Process ASS override tag
fn process_ass_override_tag(tag: &str, style: &mut TextStyle) {
    if tag.is_empty() {
        return;
    }
    
    // Get tag name and value
    let tag_name = tag.chars().take_while(|c| c.is_alphabetic()).collect::<String>();
    let tag_value = tag[tag_name.len()..].trim();
    
    match tag_name.as_str() {
        "i" => {
            style.italic = tag_value == "1";
        }
        "b" => {
            style.bold = tag_value == "1" || tag_value == "700";
        }
        "u" => {
            style.underline = tag_value == "1";
        }
        "s" => {
            style.strikethrough = tag_value == "1";
        }
        "c" | "1c" => {
            // Primary color
            if let Some(color) = parse_ass_color(tag_value) {
                style.color = Some(color);
            }
        }
        "fs" => {
            // Font size
            if let Ok(size) = tag_value.parse::<f32>() {
                style.font_size = Some(size);
            }
        }
        "fn" => {
            // Font name
            style.font_name = Some(tag_value.to_string());
        }
        "k" | "K" => {
            // Karaoke timing
            if let Ok(duration) = tag_value.parse::<f32>() {
                style.karaoke_duration = Some(duration / 100.0); // Convert centiseconds to seconds
            }
        }
        _ => {
            debug!("Unhandled ASS override tag: {}{}", tag_name, tag_value);
        }
    }
}