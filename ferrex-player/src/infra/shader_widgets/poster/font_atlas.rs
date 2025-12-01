//! SDF Font Atlas generation using fontdue.
//!
//! Generates a Signed Distance Field texture atlas from a monospace font
//! for high-quality text rendering in the poster backface menu shader.

use std::collections::HashMap;

/// Embedded monospace font (JetBrains Mono Bold)
/// Available fonts in assets/fonts/:
///   - JetBrainsMono-Regular.ttf, JetBrainsMono-Bold.ttf
///   - FiraCode-Regular.ttf, FiraCode-Bold.ttf
// const EMBEDDED_FONT: &[u8] = include_bytes!("../../../../assets/fonts/JetBrainsMono-Bold.ttf");
const EMBEDDED_FONT: &[u8] =
    include_bytes!("../../../../assets/fonts/FiraCode-Bold.ttf");

/// Characters to include in the atlas
/// Layout: A-Z (0-25), a-z (26-51), 0-9 (52-61), space (62), punctuation (63+)
const ATLAS_CHARS: &str =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789 .-:!?'&,";

/// Total number of characters in the atlas
pub const ATLAS_CHAR_COUNT: usize = 71;

/// Convert a character to its glyph index in the atlas
/// Returns space index (62) for unknown characters
pub fn char_to_glyph_index(c: char) -> u8 {
    match c {
        'A'..='Z' => (c as u8) - b'A',      // 0-25
        'a'..='z' => 26 + (c as u8) - b'a', // 26-51
        '0'..='9' => 52 + (c as u8) - b'0', // 52-61
        ' ' => 62,
        '.' => 63,
        '-' => 64,
        ':' => 65,
        '!' => 66,
        '?' => 67,
        '\'' => 68,
        '&' => 69,
        ',' => 70,
        _ => 62, // Unknown -> space
    }
}

/// SDF generation parameters
const ATLAS_SIZE: u32 = 1024;
const GLYPH_SIZE: f32 = 64.0; // Render size for SDF generation (higher = sharper)
const SDF_PADDING: u32 = 8; // Padding for SDF spread (enables quality AA and effects)

/// Baseline position within each cell, as fraction from top (0.75 = 75% down)
/// This ensures all glyphs share a common baseline regardless of their height
const BASELINE_FRACTION: f32 = 0.75;

/// Glyph metrics for shader-side text rendering
#[derive(Debug, Clone, Copy)]
pub struct GlyphMetrics {
    /// UV coordinates in atlas (min_u, min_v, max_u, max_v)
    pub uv: [f32; 4],
    /// Horizontal advance (normalized to glyph height)
    pub advance: f32,
    /// Bearing offset (normalized)
    pub bearing_x: f32,
    pub bearing_y: f32,
    /// Glyph size (normalized to glyph height)
    pub width: f32,
    pub height: f32,
}

/// Font atlas containing SDF texture data and glyph metrics
#[derive(Debug, Clone)]
pub struct FontAtlas {
    /// RGBA texture data (SDF in R channel, others set to same value)
    pub texture_data: Vec<u8>,
    /// Atlas dimensions
    pub width: u32,
    pub height: u32,
    /// Glyph metrics indexed by character
    pub glyphs: HashMap<char, GlyphMetrics>,
    /// Line height for text layout (normalized)
    pub line_height: f32,
}

impl FontAtlas {
    /// Generate a new font atlas from the embedded font
    pub fn generate() -> Result<Self, FontAtlasError> {
        Self::generate_from_bytes(EMBEDDED_FONT)
    }

