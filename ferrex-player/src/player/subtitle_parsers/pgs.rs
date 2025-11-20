use crate::player::subtitle_types::*;
use iced::{Color, Point};
use log::{debug, warn};

// PGS segment types
const PGS_PALETTE_SEGMENT: u8 = 0x14;
const PGS_OBJECT_SEGMENT: u8 = 0x15;
const PGS_PRESENTATION_SEGMENT: u8 = 0x16;
const PGS_WINDOW_SEGMENT: u8 = 0x17;
const PGS_END_SEGMENT: u8 = 0x80;

/// PGS composition state
#[derive(Debug)]
struct PgsComposition {
    width: u16,
    height: u16,
    windows: Vec<PgsWindow>,
    objects: Vec<PgsObject>,
}

#[derive(Debug)]
struct PgsWindow {
    id: u8,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
}

#[derive(Debug)]
struct PgsObject {
    id: u16,
    window_id: u8,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    data: Vec<u8>,
}

/// Parse PGS (Presentation Graphic Stream) subtitle
pub fn parse_pgs(data: &[u8]) -> Option<ParsedSubtitle> {
    if data.len() < 13 {
        warn!("PGS data too short: {} bytes", data.len());
        return None;
    }
    
    // Check magic bytes
    if data[0] != b'P' || data[1] != b'G' {
        warn!("Invalid PGS magic bytes");
        return None;
    }
    
    let mut composition = PgsComposition {
        width: 1920,  // Default HD resolution
        height: 1080,
        windows: Vec::new(),
        objects: Vec::new(),
    };
    
    let mut palette: Vec<Color> = Vec::new();
    let mut offset = 0;
    
    // Parse segments
    while offset + 13 <= data.len() {
        // Skip to segment type
        let segment_type = data[offset + 10];
        let segment_size = u16::from_be_bytes([data[offset + 11], data[offset + 12]]) as usize;
        
        if offset + 13 + segment_size > data.len() {
            warn!("PGS segment extends beyond data");
            break;
        }
        
        let segment_data = &data[offset + 13..offset + 13 + segment_size];
        
        match segment_type {
            PGS_PALETTE_SEGMENT => {
                palette = parse_pgs_palette(segment_data);
                debug!("Parsed PGS palette with {} colors", palette.len());
            }
            PGS_OBJECT_SEGMENT => {
                if let Some(object) = parse_pgs_object(segment_data) {
                    debug!("Parsed PGS object: {}x{} at ({}, {})", 
                           object.width, object.height, object.x, object.y);
                    composition.objects.push(object);
                }
            }
            PGS_PRESENTATION_SEGMENT => {
                parse_pgs_presentation(segment_data, &mut composition);
            }
            PGS_WINDOW_SEGMENT => {
                if let Some(window) = parse_pgs_window(segment_data) {
                    composition.windows.push(window);
                }
            }
            PGS_END_SEGMENT => {
                debug!("Reached PGS end segment");
                break;
            }
            _ => {
                debug!("Unknown PGS segment type: 0x{:02x}", segment_type);
            }
        }
        
        offset += 13 + segment_size;
    }
    
    // Create image from the first object (simplified)
    if let Some(object) = composition.objects.first() {
        if palette.is_empty() {
            warn!("No palette found for PGS subtitle");
            return None;
        }
        
        // Decode RLE compressed image data
        let rgba_data = decode_pgs_rle(&object.data, object.width, object.height, &palette)?;
        
        let image = ImageSubtitle {
            rgba_data,
            width: object.width as u32,
            height: object.height as u32,
            position: Point::new(object.x as f32, object.y as f32),
            palette: Some(palette),
        };
        
        debug!("Created PGS image subtitle: {}x{} at ({}, {})", 
               image.width, image.height, image.position.x, image.position.y);
        
        return Some(ParsedSubtitle::Image(image));
    }
    
    None
}

