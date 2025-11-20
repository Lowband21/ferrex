use crate::player::subtitle_types::*;
use log::debug;

/// Parse SRT (SubRip) subtitle format
pub fn parse_srt(data: &[u8]) -> Option<ParsedSubtitle> {
    let text = String::from_utf8_lossy(data);
    let mut segments = Vec::new();
    let mut current_style = TextStyle::default();
    
    // SRT can have basic HTML-like tags
    let mut chars = text.chars().peekable();
    let mut current_text = String::new();
    
    while let Some(ch) = chars.next() {
        if ch == '<' {
            // Check for SRT tags
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
                // Save current text if any
                if !current_text.is_empty() {
                    segments.push(StyledSegment {
                        text: current_text.clone(),
                        style: current_style.clone(),
                    });
                    current_text.clear();
                }
                
                // Process tag
                let tag_lower = tag.to_lowercase();
                if tag_lower.starts_with('/') {
                    // Closing tag
                    match tag_lower[1..].trim() {
                        "i" => current_style.italic = false,
                        "b" => current_style.bold = false,
                        "u" => current_style.underline = false,
                        _ => {}
                    }
                } else {
                    // Opening tag
                    match tag_lower.trim() {
                        "i" => current_style.italic = true,
                        "b" => current_style.bold = true,
                        "u" => current_style.underline = true,
                        _ => {}
                    }
                }
            } else {
                // Not a valid tag
                current_text.push(ch);
                current_text.push_str(&tag);
            }
        } else {
            current_text.push(ch);
        }
    }
    
    // Add remaining text
    if !current_text.is_empty() {
        segments.push(StyledSegment {
            text: current_text,
            style: current_style,
        });
    }
    
    if segments.is_empty() {
        return None;
    }
    
    debug!("Parsed SRT subtitle with {} segments", segments.len());
    
    Some(ParsedSubtitle::Text(TextSubtitle {
        segments,
        position: None,
        background: None,
    }))
}