    /// Generate atlas from font bytes
    pub fn generate_from_bytes(
        font_bytes: &[u8],
    ) -> Result<Self, FontAtlasError> {
        use fontdue::{Font, FontSettings};

        // Parse font
        let font = Font::from_bytes(font_bytes, FontSettings::default())
            .map_err(|e| FontAtlasError::FontParse(e.to_string()))?;

        // Calculate atlas layout
        let chars: Vec<char> = ATLAS_CHARS.chars().collect();
        let glyphs_per_row =
            (ATLAS_SIZE / (GLYPH_SIZE as u32 + SDF_PADDING * 2)) as usize;
        let rows_needed = chars.len().div_ceil(glyphs_per_row);

        let atlas_width = ATLAS_SIZE;
        let atlas_height = ((rows_needed as u32)
            * (GLYPH_SIZE as u32 + SDF_PADDING * 2))
            .next_power_of_two()
            .max(256);

        // Create atlas buffer (RGBA)
        let mut texture_data =
            vec![0u8; (atlas_width * atlas_height * 4) as usize];
        let mut glyphs = HashMap::new();

        let cell_size = GLYPH_SIZE as u32 + SDF_PADDING * 2;

        for (i, &ch) in chars.iter().enumerate() {
            let col = i % glyphs_per_row;
            let row = i / glyphs_per_row;

            let cell_x = col as u32 * cell_size;
            let cell_y = row as u32 * cell_size;

            // Rasterize glyph
            let (metrics, bitmap) = font.rasterize(ch, GLYPH_SIZE);

            if metrics.width == 0 || metrics.height == 0 {
                // Space or empty glyph
                glyphs.insert(
                    ch,
                    GlyphMetrics {
                        uv: [0.0, 0.0, 0.0, 0.0],
                        advance: metrics.advance_width / GLYPH_SIZE,
                        bearing_x: 0.0,
                        bearing_y: 0.0,
                        width: 0.0,
                        height: 0.0,
                    },
                );
                continue;
            }

            // Generate SDF from bitmap
            let sdf = generate_sdf(
                &bitmap,
                metrics.width,
                metrics.height,
                SDF_PADDING as usize,
            );
            let sdf_width = metrics.width + SDF_PADDING as usize * 2;
            let sdf_height = metrics.height + SDF_PADDING as usize * 2;

            // Calculate baseline-aligned position within cell
            // baseline_in_cell is where the baseline should be (in pixels from cell top)
            let baseline_in_cell =
                (cell_size as f32 * BASELINE_FRACTION) as i32;

            // metrics.ymin is the offset from baseline to bottom of glyph (negative for descenders)
            // glyph top relative to baseline = -(height + ymin) = -height - ymin
            // So glyph top in cell = baseline_in_cell - height - ymin
            // SDF top = glyph top - padding
            let glyph_y_offset = baseline_in_cell
                - (metrics.height as i32)
                - metrics.ymin
                - (SDF_PADDING as i32);

            // Copy SDF to atlas with baseline-aligned positioning
            for sy in 0..sdf_height {
                for sx in 0..sdf_width {
                    let atlas_x = cell_x as usize + sx;
                    let atlas_y_signed =
                        cell_y as i32 + glyph_y_offset + sy as i32;

                    // Skip if outside atlas bounds
                    if atlas_y_signed < 0 {
                        continue;
                    }
                    let atlas_y = atlas_y_signed as usize;

                    if atlas_x < atlas_width as usize
                        && atlas_y < atlas_height as usize
                    {
                        let sdf_val = sdf[sy * sdf_width + sx];
                        let pixel_idx =
                            (atlas_y * atlas_width as usize + atlas_x) * 4;

                        // Store SDF in all channels (grayscale)
                        texture_data[pixel_idx] = sdf_val;
                        texture_data[pixel_idx + 1] = sdf_val;
                        texture_data[pixel_idx + 2] = sdf_val;
                        texture_data[pixel_idx + 3] = 255;
                    }
                }
            }

            // Calculate UV coordinates
            let u_min = cell_x as f32 / atlas_width as f32;
            let v_min = cell_y as f32 / atlas_height as f32;
            let u_max = (cell_x + sdf_width as u32) as f32 / atlas_width as f32;
            let v_max =
                (cell_y + sdf_height as u32) as f32 / atlas_height as f32;

            glyphs.insert(
                ch,
                GlyphMetrics {
                    uv: [u_min, v_min, u_max, v_max],
                    advance: metrics.advance_width / GLYPH_SIZE,
                    bearing_x: (metrics.xmin as f32 - SDF_PADDING as f32)
                        / GLYPH_SIZE,
                    bearing_y: (metrics.ymin as f32 - SDF_PADDING as f32)
                        / GLYPH_SIZE,
                    width: sdf_width as f32 / GLYPH_SIZE,
                    height: sdf_height as f32 / GLYPH_SIZE,
                },
            );
        }

        // Get line height from font metrics
        let line_height = font
            .horizontal_line_metrics(GLYPH_SIZE)
            .map(|m| m.new_line_size / GLYPH_SIZE)
            .unwrap_or(1.2);

        Ok(Self {
            texture_data,
            width: atlas_width,
            height: atlas_height,
            glyphs,
            line_height,
        })
    }

    /// Get glyph metrics for a character (returns space metrics for unknown chars)
    pub fn get_glyph(&self, ch: char) -> GlyphMetrics {
        // Try exact character first, then fallback to space
        self.glyphs
            .get(&ch)
            .or_else(|| self.glyphs.get(&' '))
            .copied()
            .unwrap_or(GlyphMetrics {
                uv: [0.0, 0.0, 0.0, 0.0],
                advance: 0.5,
                bearing_x: 0.0,
                bearing_y: 0.0,
                width: 0.0,
                height: 0.0,
            })
    }

    /// Calculate the rendered width of a string in normalized units (relative to font size)
    /// Returns (width, char_count) where char_count is capped at max_chars
    pub fn measure_text(&self, text: &str, max_chars: usize) -> (f32, usize) {
        let mut width = 0.0;
        let mut count = 0;
        for ch in text.chars().take(max_chars) {
            let glyph = self.get_glyph(ch);
            width += glyph.advance;
            count += 1;
        }
        (width, count)
    }