/// Parse PGS palette segment
fn parse_pgs_palette(data: &[u8]) -> Vec<Color> {
    let mut palette = Vec::new();
    
    if data.len() < 2 {
        return palette;
    }
    
    let _palette_id = data[0];
    let _palette_version = data[1];
    
    // Each palette entry is 5 bytes: index, Y, Cr, Cb, Alpha
    let mut offset = 2;
    while offset + 5 <= data.len() {
        let _index = data[offset];
        let y = data[offset + 1];
        let cr = data[offset + 2];
        let cb = data[offset + 3];
        let alpha = data[offset + 4];
        
        // Convert YCrCb to RGB
        let (r, g, b) = ycrcb_to_rgb(y, cr, cb);
        let color = Color::from_rgba8(r, g, b, alpha);
        
        palette.push(color);
        offset += 5;
    }
    
    palette
}

/// Convert YCrCb to RGB
fn ycrcb_to_rgb(y: u8, cr: u8, cb: u8) -> (u8, u8, u8) {
    let y = y as f32;
    let cr = cr as f32 - 128.0;
    let cb = cb as f32 - 128.0;
    
    let r = (y + 1.402 * cr).clamp(0.0, 255.0) as u8;
    let g = (y - 0.34414 * cb - 0.71414 * cr).clamp(0.0, 255.0) as u8;
    let b = (y + 1.772 * cb).clamp(0.0, 255.0) as u8;
    
    (r, g, b)
}

/// Parse PGS object segment
fn parse_pgs_object(data: &[u8]) -> Option<PgsObject> {
    if data.len() < 8 {
        return None;
    }
    
    let object_id = u16::from_be_bytes([data[0], data[1]]);
    let _version = data[2];
    let _sequence = data[3];
    
    // Check if this is the last fragment
    let last_fragment = (data[3] & 0x40) != 0;
    let first_fragment = (data[3] & 0x80) != 0;
    
    if first_fragment {
        let data_length = u16::from_be_bytes([data[4], data[5]]) as usize;
        let width = u16::from_be_bytes([data[6], data[7]]);
        let height = u16::from_be_bytes([data[8], data[9]]);
        
        let object_data = data[10..].to_vec();
        
        return Some(PgsObject {
            id: object_id,
            window_id: 0,
            x: 0,
            y: 0,
            width,
            height,
            data: object_data,
        });
    }
    
    None
}

/// Parse PGS presentation segment
fn parse_pgs_presentation(data: &[u8], composition: &mut PgsComposition) {
    if data.len() < 11 {
        return;
    }
    
    composition.width = u16::from_be_bytes([data[0], data[1]]);
    composition.height = u16::from_be_bytes([data[2], data[3]]);
    
    let _frame_rate = data[4];
    let composition_number = u16::from_be_bytes([data[5], data[6]]);
    let composition_state = data[7];
    let _palette_update = data[8];
    let palette_id = data[9];
    let object_count = data[10];
    
    debug!("PGS presentation: {}x{}, state={}, objects={}", 
           composition.width, composition.height, composition_state, object_count);
    
    // Parse object positions
    let mut offset = 11;
    for _ in 0..object_count {
        if offset + 8 > data.len() {
            break;
        }
        
        let object_id = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let window_id = data[offset + 2];
        let x = u16::from_be_bytes([data[offset + 3], data[offset + 4]]);
        let y = u16::from_be_bytes([data[offset + 5], data[offset + 6]]);
        
        // Update object position
        for object in &mut composition.objects {
            if object.id == object_id {
                object.window_id = window_id;
                object.x = x;
                object.y = y;
                break;
            }
        }
        
        offset += 8;
    }
}

/// Parse PGS window segment
fn parse_pgs_window(data: &[u8]) -> Option<PgsWindow> {
    if data.len() < 9 {
        return None;
    }
    
    let window_id = data[0];
    let x = u16::from_be_bytes([data[1], data[2]]);
    let y = u16::from_be_bytes([data[3], data[4]]);
    let width = u16::from_be_bytes([data[5], data[6]]);
    let height = u16::from_be_bytes([data[7], data[8]]);
    
    Some(PgsWindow {
        id: window_id,
        x,
        y,
        width,
        height,
    })
}