    /// Generate glyph data buffer for shader uniform
    /// Returns a packed array of glyph metrics for GPU upload
    pub fn generate_glyph_data(&self) -> Vec<f32> {
        // Pack glyph data for characters A-Z, 0-9, space, and punctuation
        // Each glyph: 8 floats (uv[4], advance, bearing_x, bearing_y, width)
        let mut data = Vec::new();

        for ch in ATLAS_CHARS.chars() {
            let glyph = self.get_glyph(ch);
            data.extend_from_slice(&glyph.uv);
            data.push(glyph.advance);
            data.push(glyph.bearing_x);
            data.push(glyph.bearing_y);
            data.push(glyph.width);
        }

        data
    }
}

/// Pack a string into an array of u32s for GPU upload.
/// Each u32 contains 4 character indices (8 bits each).
/// Returns (packed_data, actual_char_count).
pub fn pack_string(text: &str, max_chars: usize) -> (Vec<u32>, usize) {
    let word_count = max_chars.div_ceil(4);
    let mut packed = vec![0xFFFFFFFF_u32; word_count]; // Fill with 0xFF (null marker)

    let mut count = 0;
    for (i, ch) in text.chars().take(max_chars).enumerate() {
        let idx = char_to_glyph_index(ch);
        let word_index = i / 4;
        let byte_offset = (i % 4) * 8;
        // Clear the byte position and set the new value
        packed[word_index] = (packed[word_index] & !(0xFF << byte_offset))
            | ((idx as u32) << byte_offset);
        count += 1;
    }

    (packed, count)
}

/// Pack a title string (max 24 chars) into 6 u32s
pub fn pack_title(text: &str) -> ([u32; 6], usize) {
    let (packed, count) = pack_string(text, 24);
    let mut result = [0xFFFFFFFF_u32; 6];
    for (i, &val) in packed.iter().take(6).enumerate() {
        result[i] = val;
    }
    (result, count)
}

/// Pack a meta string (max 16 chars) into 4 u32s
pub fn pack_meta(text: &str) -> ([u32; 4], usize) {
    let (packed, count) = pack_string(text, 16);
    let mut result = [0xFFFFFFFF_u32; 4];
    for (i, &val) in packed.iter().take(4).enumerate() {
        result[i] = val;
    }
    (result, count)
}

/// Generate SDF from a grayscale bitmap
fn generate_sdf(
    bitmap: &[u8],
    width: usize,
    height: usize,
    padding: usize,
) -> Vec<u8> {
    let sdf_width = width + padding * 2;
    let sdf_height = height + padding * 2;
    let mut sdf = vec![0u8; sdf_width * sdf_height];

    let max_dist = padding as f32;

    for sy in 0..sdf_height {
        for sx in 0..sdf_width {
            // Find distance to nearest edge
            let mut min_dist = max_dist;

            // Check if this pixel is inside the glyph
            let bx = sx as i32 - padding as i32;
            let by = sy as i32 - padding as i32;
            let inside = if bx >= 0
                && bx < width as i32
                && by >= 0
                && by < height as i32
            {
                bitmap[by as usize * width + bx as usize] > 127
            } else {
                false
            };

            // Search for nearest edge pixel
            let search_range = padding as i32;
            for dy in -search_range..=search_range {
                for dx in -search_range..=search_range {
                    let check_bx = bx + dx;
                    let check_by = by + dy;

                    let check_inside = if check_bx >= 0
                        && check_bx < width as i32
                        && check_by >= 0
                        && check_by < height as i32
                    {
                        bitmap[check_by as usize * width + check_bx as usize]
                            > 127
                    } else {
                        false
                    };

                    // Found an edge transition
                    if check_inside != inside {
                        let dist = ((dx * dx + dy * dy) as f32).sqrt();
                        min_dist = min_dist.min(dist);
                    }
                }
            }

            // Convert to 0-255 range (128 = edge, >128 = inside, <128 = outside)
            let normalized = if inside {
                0.5 + (min_dist / max_dist) * 0.5
            } else {
                0.5 - (min_dist / max_dist) * 0.5
            };

            sdf[sy * sdf_width + sx] =
                (normalized.clamp(0.0, 1.0) * 255.0) as u8;
        }
    }

    sdf
}

#[derive(Debug)]
pub enum FontAtlasError {
    FontParse(String),
    AtlasGeneration(String),
}

impl std::fmt::Display for FontAtlasError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FontAtlasError::FontParse(e) => {
                write!(f, "Failed to parse font: {}", e)
            }
            FontAtlasError::AtlasGeneration(e) => {
                write!(f, "Failed to generate atlas: {}", e)
            }
        }
    }
}

impl std::error::Error for FontAtlasError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atlas_generation() {
        // This test will fail until we have the font file
        // For now, just test the SDF generation
        let bitmap = vec![
            0, 0, 0, 0, 0, 0, 255, 255, 255, 0, 0, 255, 255, 255, 0, 0, 255,
            255, 255, 0, 0, 0, 0, 0, 0,
        ];
        let sdf = generate_sdf(&bitmap, 5, 5, 2);
        assert_eq!(sdf.len(), 9 * 9);
        // Center should be high (inside)
        assert!(sdf[4 * 9 + 4] > 127);
        // Corners should be low (outside)
        assert!(sdf[0] < 127);
    }
}