/// Decode PGS RLE compressed data
fn decode_pgs_rle(data: &[u8], width: u16, height: u16, palette: &[Color]) -> Option<Vec<u8>> {
    let mut rgba_data = vec![0u8; (width as usize * height as usize * 4)];
    let mut pixel_index = 0;
    let mut data_offset = 0;
    
    while data_offset < data.len() && pixel_index < width as usize * height as usize {
        let byte = data[data_offset];
        data_offset += 1;
        
        if byte == 0 {
            // Special codes
            if data_offset >= data.len() {
                break;
            }
            
            let next_byte = data[data_offset];
            data_offset += 1;
            
            if next_byte == 0 {
                // End of line
                let current_x = pixel_index % width as usize;
                if current_x > 0 {
                    pixel_index += width as usize - current_x;
                }
            } else if (next_byte & 0xC0) == 0x40 {
                // Run of pixels with palette index 0 (transparent)
                let count = ((next_byte & 0x3F) as usize) << 8;
                if data_offset < data.len() {
                    let count = count | data[data_offset] as usize;
                    data_offset += 1;
                    
                    for _ in 0..count {
                        if pixel_index >= width as usize * height as usize {
                            break;
                        }
                        // Transparent pixel (already zeroed)
                        pixel_index += 1;
                    }
                }
            } else if (next_byte & 0xC0) == 0x80 {
                // Run of pixels with specific palette index
                let count = (next_byte & 0x3F) as usize;
                if data_offset < data.len() {
                    let palette_index = data[data_offset] as usize;
                    data_offset += 1;
                    
                    if let Some(color) = palette.get(palette_index) {
                        for _ in 0..count {
                            if pixel_index >= width as usize * height as usize {
                                break;
                            }
                            
                            let offset = pixel_index * 4;
                            rgba_data[offset] = (color.r * 255.0) as u8;
                            rgba_data[offset + 1] = (color.g * 255.0) as u8;
                            rgba_data[offset + 2] = (color.b * 255.0) as u8;
                            rgba_data[offset + 3] = (color.a * 255.0) as u8;
                            
                            pixel_index += 1;
                        }
                    }
                }
            } else if (next_byte & 0xC0) == 0xC0 {
                // Long run
                let count = ((next_byte & 0x3F) as usize) << 8;
                if data_offset + 1 < data.len() {
                    let count = count | data[data_offset] as usize;
                    let palette_index = data[data_offset + 1] as usize;
                    data_offset += 2;
                    
                    if let Some(color) = palette.get(palette_index) {
                        for _ in 0..count {
                            if pixel_index >= width as usize * height as usize {
                                break;
                            }
                            
                            let offset = pixel_index * 4;
                            rgba_data[offset] = (color.r * 255.0) as u8;
                            rgba_data[offset + 1] = (color.g * 255.0) as u8;
                            rgba_data[offset + 2] = (color.b * 255.0) as u8;
                            rgba_data[offset + 3] = (color.a * 255.0) as u8;
                            
                            pixel_index += 1;
                        }
                    }
                }
            } else {
                // Run of pixels with palette index 0
                let count = next_byte as usize;
                pixel_index += count;
            }
        } else {
            // Single pixel with palette index
            if let Some(color) = palette.get(byte as usize) {
                let offset = pixel_index * 4;
                if offset + 3 < rgba_data.len() {
                    rgba_data[offset] = (color.r * 255.0) as u8;
                    rgba_data[offset + 1] = (color.g * 255.0) as u8;
                    rgba_data[offset + 2] = (color.b * 255.0) as u8;
                    rgba_data[offset + 3] = (color.a * 255.0) as u8;
                }
            }
            pixel_index += 1;
        }
    }
    
    Some(rgba_data)
}